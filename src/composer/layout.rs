use crate::{app::CreateAgentKind, hud::HudRect};
use bevy::{prelude::Vec2, window::Window};

const ACTION_BUTTON_W: f32 = 170.0;
const ACTION_BUTTON_H: f32 = 28.0;
const ACTION_BUTTON_GAP: f32 = 12.0;
const TOP_GAP: f32 = 8.0;
const MESSAGE_BOX_HEIGHT_RATIO: f32 = 0.52;
const CREATE_AGENT_DIALOG_WIDTH_RATIO: f32 = 0.48;
const CREATE_AGENT_DIALOG_HEIGHT_RATIO: f32 = 0.36;
const RENAME_AGENT_DIALOG_HEIGHT_RATIO: f32 = 0.22;
const CREATE_AGENT_DIALOG_FIELD_HEIGHT: f32 = 32.0;
const CREATE_AGENT_DIALOG_INSET_X: f32 = 24.0;
const CREATE_AGENT_DIALOG_LABEL_W: f32 = 136.0;
const CREATE_AGENT_DIALOG_ROW_GAP: f32 = 18.0;
const CREATE_AGENT_DIALOG_RADIO_SIZE: f32 = 22.0;
const CREATE_AGENT_DIALOG_RADIO_GAP: f32 = 20.0;
const CLONE_AGENT_DIALOG_HEIGHT_RATIO: f32 = 0.24;
const AEGIS_DIALOG_HEIGHT_RATIO: f32 = 0.24;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloneAgentDialogTarget {
    NameField,
    WorkdirToggle,
    CloneButton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenameAgentDialogTarget {
    NameField,
    RenameButton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AegisDialogTarget {
    PromptField,
    EnableButton,
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

/// Computes the centered modal rectangle used by the clone-agent dialog.
pub(crate) fn clone_agent_dialog_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * CREATE_AGENT_DIALOG_WIDTH_RATIO).clamp(560.0, 960.0),
        (window.height() * CLONE_AGENT_DIALOG_HEIGHT_RATIO).clamp(200.0, 280.0),
    );
    HudRect {
        x: window.width() * 0.5 - size.x * 0.5,
        y: window.height() * 0.5 - size.y * 0.5,
        w: size.x,
        h: size.y,
    }
}

/// Computes the centered modal rectangle used by the rename-agent dialog.
pub(crate) fn rename_agent_dialog_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * CREATE_AGENT_DIALOG_WIDTH_RATIO).clamp(560.0, 960.0),
        (window.height() * RENAME_AGENT_DIALOG_HEIGHT_RATIO).clamp(180.0, 240.0),
    );
    HudRect {
        x: window.width() * 0.5 - size.x * 0.5,
        y: window.height() * 0.5 - size.y * 0.5,
        w: size.x,
        h: size.y,
    }
}

pub(crate) fn aegis_dialog_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * CREATE_AGENT_DIALOG_WIDTH_RATIO).clamp(560.0, 960.0),
        (window.height() * AEGIS_DIALOG_HEIGHT_RATIO).clamp(200.0, 280.0),
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

/// Returns the editable name field rectangle within the clone-agent dialog.
pub(crate) fn clone_agent_name_field_rect(window: &Window) -> HudRect {
    let rect = clone_agent_dialog_rect(window);
    HudRect {
        x: rect.x + CREATE_AGENT_DIALOG_INSET_X + CREATE_AGENT_DIALOG_LABEL_W,
        y: rect.y + 68.0,
        w: rect.w - (CREATE_AGENT_DIALOG_INSET_X * 2.0 + CREATE_AGENT_DIALOG_LABEL_W),
        h: CREATE_AGENT_DIALOG_FIELD_HEIGHT,
    }
}

/// Returns the checkbox hit rectangle within the clone-agent dialog.
pub(crate) fn clone_agent_workdir_rect(window: &Window) -> HudRect {
    let rect = clone_agent_dialog_rect(window);
    HudRect {
        x: rect.x + CREATE_AGENT_DIALOG_INSET_X + CREATE_AGENT_DIALOG_LABEL_W,
        y: rect.y + 68.0 + CREATE_AGENT_DIALOG_FIELD_HEIGHT + CREATE_AGENT_DIALOG_ROW_GAP + 3.0,
        w: 22.0,
        h: 22.0,
    }
}

/// Returns the submit button rectangle for the clone-agent dialog.
pub(crate) fn clone_agent_submit_button_rect(window: &Window) -> HudRect {
    let rect = clone_agent_dialog_rect(window);
    HudRect {
        x: rect.x + rect.w - 24.0 - ACTION_BUTTON_W,
        y: rect.y + rect.h - 40.0,
        w: ACTION_BUTTON_W,
        h: ACTION_BUTTON_H,
    }
}

/// Returns the editable name field rectangle within the rename-agent dialog.
pub(crate) fn rename_agent_name_field_rect(window: &Window) -> HudRect {
    let rect = rename_agent_dialog_rect(window);
    HudRect {
        x: rect.x + CREATE_AGENT_DIALOG_INSET_X + CREATE_AGENT_DIALOG_LABEL_W,
        y: rect.y + 68.0,
        w: rect.w - (CREATE_AGENT_DIALOG_INSET_X * 2.0 + CREATE_AGENT_DIALOG_LABEL_W),
        h: CREATE_AGENT_DIALOG_FIELD_HEIGHT,
    }
}

