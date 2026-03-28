#![allow(dead_code)]

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

/// Computes the outer rectangle for the message-box modal.
///
/// The box scales with the window but is clamped to sane min/max dimensions so the editor remains
/// usable on both small and large displays.
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

/// Lays out the two task action buttons shown at the bottom of the message box.
///
/// The buttons are anchored from the modal's right edge so the append/prepend pair stays aligned even
/// as the modal width changes.
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

/// Hit-tests the message-box action buttons and returns the clicked action.
///
/// The helper intentionally recomputes the current button layout from the window size instead of
/// storing stale rectangles anywhere.
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

    /// Exposes the task dialog's embedded editor state through `Deref`.
    ///
    /// The task dialog intentionally reuses the message-box editor machinery instead of maintaining a
    /// separate editor implementation.
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for HudTaskDialogState {
    /// Exposes mutable access to the embedded message-box editor state.
    ///
    /// This keeps the task dialog API thin while still allowing all editor helpers to operate on it.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl HudTaskDialogState {
    /// Opens the task dialog for one terminal and seeds the editor with the supplied text.
    ///
    /// Unlike the main message box, the task dialog does not restore drafts; it always reflects the
    /// current task text being edited.
    pub(crate) fn open_with_text(&mut self, target_terminal: TerminalId, text: &str) {
        self.inner.visible = true;
        self.inner.target_terminal = Some(target_terminal);
        self.inner.load_text(text);
    }

    /// Closes the task dialog and clears its transient editor state.
    ///
    /// Task dialogs are not draft-preserving, so closing fully discards the current text.
    pub(crate) fn close(&mut self) {
        self.inner.visible = false;
        self.inner.target_terminal = None;
        self.inner.clear_editor();
    }
}

/// Returns the outer rectangle for the task dialog.
///
/// Task dialogs intentionally share the same modal footprint as the message box so both editors align
/// visually and can reuse the same rendering layout.
pub(crate) fn task_dialog_rect(window: &Window) -> HudRect {
    message_box_rect(window)
}

/// Lays out the task dialog's action buttons.
///
/// There is currently only one destructive action, so it is anchored on the left rather than using
/// the message box's right-aligned dual-button layout.
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

/// Hit-tests the task dialog's action buttons and returns the clicked action.
///
/// As with the message box, the function derives button rectangles on demand from the current window
/// size so it cannot drift from the rendered layout.
pub(crate) fn task_dialog_action_at(window: &Window, point: Vec2) -> Option<HudTaskDialogAction> {
    task_dialog_action_buttons(window)
        .into_iter()
        .find(|button| button.rect.contains(point))
        .map(|button| button.action)
}

impl HudMessageBoxState {
    /// Resets the in-memory editor cursor/selection state and clears all text.
    ///
    /// The kill ring is intentionally preserved across closes so repeated editor operations can still
    /// yank previous kills.
    fn clear_editor(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.mark = None;
        self.preferred_column = None;
        self.yank_state = None;
    }

    /// Stores the current editor contents as the draft for the currently targeted terminal.
    ///
    /// Drafts are keyed by terminal id so switching between terminals preserves per-terminal in-flight
    /// edits.
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

    /// Restores the saved draft for one terminal, if any.
    ///
    /// Cursor and mark positions are clamped back into the restored text in case the stored draft was
    /// produced before later normalization or trimming.
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

    /// Replaces the editor contents with freshly supplied text.
    ///
    /// The text is normalized to LF newlines and the cursor is placed at the end, matching the usual
    /// modal-open behavior.
    pub(crate) fn load_text(&mut self, text: &str) {
        self.clear_editor();
        self.text = normalize_message_box_text(text);
        self.cursor = self.text.len();
    }

    /// Opens the message box for a terminal, restoring its draft if one exists or seeding new text
    /// otherwise.
    ///
    /// Before switching targets, the current terminal's draft is saved so cross-terminal editing does
    /// not lose work.
    pub(crate) fn reset_for_target_with_text(&mut self, target_terminal: TerminalId, text: &str) {
        self.save_current_draft();
        self.visible = true;
        self.target_terminal = Some(target_terminal);
        if !self.restore_draft(target_terminal) {
            self.load_text(text);
        }
    }

    /// Opens the message box for a terminal without providing any initial text.
    ///
    /// This is the common path for creating a new task draft while still restoring any saved draft for
    /// that terminal.
    pub(crate) fn reset_for_target(&mut self, target_terminal: TerminalId) {
        self.reset_for_target_with_text(target_terminal, "");
    }

    /// Closes the message box while preserving the current terminal draft.
    ///
    /// Selection and yank bookkeeping are cleared, but the text itself is cached per terminal for the
    /// next reopen.
    pub(crate) fn close(&mut self) {
        self.save_current_draft();
        self.visible = false;
        self.target_terminal = None;
        self.mark = None;
        self.preferred_column = None;
        self.yank_state = None;
    }

