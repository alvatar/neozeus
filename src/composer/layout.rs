use crate::{app::CreateAgentKind, hud::HudRect};
use bevy::{prelude::Vec2, window::Window};

const ACTION_BUTTON_W: f32 = 170.0;
const ACTION_BUTTON_H: f32 = 28.0;
const ACTION_BUTTON_GAP: f32 = 12.0;
const TOP_GAP: f32 = 8.0;
const MESSAGE_BOX_HEIGHT_RATIO: f32 = 0.52;
const CREATE_AGENT_DIALOG_WIDTH_RATIO: f32 = 0.48;
const CREATE_AGENT_DIALOG_HEIGHT_RATIO: f32 = 0.36;
const CREATE_AGENT_DIALOG_FIELD_HEIGHT: f32 = 32.0;
const CREATE_AGENT_DIALOG_INSET_X: f32 = 24.0;
const CREATE_AGENT_DIALOG_LABEL_W: f32 = 136.0;
const CREATE_AGENT_DIALOG_ROW_GAP: f32 = 18.0;
const CREATE_AGENT_DIALOG_RADIO_SIZE: f32 = 22.0;
const CREATE_AGENT_DIALOG_RADIO_GAP: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageBoxAction {
    AppendTask,
    PrependTask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskDialogAction {
    ClearDone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreateAgentDialogTarget {
    NameField,
    Kind(CreateAgentKind),
    StartingFolderField,
    CreateButton,
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
pub(crate) fn message_box_action_buttons(
    window: &Window,
) -> [(MessageBoxAction, HudRect, &'static str); 2] {
    let rect = message_box_rect(window);
    let base_y = rect.y + rect.h - 36.0;
    let prepend_x = rect.x + rect.w - 24.0 - ACTION_BUTTON_W;
    let append_x = prepend_x - ACTION_BUTTON_GAP - ACTION_BUTTON_W;
    [
        (
            MessageBoxAction::AppendTask,
            HudRect {
                x: append_x,
                y: base_y,
                w: ACTION_BUTTON_W,
                h: ACTION_BUTTON_H,
            },
            "Append Task",
        ),
        (
            MessageBoxAction::PrependTask,
            HudRect {
                x: prepend_x,
                y: base_y,
                w: ACTION_BUTTON_W,
                h: ACTION_BUTTON_H,
            },
            "Prepend Task",
        ),
    ]
}

/// Hit-tests the message-box action buttons and returns the clicked action.
pub(crate) fn message_box_action_at(window: &Window, point: Vec2) -> Option<MessageBoxAction> {
    message_box_action_buttons(window)
        .into_iter()
        .find(|(_, rect, _)| rect.contains(point))
        .map(|(action, _, _)| action)
}

/// Returns the outer rectangle for the task dialog.
///
/// Task dialogs intentionally share the same modal footprint as the message box so both editors align
/// visually and can reuse the same rendering layout.
pub(crate) fn task_dialog_rect(window: &Window) -> HudRect {
    message_box_rect(window)
}

/// Lays out the task dialog's action buttons.
pub(crate) fn task_dialog_action_buttons(
    window: &Window,
) -> [(TaskDialogAction, HudRect, &'static str); 1] {
    let rect = task_dialog_rect(window);
    let base_y = rect.y + rect.h - 36.0;
    [(
        TaskDialogAction::ClearDone,
        HudRect {
            x: rect.x + 24.0,
            y: base_y,
            w: ACTION_BUTTON_W,
            h: ACTION_BUTTON_H,
        },
        "Clear done [x]",
    )]
}

/// Hit-tests the task dialog's action buttons and returns the clicked action.
pub(crate) fn task_dialog_action_at(window: &Window, point: Vec2) -> Option<TaskDialogAction> {
    task_dialog_action_buttons(window)
        .into_iter()
        .find(|(_, rect, _)| rect.contains(point))
        .map(|(action, _, _)| action)
}

/// Computes the centered modal rectangle used by the create-agent dialog.
pub(crate) fn create_agent_dialog_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * CREATE_AGENT_DIALOG_WIDTH_RATIO).clamp(560.0, 960.0),
        (window.height() * CREATE_AGENT_DIALOG_HEIGHT_RATIO).clamp(280.0, 420.0),
    );
    HudRect {
        x: window.width() * 0.5 - size.x * 0.5,
        y: window.height() * 0.5 - size.y * 0.5,
        w: size.x,
        h: size.y,
    }
}

fn create_agent_row_y(rect: HudRect, row_index: usize) -> f32 {
    rect.y
        + 68.0
        + row_index as f32 * (CREATE_AGENT_DIALOG_FIELD_HEIGHT + CREATE_AGENT_DIALOG_ROW_GAP)
}

/// Returns the editable name field rectangle within the create-agent dialog.
pub(crate) fn create_agent_name_field_rect(window: &Window) -> HudRect {
    let rect = create_agent_dialog_rect(window);
    HudRect {
        x: rect.x + CREATE_AGENT_DIALOG_INSET_X + CREATE_AGENT_DIALOG_LABEL_W,
        y: create_agent_row_y(rect, 0),
        w: rect.w - (CREATE_AGENT_DIALOG_INSET_X * 2.0 + CREATE_AGENT_DIALOG_LABEL_W),
        h: CREATE_AGENT_DIALOG_FIELD_HEIGHT,
    }
}

/// Returns the radio-style type option rects within the create-agent dialog.
pub(crate) fn create_agent_kind_option_rects(
    window: &Window,
) -> [(CreateAgentKind, HudRect, &'static str); 2] {
    let rect = create_agent_dialog_rect(window);
    let row_y = create_agent_row_y(rect, 1) + 5.0;
    let left_x = rect.x + CREATE_AGENT_DIALOG_INSET_X + CREATE_AGENT_DIALOG_LABEL_W;
    let first_w = 90.0;
    let second_x = left_x + first_w + CREATE_AGENT_DIALOG_RADIO_GAP;
    [
        (
            CreateAgentKind::Agent,
            HudRect {
                x: left_x,
                y: row_y,
                w: first_w,
                h: CREATE_AGENT_DIALOG_RADIO_SIZE,
            },
            "Agent",
        ),
        (
            CreateAgentKind::Shell,
            HudRect {
                x: second_x,
                y: row_y,
                w: 90.0,
                h: CREATE_AGENT_DIALOG_RADIO_SIZE,
            },
            "Shell",
        ),
    ]
}

/// Returns the editable starting-folder field rectangle within the create-agent dialog.
pub(crate) fn create_agent_starting_folder_rect(window: &Window) -> HudRect {
    let rect = create_agent_dialog_rect(window);
    HudRect {
        x: rect.x + CREATE_AGENT_DIALOG_INSET_X + CREATE_AGENT_DIALOG_LABEL_W,
        y: create_agent_row_y(rect, 2),
        w: rect.w - (CREATE_AGENT_DIALOG_INSET_X * 2.0 + CREATE_AGENT_DIALOG_LABEL_W),
        h: CREATE_AGENT_DIALOG_FIELD_HEIGHT,
    }
}

/// Returns the create button rectangle for the create-agent dialog.
pub(crate) fn create_agent_create_button_rect(window: &Window) -> HudRect {
    let rect = create_agent_dialog_rect(window);
    HudRect {
        x: rect.x + rect.w - 24.0 - ACTION_BUTTON_W,
        y: rect.y + rect.h - 40.0,
        w: ACTION_BUTTON_W,
        h: ACTION_BUTTON_H,
    }
}

/// Hit-tests the create-agent dialog controls and returns the target under the pointer.
pub(crate) fn create_agent_dialog_target_at(
    window: &Window,
    point: Vec2,
) -> Option<CreateAgentDialogTarget> {
    let name_rect = create_agent_name_field_rect(window);
    if name_rect.contains(point) {
        return Some(CreateAgentDialogTarget::NameField);
    }
    for (kind, rect, _) in create_agent_kind_option_rects(window) {
        if rect.contains(point) {
            return Some(CreateAgentDialogTarget::Kind(kind));
        }
    }
    let folder_rect = create_agent_starting_folder_rect(window);
    if folder_rect.contains(point) {
        return Some(CreateAgentDialogTarget::StartingFolderField);
    }
    let create_rect = create_agent_create_button_rect(window);
    if create_rect.contains(point) {
        return Some(CreateAgentDialogTarget::CreateButton);
    }
    None
}
