mod editor;
mod layout;
mod state;

pub(crate) use layout::{
    message_box_action_at, message_box_action_buttons, message_box_rect, task_dialog_action_at,
    task_dialog_action_buttons, task_dialog_rect, MessageBoxAction, TaskDialogAction,
};
pub(crate) use state::{ComposerMode, ComposerState, TextEditorState};

#[cfg(test)]
pub(crate) use state::ComposerSession;

#[cfg(test)]
mod tests;