    /// Closes the message box and permanently discards the current terminal's draft.
    ///
    /// This is used when the draft has been consumed into a real command and should not come back on
    /// reopen.
    pub(crate) fn close_and_discard_current(&mut self) {
        self.clear_current_draft();
        self.visible = false;
        self.target_terminal = None;
        self.clear_editor();
    }

    /// Removes the saved draft for the currently targeted terminal, if there is one.
    ///
    /// This only touches persisted draft state; it does not modify the live editor buffer directly.
    pub(crate) fn clear_current_draft(&mut self) {
        if let Some(target_terminal) = self.target_terminal {
            self.drafts.remove(&target_terminal);
        }
    }

    /// Returns the normalized byte range of the active region, if mark and cursor differ.
    ///
    /// The editor stores mark/cursor independently, so this helper canonicalizes them into ascending
    /// bounds for copy/kill operations.
    pub(crate) fn region_bounds(&self) -> Option<(usize, usize)> {
        let mark = self.mark?;
        (mark != self.cursor).then_some((mark.min(self.cursor), mark.max(self.cursor)))
    }

    /// Sets the selection mark at the current cursor position.
    ///
    /// Re-setting the mark to the same location reports `false`; any yank-pop chain is cancelled
    /// because the editor state has conceptually changed.
    pub(crate) fn set_mark(&mut self) -> bool {
        let changed = self.mark != Some(self.cursor);
        self.mark = Some(self.cursor);
        self.yank_state = None;
        changed
    }