/// Returns the submit button rectangle for the rename-agent dialog.
pub(crate) fn rename_agent_submit_button_rect(window: &Window) -> HudRect {
    let rect = rename_agent_dialog_rect(window);
    HudRect {
        x: rect.x + rect.w - 24.0 - ACTION_BUTTON_W,
        y: rect.y + rect.h - 40.0,
        w: ACTION_BUTTON_W,
        h: ACTION_BUTTON_H,
    }
}

pub(crate) fn aegis_prompt_field_rect(window: &Window) -> HudRect {
    let rect = aegis_dialog_rect(window);
    HudRect {
        x: rect.x + CREATE_AGENT_DIALOG_INSET_X + CREATE_AGENT_DIALOG_LABEL_W,
        y: rect.y + 68.0,
        w: rect.w - (CREATE_AGENT_DIALOG_INSET_X * 2.0 + CREATE_AGENT_DIALOG_LABEL_W),
        h: CREATE_AGENT_DIALOG_FIELD_HEIGHT,
    }
}

pub(crate) fn aegis_enable_button_rect(window: &Window) -> HudRect {
    let rect = aegis_dialog_rect(window);
    HudRect {
        x: rect.x + rect.w - 24.0 - ACTION_BUTTON_W,
        y: rect.y + rect.h - 40.0,
        w: ACTION_BUTTON_W,
        h: ACTION_BUTTON_H,
    }
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
) -> [(CreateAgentKind, HudRect, &'static str); 4] {
    let rect = create_agent_dialog_rect(window);
    let row_y = create_agent_row_y(rect, 1) + 5.0;
    let left_x = rect.x + CREATE_AGENT_DIALOG_INSET_X + CREATE_AGENT_DIALOG_LABEL_W;
    let total_w = rect.w - (CREATE_AGENT_DIALOG_INSET_X * 2.0 + CREATE_AGENT_DIALOG_LABEL_W);
    let option_w = (total_w - CREATE_AGENT_DIALOG_RADIO_GAP * 3.0) / 4.0;
    [
        (
            CreateAgentKind::Pi,
            HudRect {
                x: left_x,
                y: row_y,
                w: option_w,
                h: CREATE_AGENT_DIALOG_RADIO_SIZE,
            },
            CreateAgentKind::Pi.label(),
        ),
        (
            CreateAgentKind::Claude,
            HudRect {
                x: left_x + (option_w + CREATE_AGENT_DIALOG_RADIO_GAP),
                y: row_y,
                w: option_w,
                h: CREATE_AGENT_DIALOG_RADIO_SIZE,
            },
            CreateAgentKind::Claude.label(),
        ),
        (
            CreateAgentKind::Codex,
            HudRect {
                x: left_x + (option_w + CREATE_AGENT_DIALOG_RADIO_GAP) * 2.0,
                y: row_y,
                w: option_w,
                h: CREATE_AGENT_DIALOG_RADIO_SIZE,
            },
            CreateAgentKind::Codex.label(),
        ),
        (
            CreateAgentKind::Terminal,
            HudRect {
                x: left_x + (option_w + CREATE_AGENT_DIALOG_RADIO_GAP) * 3.0,
                y: row_y,
                w: option_w,
                h: CREATE_AGENT_DIALOG_RADIO_SIZE,
            },
            CreateAgentKind::Terminal.label(),
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

/// Hit-tests the clone-agent dialog controls and returns the target under the pointer.
pub(crate) fn clone_agent_dialog_target_at(
    window: &Window,
    point: Vec2,
) -> Option<CloneAgentDialogTarget> {
    let name_rect = clone_agent_name_field_rect(window);
    if name_rect.contains(point) {
        return Some(CloneAgentDialogTarget::NameField);
    }
    let workdir_rect = clone_agent_workdir_rect(window);
    if workdir_rect.contains(point) {
        return Some(CloneAgentDialogTarget::WorkdirToggle);
    }
    let clone_rect = clone_agent_submit_button_rect(window);
    if clone_rect.contains(point) {
        return Some(CloneAgentDialogTarget::CloneButton);
    }
    None
}

/// Hit-tests the rename-agent dialog controls and returns the target under the pointer.
pub(crate) fn rename_agent_dialog_target_at(
    window: &Window,
    point: Vec2,
) -> Option<RenameAgentDialogTarget> {
    let name_rect = rename_agent_name_field_rect(window);
    if name_rect.contains(point) {
        return Some(RenameAgentDialogTarget::NameField);
    }
    let rename_rect = rename_agent_submit_button_rect(window);
    if rename_rect.contains(point) {
        return Some(RenameAgentDialogTarget::RenameButton);
    }
    None
}

pub(crate) fn aegis_dialog_target_at(window: &Window, point: Vec2) -> Option<AegisDialogTarget> {
    let prompt_rect = aegis_prompt_field_rect(window);
    if prompt_rect.contains(point) {
        return Some(AegisDialogTarget::PromptField);
    }
    let enable_rect = aegis_enable_button_rect(window);
    if enable_rect.contains(point) {
        return Some(AegisDialogTarget::EnableButton);
    }
    None
}
