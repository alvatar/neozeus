use crate::shared::text_cursor::{
    next_char_boundary, previous_char_boundary, word_backward_boundary, word_forward_boundary,
};

use super::state::{
    TextEditorDraft, TextEditorState, TextEditorYankState, TEXT_EDITOR_KILL_RING_LIMIT,
};

impl TextEditorState {
    /// Handles close.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        #[cfg(test)]
        {
            self.target_terminal = None;
        }
        self.mark = None;
        self.preferred_column = None;
        self.yank_state = None;
    }

    /// Closes and discard.
    pub(crate) fn close_and_discard(&mut self) {
        self.visible = false;
        #[cfg(test)]
        {
            self.target_terminal = None;
        }
        self.clear_editor();
    }

    /// Loads text.
    pub(crate) fn load_text(&mut self, text: &str) {
        self.clear_editor();
        self.text = normalize_text(text);
        self.cursor = self.text.len();
    }

    /// Clears editor.
    fn clear_editor(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.mark = None;
        self.preferred_column = None;
        self.yank_state = None;
    }

    /// Handles snapshot draft.
    pub(super) fn snapshot_draft(&self) -> TextEditorDraft {
        TextEditorDraft {
            text: self.text.clone(),
            cursor: self.cursor,
            mark: self.mark,
            preferred_column: self.preferred_column,
        }
    }

    /// Restores draft.
    pub(super) fn restore_draft(&mut self, draft: TextEditorDraft) {
        self.visible = true;
        self.text = draft.text;
        self.cursor = draft.cursor.min(self.text.len());
        self.mark = draft.mark.map(|mark| mark.min(self.text.len()));
        self.preferred_column = draft.preferred_column;
        self.yank_state = None;
    }

    /// Returns the current marked region bounds when a non-empty region is active.
    pub(crate) fn region_bounds(&self) -> Option<(usize, usize)> {
        let mark = self.mark?;
        (mark != self.cursor).then_some((mark.min(self.cursor), mark.max(self.cursor)))
    }

    /// Sets mark.
    pub(crate) fn set_mark(&mut self) -> bool {
        let changed = self.mark != Some(self.cursor);
        self.mark = Some(self.cursor);
        self.yank_state = None;
        changed
    }

    /// Handles insert text.
    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        let inserted = self.insert_text_internal(self.cursor, text, true);
        if inserted == 0 {
            return false;
        }
        self.cursor += inserted;
        self.preferred_column = None;
        true
    }

    /// Inserts a newline at the cursor position.
    pub(crate) fn insert_newline(&mut self) -> bool {
        self.insert_text("\n")
    }

    /// Inserts a newline using the editor newline-and-indent behavior.
    pub(crate) fn newline_and_indent(&mut self) -> bool {
        self.insert_newline()
    }

    /// Opens a new line at the cursor without moving past it.
    pub(crate) fn open_line(&mut self) -> bool {
        let inserted = self.insert_text_internal(self.cursor, "\n", true);
        if inserted == 0 {
            return false;
        }
        self.preferred_column = None;
        true
    }

    /// Moves left.
    pub(crate) fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    /// Moves right.
    pub(crate) fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = next;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    /// Moves line start.
    pub(crate) fn move_line_start(&mut self) -> bool {
        let (line_start, _) = current_line_bounds(&self.text, self.cursor);
        if self.cursor == line_start {
            return false;
        }
        self.cursor = line_start;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    /// Moves line end.
    pub(crate) fn move_line_end(&mut self) -> bool {
        let (_, line_end) = current_line_bounds(&self.text, self.cursor);
        if self.cursor == line_end {
            return false;
        }
        self.cursor = line_end;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    /// Moves up.
    pub(crate) fn move_up(&mut self) -> bool {
        // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
        let (line_start, _) = current_line_bounds(&self.text, self.cursor);
        if line_start == 0 {
            return false;
        }
        let target_column = self
            .preferred_column
            .unwrap_or_else(|| current_line_column_chars(&self.text, self.cursor));
        let previous_line_end = line_start - 1;
        let previous_line_start = self.text[..previous_line_end]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        self.cursor = advance_by_chars(
            &self.text,
            previous_line_start,
            previous_line_end,
            target_column,
        );
        self.preferred_column = Some(target_column);
        self.yank_state = None;
        true
    }

    /// Moves down.
    pub(crate) fn move_down(&mut self) -> bool {
        let (_, line_end) = current_line_bounds(&self.text, self.cursor);
        if line_end >= self.text.len() {
            return false;
        }
        let target_column = self
            .preferred_column
            .unwrap_or_else(|| current_line_column_chars(&self.text, self.cursor));
        let next_line_start = line_end + 1;
        let next_line_end = self.text[next_line_start..]
            .find('\n')
            .map(|offset| next_line_start + offset)
            .unwrap_or(self.text.len());
        self.cursor = advance_by_chars(&self.text, next_line_start, next_line_end, target_column);
        self.preferred_column = Some(target_column);
        self.yank_state = None;
        true
    }

    /// Moves word backward.
    pub(crate) fn move_word_backward(&mut self) -> bool {
        let boundary = word_backward_boundary(&self.text, self.cursor, is_word_char);
        if boundary == self.cursor {
            return false;
        }
        self.cursor = boundary;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    /// Moves word forward.
    pub(crate) fn move_word_forward(&mut self) -> bool {
        let boundary = word_forward_boundary(&self.text, self.cursor, is_word_char);
        if boundary == self.cursor {
            return false;
        }
        self.cursor = boundary;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    /// Deletes backward char.
    pub(crate) fn delete_backward_char(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.delete_range_internal(previous, self.cursor, true)
            .is_some()
    }

    /// Deletes forward char.
    pub(crate) fn delete_forward_char(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.delete_range_internal(self.cursor, next, true)
            .is_some()
    }

    /// Copies region.
    pub(crate) fn copy_region(&mut self) -> bool {
        let Some((start, end)) = self.region_bounds() else {
            return false;
        };
        let copied = self.text[start..end].to_owned();
        let pushed = self.push_kill(copied);
        self.mark = None;
        self.yank_state = None;
        pushed
    }

    /// Deletes region and updates the kill-ring state.
    pub(crate) fn kill_region(&mut self) -> bool {
        let Some((start, end)) = self.region_bounds() else {
            return false;
        };
        let Some(killed) = self.delete_range_internal(start, end, true) else {
            return false;
        };
        self.mark = None;
        self.push_kill(killed)
    }

    /// Deletes to end of line and updates the kill-ring state.
    pub(crate) fn kill_to_end_of_line(&mut self) -> bool {
        let (_, line_end) = current_line_bounds(&self.text, self.cursor);
        let kill_end = if self.cursor == line_end && line_end < self.text.len() {
            line_end + 1
        } else {
            line_end
        };
        let Some(killed) = self.delete_range_internal(self.cursor, kill_end, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    /// Deletes word forward and updates the kill-ring state.
    pub(crate) fn kill_word_forward(&mut self) -> bool {
        let boundary = word_forward_boundary(&self.text, self.cursor, is_word_char);
        let Some(killed) = self.delete_range_internal(self.cursor, boundary, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    /// Deletes word backward and updates the kill-ring state.
    pub(crate) fn kill_word_backward(&mut self) -> bool {
        let boundary = word_backward_boundary(&self.text, self.cursor, is_word_char);
        let Some(killed) = self.delete_range_internal(boundary, self.cursor, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    /// Inserts the most recent kill-ring entry at the cursor.
    pub(crate) fn yank(&mut self) -> bool {
        let Some(payload) = self.kill_ring.first().cloned() else {
            return false;
        };
        let start = self.cursor;
        let inserted = self.insert_text_internal(start, &payload, false);
        if inserted == 0 {
            return false;
        }
        self.cursor = start + inserted;
        self.preferred_column = None;
        self.yank_state = Some(TextEditorYankState {
            start,
            end: start + inserted,
            ring_index: 0,
        });
        true
    }

    /// Replaces the last yank with the previous kill-ring entry.
    pub(crate) fn yank_pop(&mut self) -> bool {
        // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
        let Some(yank_state) = self.yank_state else {
            return false;
        };
        if self.kill_ring.len() < 2 {
            return false;
        }
        let next_ring_index = (yank_state.ring_index + 1) % self.kill_ring.len();
        let replacement = self.kill_ring[next_ring_index].clone();
        let _ = self.delete_range_internal(yank_state.start, yank_state.end, false);
        let inserted = self.insert_text_internal(yank_state.start, &replacement, false);
        self.cursor = yank_state.start + inserted;
        self.preferred_column = None;
        self.yank_state = Some(TextEditorYankState {
            start: yank_state.start,
            end: yank_state.start + inserted,
            ring_index: next_ring_index,
        });
        true
    }

    /// Returns the cursor line and column in character coordinates.
    pub(crate) fn cursor_line_and_column(&self) -> (usize, usize) {
        (
            self.text[..self.cursor]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
            current_line_column_chars(&self.text, self.cursor),
        )
    }

    /// Handles insert text internal.
    fn insert_text_internal(&mut self, at: usize, text: &str, clear_yank_state: bool) -> usize {
        // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
        let normalized = normalize_text(text);
        if normalized.is_empty() {
            return 0;
        }
        let inserted = normalized.len();
        if let Some(mark) = self.mark.as_mut() {
            if *mark >= at {
                *mark += inserted;
            }
        }
        if let Some(yank_state) = self.yank_state.as_mut() {
            if yank_state.start >= at {
                yank_state.start += inserted;
            }
            if yank_state.end >= at {
                yank_state.end += inserted;
            }
        }
        self.text.insert_str(at, &normalized);
        if clear_yank_state {
            self.yank_state = None;
        }
        inserted
    }

    /// Deletes range internal.
    fn delete_range_internal(
        &mut self,
        start: usize,
        end: usize,
        clear_yank_state: bool,
    ) -> Option<String> {
        // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
        if start >= end {
            return None;
        }
        let removed = self.text[start..end].to_owned();
        self.text.drain(start..end);
        self.cursor = adjust_index_after_delete(self.cursor, start, end);
        if let Some(mark) = self.mark.as_mut() {
            *mark = adjust_index_after_delete(*mark, start, end);
        }
        if clear_yank_state {
            self.yank_state = None;
        } else if let Some(yank_state) = self.yank_state.as_mut() {
            yank_state.start = adjust_index_after_delete(yank_state.start, start, end);
            yank_state.end = adjust_index_after_delete(yank_state.end, start, end);
            if yank_state.start >= yank_state.end {
                self.yank_state = None;
            }
        }
        self.preferred_column = None;
        Some(removed)
    }

    /// Appends kill.
    fn push_kill(&mut self, text: String) -> bool {
        if text.is_empty() {
            return false;
        }
        self.kill_ring.insert(0, text);
        if self.kill_ring.len() > TEXT_EDITOR_KILL_RING_LIMIT {
            self.kill_ring.truncate(TEXT_EDITOR_KILL_RING_LIMIT);
        }
        self.yank_state = None;
        true
    }
}

/// Normalizes editor text into the canonical internal newline representation.
fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Handles adjust index after delete.
fn adjust_index_after_delete(index: usize, start: usize, end: usize) -> usize {
    if index <= start {
        index
    } else if index <= end {
        start
    } else {
        index - (end - start)
    }
}

/// Handles current line bounds.
fn current_line_bounds(text: &str, cursor: usize) -> (usize, usize) {
    let line_start = text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let line_end = text[cursor..]
        .find('\n')
        .map(|offset| cursor + offset)
        .unwrap_or(text.len());
    (line_start, line_end)
}

/// Handles current line column chars.
fn current_line_column_chars(text: &str, cursor: usize) -> usize {
    let (line_start, _) = current_line_bounds(text, cursor);
    text[line_start..cursor].chars().count()
}

/// Advances by chars.
fn advance_by_chars(text: &str, start: usize, end: usize, count: usize) -> usize {
    let mut cursor = start;
    for ch in text[start..end].chars().take(count) {
        cursor += ch.len_utf8();
    }
    if count >= text[start..end].chars().count() {
        end
    } else {
        cursor
    }
}

/// Returns whether word char.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