    /// Inserts normalized text at the cursor and advances the cursor past it.
    ///
    /// This is the ordinary self-insert path: it clears preferred-column tracking and treats the edit
    /// as a fresh command that breaks any yank-pop chain.
    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        let inserted = self.insert_text_internal(self.cursor, text, true);
        if inserted == 0 {
            return false;
        }
        self.cursor += inserted;
        self.preferred_column = None;
        true
    }

    /// Inserts a literal newline at the cursor.
    ///
    /// This delegates to [`Self::insert_text`] so newline normalization and yank-state handling stay
    /// consistent with ordinary insertion.
    pub(crate) fn insert_newline(&mut self) -> bool {
        self.insert_text("\n")
    }

    /// Inserts a newline using the editor's current indentation policy.
    ///
    /// There is no indentation logic yet, so this is intentionally equivalent to plain newline insert.
    pub(crate) fn newline_and_indent(&mut self) -> bool {
        self.insert_newline()
    }

    /// Inserts a newline at the cursor without moving the cursor onto the new line.
    ///
    /// This matches the classic Emacs-style `open-line` behavior: text after the cursor is pushed down
    /// while point stays before the inserted newline.
    pub(crate) fn open_line(&mut self) -> bool {
        let inserted = self.insert_text_internal(self.cursor, "\n", true);
        if inserted == 0 {
            return false;
        }
        self.preferred_column = None;
        true
    }

    /// Moves the cursor one Unicode scalar backward.
    ///
    /// Cursor math is byte-based internally, so movement goes through UTF-8 boundary helpers instead of
    /// subtracting raw bytes.
    pub(crate) fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    /// Moves the cursor one Unicode scalar forward.
    ///
    /// As with left movement, this respects UTF-8 character boundaries and clears transient column/yank
    /// tracking.
    pub(crate) fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = next;
        self.preferred_column = None;
        self.yank_state = None;
        true
    }

    /// Moves the cursor to the start of the current line.
    ///
    /// The line bounds are derived from newline search around the current cursor; no full-line split is
    /// allocated.
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

    /// Moves the cursor to the end of the current line, stopping before the newline byte.
    ///
    /// Returning `false` when already at the line end lets command handlers distinguish no-op moves.
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

    /// Moves the cursor to the previous line while trying to preserve visual column.
    ///
    /// The first vertical move captures the current column in characters; subsequent up/down moves reuse
    /// that preferred column until another editing or horizontal motion clears it.
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

    /// Moves the cursor to the next line while trying to preserve visual column.
    ///
    /// Like [`Self::move_up`], this uses character counts rather than byte offsets so UTF-8 text does
    /// not skew column tracking.
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

    /// Moves the cursor backward to the start of the previous word.
    ///
    /// Non-word punctuation and whitespace are skipped first, then the contiguous word run is crossed.
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

    /// Moves the cursor forward to the end of the next word.
    ///
    /// The boundary helper first skips non-word separators and then consumes the following word run.
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

    /// Deletes the Unicode scalar immediately before the cursor.
    ///
    /// This is pure deletion, not kill-ring insertion, so deleted text is not recoverable via yank.
    pub(crate) fn delete_backward_char(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.delete_range_internal(previous, self.cursor, true)
            .is_some()
    }

    /// Deletes the Unicode scalar immediately after the cursor.
    ///
    /// Like backward delete, this bypasses the kill ring and just mutates the buffer in place.
    pub(crate) fn delete_forward_char(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.delete_range_internal(self.cursor, next, true)
            .is_some()
    }

    /// Copies the active region into the kill ring without changing the buffer.
    ///
    /// The mark is cleared afterwards, matching the editor's convention that region operations consume
    /// the active selection.
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

    /// Deletes the active region and pushes the removed text onto the kill ring.
    ///
    /// This is the destructive counterpart to [`Self::copy_region`].
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

    /// Kills from the cursor to the logical end of line.
    ///
    /// If point is already at line end and there is another line, the trailing newline is killed so
    /// repeated invocations keep making progress.
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

    /// Kills the next word-sized span forward from the cursor and stores it in the kill ring.
    ///
    /// The exact span is defined by [`word_forward_boundary`], including any leading separator skip.
    pub(crate) fn kill_word_forward(&mut self) -> bool {
        let boundary = word_forward_boundary(&self.text, self.cursor);
        let Some(killed) = self.delete_range_internal(self.cursor, boundary, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    /// Kills the previous word-sized span backward from the cursor and stores it in the kill ring.
    ///
    /// The exact range is defined by [`word_backward_boundary`].
    pub(crate) fn kill_word_backward(&mut self) -> bool {
        let boundary = word_backward_boundary(&self.text, self.cursor);
        let Some(killed) = self.delete_range_internal(boundary, self.cursor, true) else {
            return false;
        };
        self.push_kill(killed)
    }

    /// Inserts the most recent kill-ring entry at the cursor and records its range for `yank-pop`.
    ///
    /// The inserted range is tracked so subsequent rotation can replace exactly the text produced by the
    /// most recent yank.
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

    /// Rotates the most recent yank through older kill-ring entries.
    ///
    /// The operation only works immediately after a yank because it relies on the tracked yank range to
    /// delete and replace the inserted text in place.
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

    /// Returns the cursor position as zero-based `(line, column)` in character coordinates.
    ///
    /// Line counting is byte-based over newline markers, while the column is measured in characters on
    /// the current line so UTF-8 text reports a human-meaningful column.
    pub(crate) fn cursor_line_and_column(&self) -> (usize, usize) {
        (
            self.text[..self.cursor]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
            current_line_column_chars(&self.text, self.cursor),
        )
    }

    /// Inserts normalized text at an arbitrary byte index and repairs editor bookkeeping around it.
    ///
    /// Mark and yank ranges that lie at or after the insertion point are shifted forward. Callers can
    /// choose whether the edit should invalidate `yank_pop` state.
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

    /// Deletes an arbitrary byte range and repairs cursor/mark/yank bookkeeping afterwards.
    ///
    /// Any index that fell inside the removed span is collapsed to the start of the deletion, and the
    /// yank range is dropped entirely if the deletion erases it.
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

    /// Pushes a non-empty string onto the front of the kill ring.
    ///
    /// The ring is bounded and truncates older entries once the fixed limit is exceeded.
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

/// Normalizes incoming editor text to use LF newlines only.
///
/// The editor stores byte indices into the string, so keeping newline representation uniform avoids a
/// lot of awkward platform-specific edge cases.
fn normalize_message_box_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Re-maps one byte index after deleting the half-open range `start..end`.
///
/// Indices before the deletion stay untouched, indices inside collapse to `start`, and later indices
/// shift backward by the deleted byte count.
fn adjust_index_after_delete(index: usize, start: usize, end: usize) -> usize {
    if index <= start {
        index
    } else if index <= end {
        start
    } else {
        index - (end - start)
    }
}

/// Finds the byte boundary reached by moving backward one word from `cursor`.
///
/// The search first skips separators/punctuation, then continues over a contiguous run of word
/// characters.
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

/// Finds the byte boundary reached by moving forward one word from `cursor`.
///
/// The search mirrors [`word_backward_boundary`]: skip separators first, then consume the following
/// word run.
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

/// Returns the byte index of the previous UTF-8 character boundary before `cursor`.
///
/// This is the primitive used by leftward cursor motion and backward deletion.
fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last().map(|(index, _)| index)
}

/// Returns the byte index immediately after the next UTF-8 character starting at `cursor`.
///
/// This is the primitive used by rightward cursor motion and forward deletion.
fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

/// Returns the previous character and its starting byte index before `cursor`.
///
/// The returned index points at the character itself, not at the cursor position after traversing it.
fn previous_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last()
}

/// Returns the next character after `cursor` together with the byte index immediately after it.
///
/// This slightly asymmetric shape matches how the word-scanning loops advance their cursor.
fn next_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| (cursor + ch.len_utf8(), ch))
}

/// Returns the byte bounds of the line containing `cursor`.
///
/// The end bound stops before the newline separator if one exists.
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

/// Counts the cursor's column within its current line in characters, not bytes.
///
/// Vertical motion uses this to preserve visual column across UTF-8 text.
fn current_line_column_chars(text: &str, cursor: usize) -> usize {
    let (line_start, _) = current_line_bounds(text, cursor);
    text[line_start..cursor].chars().count()
}

/// Advances from `start` toward `end` by at most `count` characters and returns the resulting byte
/// index.
///
/// If the span is shorter than `count` characters, the function returns `end`.
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

/// Returns whether a character should be considered part of a word for editor motions.
///
/// The current policy is deliberately simple: alphanumerics and underscore form words.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
