use crate::{hud::HudRect, terminals::TerminalId};
use bevy::{prelude::Vec2, window::Window};
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
};

const HUD_MESSAGE_BOX_KILL_RING_LIMIT: usize = 32;
const HUD_MESSAGE_BOX_ACTION_BUTTON_W: f32 = 170.0;
const HUD_MESSAGE_BOX_ACTION_BUTTON_H: f32 = 28.0;
const HUD_MESSAGE_BOX_ACTION_BUTTON_GAP: f32 = 12.0;
const HUD_MESSAGE_BOX_TOP_GAP: f32 = 8.0;
const HUD_MESSAGE_BOX_HEIGHT_RATIO: f32 = 0.52;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HudMessageBoxAction {
    AppendTask,
    PrependTask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HudTaskDialogAction {
    ClearDone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudMessageBoxActionButton {
    pub(crate) action: HudMessageBoxAction,
    pub(crate) rect: HudRect,
    pub(crate) label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudTaskDialogActionButton {
    pub(crate) action: HudTaskDialogAction,
    pub(crate) rect: HudRect,
    pub(crate) label: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HudMessageBoxYankState {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) ring_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct HudMessageBoxDraft {
    text: String,
    cursor: usize,
    mark: Option<usize>,
    preferred_column: Option<usize>,
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
    drafts: BTreeMap<TerminalId, HudMessageBoxDraft>,
}

// Implements message box rect.
pub(crate) fn message_box_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * 0.84).clamp(520.0, 1680.0),
        (window.height() * HUD_MESSAGE_BOX_HEIGHT_RATIO).clamp(240.0, 760.0),
    );
    HudRect {
        x: window.width() * 0.5 - size.x * 0.5,
        y: HUD_MESSAGE_BOX_TOP_GAP,
        w: size.x,
        h: size.y,
    }
}

// Implements message box action buttons.
pub(crate) fn message_box_action_buttons(window: &Window) -> [HudMessageBoxActionButton; 2] {
    let rect = message_box_rect(window);
    let base_y = rect.y + rect.h - 36.0;
    let prepend_x = rect.x + rect.w - 24.0 - HUD_MESSAGE_BOX_ACTION_BUTTON_W;
    let append_x = prepend_x - HUD_MESSAGE_BOX_ACTION_BUTTON_GAP - HUD_MESSAGE_BOX_ACTION_BUTTON_W;
    [
        HudMessageBoxActionButton {
            action: HudMessageBoxAction::AppendTask,
            rect: HudRect {
                x: append_x,
                y: base_y,
                w: HUD_MESSAGE_BOX_ACTION_BUTTON_W,
                h: HUD_MESSAGE_BOX_ACTION_BUTTON_H,
            },
            label: "Append Task",
        },
        HudMessageBoxActionButton {
            action: HudMessageBoxAction::PrependTask,
            rect: HudRect {
                x: prepend_x,
                y: base_y,
                w: HUD_MESSAGE_BOX_ACTION_BUTTON_W,
                h: HUD_MESSAGE_BOX_ACTION_BUTTON_H,
            },
            label: "Prepend Task",
        },
    ]
}

// Implements message box action at.
pub(crate) fn message_box_action_at(window: &Window, point: Vec2) -> Option<HudMessageBoxAction> {
    message_box_action_buttons(window)
        .into_iter()
        .find(|button| button.rect.contains(point))
        .map(|button| button.action)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HudTaskDialogState {
    inner: HudMessageBoxState,
}

impl Deref for HudTaskDialogState {
    type Target = HudMessageBoxState;

    // Implements deref.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for HudTaskDialogState {
    // Implements deref mut.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl HudTaskDialogState {
    // Opens with text.
    pub(crate) fn open_with_text(&mut self, target_terminal: TerminalId, text: &str) {
        self.inner.visible = true;
        self.inner.target_terminal = Some(target_terminal);
        self.inner.load_text(text);
    }

    // Closes this value.
    pub(crate) fn close(&mut self) {
        self.inner.visible = false;
        self.inner.target_terminal = None;
        self.inner.clear_editor();
    }
}

// Implements task dialog rect.
pub(crate) fn task_dialog_rect(window: &Window) -> HudRect {
    message_box_rect(window)
}

// Implements task dialog action buttons.
pub(crate) fn task_dialog_action_buttons(window: &Window) -> [HudTaskDialogActionButton; 1] {
    let rect = task_dialog_rect(window);
    let base_y = rect.y + rect.h - 36.0;
    [HudTaskDialogActionButton {
        action: HudTaskDialogAction::ClearDone,
        rect: HudRect {
            x: rect.x + 24.0,
            y: base_y,
            w: HUD_MESSAGE_BOX_ACTION_BUTTON_W,
            h: HUD_MESSAGE_BOX_ACTION_BUTTON_H,
        },
        label: "Clear done [x]",
    }]
}

// Implements task dialog action at.
pub(crate) fn task_dialog_action_at(window: &Window, point: Vec2) -> Option<HudTaskDialogAction> {
    task_dialog_action_buttons(window)
        .into_iter()
        .find(|button| button.rect.contains(point))
        .map(|button| button.action)
}

impl HudMessageBoxState {
    // Clears editor.
    fn clear_editor(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.mark = None;
        self.preferred_column = None;
        self.yank_state = None;
    }

    // Saves current draft.
    fn save_current_draft(&mut self) {
        let Some(target_terminal) = self.target_terminal else {
            return;
        };
        self.drafts.insert(
            target_terminal,
            HudMessageBoxDraft {
                text: self.text.clone(),
                cursor: self.cursor,
                mark: self.mark,
                preferred_column: self.preferred_column,
            },
        );
    }

    // Restores draft.
    fn restore_draft(&mut self, target_terminal: TerminalId) -> bool {
        if let Some(draft) = self.drafts.get(&target_terminal).cloned() {
            self.text = draft.text;
            self.cursor = draft.cursor.min(self.text.len());
            self.mark = draft.mark.map(|mark| mark.min(self.text.len()));
            self.preferred_column = draft.preferred_column;
            self.yank_state = None;
            return true;
        }

        false
    }

    // Loads text.
    pub(crate) fn load_text(&mut self, text: &str) {
        self.clear_editor();
        self.text = normalize_message_box_text(text);
        self.cursor = self.text.len();
    }

    // Implements reset for target with text.
    pub(crate) fn reset_for_target_with_text(&mut self, target_terminal: TerminalId, text: &str) {
        self.save_current_draft();
        self.visible = true;
        self.target_terminal = Some(target_terminal);
        if !self.restore_draft(target_terminal) {
            self.load_text(text);
        }
    }

    // Implements reset for target.
    pub(crate) fn reset_for_target(&mut self, target_terminal: TerminalId) {
        self.reset_for_target_with_text(target_terminal, "");
    }

    // Closes this value.
    pub(crate) fn close(&mut self) {
        self.save_current_draft();
        self.visible = false;
        self.target_terminal = None;
        self.mark = None;
        self.preferred_column = None;
        self.yank_state = None;
    }

    // Closes and discard current.
    pub(crate) fn close_and_discard_current(&mut self) {
        self.clear_current_draft();
        self.visible = false;
        self.target_terminal = None;
        self.clear_editor();
    }

    // Clears current draft.
    pub(crate) fn clear_current_draft(&mut self) {
        if let Some(target_terminal) = self.target_terminal {
            self.drafts.remove(&target_terminal);
        }
    }

    // Implements region bounds.
    pub(crate) fn region_bounds(&self) -> Option<(usize, usize)> {
        let mark = self.mark?;
        (mark != self.cursor).then_some((mark.min(self.cursor), mark.max(self.cursor)))
    }

    // Sets mark.
    pub(crate) fn set_mark(&mut self) -> bool {
        let changed = self.mark != Some(self.cursor);
        self.mark = Some(self.cursor);
        self.yank_state = None;
        changed
    }

    // Inserts text.
    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        let inserted = self.insert_text_internal(self.cursor, text, true);
        if inserted == 0 {
            return false;
        }
        self.cursor += inserted;
        self.preferred_column = None;
        true
    }

    // Inserts newline.
    pub(crate) fn insert_newline(&mut self) -> bool {
        self.insert_text("\n")
    }

    // Implements newline and indent.
    pub(crate) fn newline_and_indent(&mut self) -> bool {
        self.insert_newline()
    }

    // Opens line.
    pub(crate) fn open_line(&mut self) -> bool {
        let inserted = self.insert_text_internal(self.cursor, "\n", true);
        if inserted == 0 {
            return false;
        }
        self.preferred_column = None;
        true
    }

    // Implements move left.
    pub(crate) fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    // Implements move right.
    pub(crate) fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = next;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    // Implements move line start.
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

    // Implements move line end.
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

    // Implements move up.
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

    // Implements move down.
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

    // Implements move word backward.
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

    // Implements move word forward.
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

    // Implements delete backward char.
    pub(crate) fn delete_backward_char(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.delete_range_internal(previous, self.cursor, true)
            .is_some()
    }

    // Implements delete forward char.
    pub(crate) fn delete_forward_char(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.delete_range_internal(self.cursor, next, true)
            .is_some()
    }

    // Implements copy region.
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

    // Kills region.
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

    // Kills to end of line.
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

    // Kills word forward.
    pub(crate) fn kill_word_forward(&mut self) -> bool {
        let boundary = word_forward_boundary(&self.text, self.cursor);
        let Some(killed) = self.delete_range_internal(self.cursor, boundary, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    // Kills word backward.
    pub(crate) fn kill_word_backward(&mut self) -> bool {
        let boundary = word_backward_boundary(&self.text, self.cursor);
        let Some(killed) = self.delete_range_internal(boundary, self.cursor, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    // Implements yank.
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

    // Implements yank pop.
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

    // Implements cursor line and column.
    pub(crate) fn cursor_line_and_column(&self) -> (usize, usize) {
        (
            self.text[..self.cursor]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
            current_line_column_chars(&self.text, self.cursor),
        )
    }

    // Inserts text internal.
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

    // Implements delete range internal.
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

    // Pushes kill.
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

// Normalizes message box text.
fn normalize_message_box_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

// Adjusts index after delete.
fn adjust_index_after_delete(index: usize, start: usize, end: usize) -> usize {
    if index <= start {
        index
    } else if index <= end {
        start
    } else {
        index - (end - start)
    }
}

// Implements word backward boundary.
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

// Implements word forward boundary.
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

// Implements previous char boundary.
fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last().map(|(index, _)| index)
}

// Implements next char boundary.
fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

// Implements previous char.
fn previous_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last()
}

// Implements next char.
fn next_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| (cursor + ch.len_utf8(), ch))
}

// Implements current line bounds.
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

// Implements current line column chars.
fn current_line_column_chars(text: &str, cursor: usize) -> usize {
    let (line_start, _) = current_line_bounds(text, cursor);
    text[line_start..cursor].chars().count()
}

// Advances by chars.
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

// Returns whether word char.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
