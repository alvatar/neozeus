use crate::hud::HudRect;
use bevy::{prelude::Vec2, window::Window};

const ACTION_BUTTON_W: f32 = 170.0;
const ACTION_BUTTON_H: f32 = 28.0;
const ACTION_BUTTON_GAP: f32 = 12.0;
const TOP_GAP: f32 = 8.0;
const MESSAGE_BOX_HEIGHT_RATIO: f32 = 0.52;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageBoxAction {
    AppendTask,
    PrependTask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskDialogAction {
    ClearDone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MessageBoxActionButton {
    pub(crate) action: MessageBoxAction,
    pub(crate) rect: HudRect,
    pub(crate) label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TaskDialogActionButton {
    pub(crate) action: TaskDialogAction,
    pub(crate) rect: HudRect,
    pub(crate) label: &'static str,
}

/// Computes the outer rectangle for the message-box modal.
///
/// The box scales with the window but is clamped to sane min/max dimensions so the editor remains
/// usable on both small and large displays.
pub(crate) fn message_box_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * 0.84).clamp(520.0, 1680.0),
        (window.height() * MESSAGE_BOX_HEIGHT_RATIO).clamp(240.0, 760.0),
    );
    HudRect {
        x: window.width() * 0.5 - size.x * 0.5,
        y: TOP_GAP,
        w: size.x,
        h: size.y,
    }
}

/// Lays out the two task action buttons shown at the bottom of the message box.
pub(crate) fn message_box_action_buttons(window: &Window) -> [MessageBoxActionButton; 2] {
    let rect = message_box_rect(window);
    let base_y = rect.y + rect.h - 36.0;
    let prepend_x = rect.x + rect.w - 24.0 - ACTION_BUTTON_W;
    let append_x = prepend_x - ACTION_BUTTON_GAP - ACTION_BUTTON_W;
    [
        MessageBoxActionButton {
            action: MessageBoxAction::AppendTask,
            rect: HudRect {
                x: append_x,
                y: base_y,
                w: ACTION_BUTTON_W,
                h: ACTION_BUTTON_H,
            },
            label: "Append Task",
        },
        MessageBoxActionButton {
            action: MessageBoxAction::PrependTask,
            rect: HudRect {
                x: prepend_x,
                y: base_y,
                w: ACTION_BUTTON_W,
                h: ACTION_BUTTON_H,
            },
            label: "Prepend Task",
        },
    ]
}

/// Hit-tests the message-box action buttons and returns the clicked action.
pub(crate) fn message_box_action_at(window: &Window, point: Vec2) -> Option<MessageBoxAction> {
    message_box_action_buttons(window)
        .into_iter()
        .find(|button| button.rect.contains(point))
        .map(|button| button.action)
}

/// Returns the outer rectangle for the task dialog.
///
/// Task dialogs intentionally share the same modal footprint as the message box so both editors align
/// visually and can reuse the same rendering layout.
pub(crate) fn task_dialog_rect(window: &Window) -> HudRect {
    message_box_rect(window)
}

/// Lays out the task dialog's action buttons.
pub(crate) fn task_dialog_action_buttons(window: &Window) -> [TaskDialogActionButton; 1] {
    let rect = task_dialog_rect(window);
    let base_y = rect.y + rect.h - 36.0;
    [TaskDialogActionButton {
        action: TaskDialogAction::ClearDone,
        rect: HudRect {
            x: rect.x + 24.0,
            y: base_y,
            w: ACTION_BUTTON_W,
            h: ACTION_BUTTON_H,
        },
        label: "Clear done [x]",
    }]
}

/// Hit-tests the task dialog's action buttons and returns the clicked action.
pub(crate) fn task_dialog_action_at(window: &Window, point: Vec2) -> Option<TaskDialogAction> {
    task_dialog_action_buttons(window)
        .into_iter()
        .find(|button| button.rect.contains(point))
        .map(|button| button.action)
}
