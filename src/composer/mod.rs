mod editor;
mod layout;
mod state;

pub(crate) use layout::{
    create_agent_create_button_rect, create_agent_dialog_rect, create_agent_dialog_target_at,
    create_agent_kind_option_rects, create_agent_name_field_rect,
    create_agent_starting_folder_rect, message_box_action_at, message_box_action_buttons,
    message_box_rect, task_dialog_action_at, task_dialog_action_buttons, task_dialog_rect,
    CreateAgentDialogTarget, MessageBoxAction, TaskDialogAction,
};
pub(crate) use state::{
    ComposerMode, ComposerState, MessageDialogFocus, TaskDialogFocus, TextEditorState,
};

#[cfg(test)]
pub(crate) use state::ComposerSession;

#[cfg(test)]
mod tests;
