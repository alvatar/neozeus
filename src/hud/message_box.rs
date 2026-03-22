use crate::terminals::TerminalId;

const HUD_MESSAGE_BOX_KILL_RING_LIMIT: usize = 32;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HudMessageBoxYankState {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) ring_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HudMessageBoxState {
    pub(crate) visible: bool,
    pub(crate) target_terminal: Option<TerminalId>,
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) mark: Option<usize>,
    pub(crate) preferred_column: Option<usize>,
    pub(crate) kill_ring: Vec<String>,
    pub(crate) yank_state: Option<HudMessageBoxYankState>,
}

impl HudMessageBoxState {
    pub(crate) fn reset_for_target(&mut self, target_terminal: TerminalId) {
        self.visible = true;
        self.target_terminal = Some(target_terminal);
        self.text.clear();
        self.cursor = 0;
        self.mark = None;
        self.preferred_column = None;
        self.yank_state = None;
    }

    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.target_terminal = None;
        self.text.clear();
        self.cursor = 0;
        self.mark = None;
        self.preferred_column = None;
        self.yank_state = None;
    }

    pub(crate) fn region_bounds(&self) -> Option<(usize, usize)> {
        let mark = self.mark?;
        (mark != self.cursor).then_some((mark.min(self.cursor), mark.max(self.cursor)))
    }

    pub(crate) fn set_mark(&mut self) -> bool {
        let changed = self.mark != Some(self.cursor);
        self.mark = Some(self.cursor);
        self.yank_state = None;
        changed
    }

    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        let inserted = self.insert_text_internal(self.cursor, text, true);
        if inserted == 0 {
            return false;
        }
        self.cursor += inserted;
        self.preferred_column = None;
        true
    }

    pub(crate) fn insert_newline(&mut self) -> bool {
        self.insert_text("\n")
    }

    pub(crate) fn newline_and_indent(&mut self) -> bool {
        self.insert_newline()
    }

    pub(crate) fn open_line(&mut self) -> bool {
        let inserted = self.insert_text_internal(self.cursor, "\n", true);
        if inserted == 0 {
            return false;
        }
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    pub(crate) fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = next;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

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

    pub(crate) fn move_up(&mut self) -> bool {
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

    pub(crate) fn move_word_backward(&mut self) -> bool {
        let boundary = word_backward_boundary(&self.text, self.cursor);
        if boundary == self.cursor {
            return false;
        }
        self.cursor = boundary;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    pub(crate) fn move_word_forward(&mut self) -> bool {
        let boundary = word_forward_boundary(&self.text, self.cursor);
        if boundary == self.cursor {
            return false;
        }
        self.cursor = boundary;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    pub(crate) fn delete_backward_char(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.delete_range_internal(previous, self.cursor, true)
            .is_some()
    }

    pub(crate) fn delete_forward_char(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.delete_range_internal(self.cursor, next, true)
            .is_some()
    }

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

    pub(crate) fn kill_word_forward(&mut self) -> bool {
        let boundary = word_forward_boundary(&self.text, self.cursor);
        let Some(killed) = self.delete_range_internal(self.cursor, boundary, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    pub(crate) fn kill_word_backward(&mut self) -> bool {
        let boundary = word_backward_boundary(&self.text, self.cursor);
        let Some(killed) = self.delete_range_internal(boundary, self.cursor, true) else {
            return false;
        };
        self.push_kill(killed)
    }

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
        self.yank_state = Some(HudMessageBoxYankState {
            start,
            end: start + inserted,
            ring_index: 0,
        });
        true
    }

    pub(crate) fn yank_pop(&mut self) -> bool {
        let Some(yank_state) = self.yank_state.clone() else {
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
        self.yank_state = Some(HudMessageBoxYankState {
            start: yank_state.start,
            end: yank_state.start + inserted,
            ring_index: next_ring_index,
        });
        true
    }

    pub(crate) fn cursor_line_and_column(&self) -> (usize, usize) {
        (
            self.text[..self.cursor]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
            current_line_column_chars(&self.text, self.cursor),
        )
    }

    fn insert_text_internal(&mut self, at: usize, text: &str, clear_yank_state: bool) -> usize {
        let normalized = normalize_message_box_text(text);
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

    fn delete_range_internal(
        &mut self,
        start: usize,
        end: usize,
        clear_yank_state: bool,
    ) -> Option<String> {
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

    fn push_kill(&mut self, text: String) -> bool {
        if text.is_empty() {
            return false;
        }
        self.kill_ring.insert(0, text);
        if self.kill_ring.len() > HUD_MESSAGE_BOX_KILL_RING_LIMIT {
            self.kill_ring.truncate(HUD_MESSAGE_BOX_KILL_RING_LIMIT);
        }
        self.yank_state = None;
        true
    }
}

fn normalize_message_box_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn adjust_index_after_delete(index: usize, start: usize, end: usize) -> usize {
    if index <= start {
        index
    } else if index <= end {
        start
    } else {
        index - (end - start)
    }
}

fn word_backward_boundary(text: &str, cursor: usize) -> usize {
    let mut current = cursor;
    while let Some((previous, ch)) = previous_char(text, current) {
        if is_word_char(ch) {
            break;
        }
        current = previous;
    }
    while let Some((previous, ch)) = previous_char(text, current) {
        if !is_word_char(ch) {
            break;
        }
        current = previous;
    }
    current
}

fn word_forward_boundary(text: &str, cursor: usize) -> usize {
    let mut current = cursor;
    while let Some((next, ch)) = next_char(text, current) {
        if is_word_char(ch) {
            break;
        }
        current = next;
    }
    while let Some((next, ch)) = next_char(text, current) {
        if !is_word_char(ch) {
            break;
        }
        current = next;
    }
    current
}

fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last().map(|(index, _)| index)
}

fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

fn previous_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last()
}

fn next_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| (cursor + ch.len_utf8(), ch))
}

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

fn current_line_column_chars(text: &str, cursor: usize) -> usize {
    let (line_start, _) = current_line_bounds(text, cursor);
    text[line_start..cursor].chars().count()
}

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

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
