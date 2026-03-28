mod layout;

use crate::agents::AgentId;
use std::collections::BTreeMap;

pub(crate) use layout::{
    message_box_action_at, message_box_action_buttons, message_box_rect, task_dialog_action_at,
    task_dialog_action_buttons, task_dialog_rect, MessageBoxAction, TaskDialogAction,
};

const TEXT_EDITOR_KILL_RING_LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextEditorYankState {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) ring_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct TextEditorDraft {
    text: String,
    cursor: usize,
    mark: Option<usize>,
    preferred_column: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TextEditorState {
    pub(crate) visible: bool,
    #[cfg(test)]
    pub(crate) target_terminal: Option<crate::terminals::TerminalId>,
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) mark: Option<usize>,
    pub(crate) preferred_column: Option<usize>,
    pub(crate) kill_ring: Vec<String>,
    pub(crate) yank_state: Option<TextEditorYankState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ComposerMode {
    Message { agent_id: AgentId },
    TaskEdit { agent_id: AgentId },
}

impl ComposerMode {
    /// Returns the agent id associated with this composer mode.
    pub(crate) fn agent_id(&self) -> AgentId {
        match self {
            Self::Message { agent_id } | Self::TaskEdit { agent_id } => *agent_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerSession {
    pub(crate) mode: ComposerMode,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ComposerState {
    pub(crate) session: Option<ComposerSession>,
    pub(crate) message_editor: TextEditorState,
    pub(crate) task_editor: TextEditorState,
    message_drafts: BTreeMap<AgentId, TextEditorDraft>,
}

impl ComposerState {
    /// Returns whether the composer or direct-input mode currently owns keyboard capture.
    pub(crate) fn keyboard_capture_active(
        &self,
        input_capture: &crate::hud::HudInputCaptureState,
    ) -> bool {
        self.message_editor.visible
            || self.task_editor.visible
            || input_capture.direct_input_terminal.is_some()
    }

    /// Handles unbind agent.
    pub(crate) fn unbind_agent(&mut self, agent_id: AgentId) {
        self.message_drafts.remove(&agent_id);
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.mode.agent_id() == agent_id)
        {
            self.cancel_preserving_draft();
        }
    }

    /// Opens message.
    pub(crate) fn open_message(&mut self, agent_id: AgentId) {
        self.save_open_message_draft();
        self.task_editor.close();
        self.message_editor.visible = true;
        if let Some(draft) = self.message_drafts.get(&agent_id).cloned() {
            self.message_editor.restore_draft(draft);
        } else {
            self.message_editor.load_text("");
        }
        self.session = Some(ComposerSession {
            mode: ComposerMode::Message { agent_id },
        });
    }

    /// Opens task editor.
    pub(crate) fn open_task_editor(&mut self, agent_id: AgentId, text: &str) {
        self.save_open_message_draft();
        self.message_editor.close();
        self.task_editor.visible = true;
        self.task_editor.load_text(text);
        self.session = Some(ComposerSession {
            mode: ComposerMode::TaskEdit { agent_id },
        });
    }

    /// Handles cancel preserving draft.
    pub(crate) fn cancel_preserving_draft(&mut self) {
        if let Some(session) = self.session.take() {
            match session.mode {
                ComposerMode::Message { agent_id } => {
                    self.save_message_draft(agent_id);
                    self.message_editor.close();
                }
                ComposerMode::TaskEdit { .. } => self.task_editor.close(),
            }
        }
    }

    /// Handles discard current message.
    pub(crate) fn discard_current_message(&mut self) {
        if let Some(agent_id) = self.current_message_agent() {
            self.message_drafts.remove(&agent_id);
        }
        self.message_editor.close_and_discard();
        if matches!(
            self.session.as_ref().map(|session| &session.mode),
            Some(ComposerMode::Message { .. })
        ) {
            self.session = None;
        }
    }

    /// Closes task editor.
    pub(crate) fn close_task_editor(&mut self) {
        self.task_editor.close();
        if matches!(
            self.session.as_ref().map(|session| &session.mode),
            Some(ComposerMode::TaskEdit { .. })
        ) {
            self.session = None;
        }
    }

    /// Returns the agent currently bound to the active composer session.
    pub(crate) fn current_agent(&self) -> Option<AgentId> {
        self.session.as_ref().map(|session| session.mode.agent_id())
    }

    /// Returns the agent currently bound to the active message composer session.
    fn current_message_agent(&self) -> Option<AgentId> {
        match self.session.as_ref().map(|session| &session.mode) {
            Some(ComposerMode::Message { agent_id }) => Some(*agent_id),
            _ => None,
        }
    }

    /// Saves open message draft.
    fn save_open_message_draft(&mut self) {
        if let Some(agent_id) = self.current_message_agent() {
            self.save_message_draft(agent_id);
        }
    }

    /// Saves message draft.
    fn save_message_draft(&mut self, agent_id: AgentId) {
        self.message_drafts
            .insert(agent_id, self.message_editor.snapshot_draft());
    }
}

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
    fn snapshot_draft(&self) -> TextEditorDraft {
        TextEditorDraft {
            text: self.text.clone(),
            cursor: self.cursor,
            mark: self.mark,
            preferred_column: self.preferred_column,
        }
    }

    /// Restores draft.
    fn restore_draft(&mut self, draft: TextEditorDraft) {
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
        let boundary = word_backward_boundary(&self.text, self.cursor);
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
        let boundary = word_forward_boundary(&self.text, self.cursor);
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
        let boundary = word_forward_boundary(&self.text, self.cursor);
        let Some(killed) = self.delete_range_internal(self.cursor, boundary, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    /// Deletes word backward and updates the kill-ring state.
    pub(crate) fn kill_word_backward(&mut self) -> bool {
        let boundary = word_backward_boundary(&self.text, self.cursor);
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

/// Handles word backward boundary.
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

/// Handles word forward boundary.
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

/// Handles previous char boundary.
fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last().map(|(index, _)| index)
}

/// Handles next char boundary.
fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

/// Handles previous char.
fn previous_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last()
}

/// Handles next char.
fn next_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| (cursor + ch.len_utf8(), ch))
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

#[cfg(test)]
mod tests;
