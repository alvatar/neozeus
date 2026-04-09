mod editor;
mod layout;
mod state;
mod wrapping;

#[allow(
    unused_imports,
    reason = "some layout helpers are test-only call sites but remain part of the composer surface"
)]
pub(crate) use layout::{
    aegis_dialog_rect, aegis_dialog_target_at, aegis_enable_button_rect, aegis_prompt_field_rect,
    aegis_visible_cols, clone_agent_dialog_rect, clone_agent_dialog_target_at,
    clone_agent_name_field_rect, clone_agent_submit_button_rect, clone_agent_workdir_rect,
    create_agent_create_button_rect, create_agent_dialog_rect, create_agent_dialog_target_at,
    create_agent_kind_option_rects, create_agent_name_field_rect,
    create_agent_starting_folder_rect, message_box_action_at, message_box_action_buttons,
    message_box_rect, message_box_visible_cols, rename_agent_dialog_rect,
    rename_agent_dialog_target_at, rename_agent_name_field_rect, rename_agent_submit_button_rect,
    reset_dialog_buttons, reset_dialog_rect, reset_dialog_target_at, task_dialog_action_at,
    task_dialog_action_buttons, task_dialog_rect, task_dialog_visible_cols, AegisDialogTarget,
    CloneAgentDialogTarget, CreateAgentDialogTarget, MessageBoxAction, RenameAgentDialogTarget,
    ResetDialogTarget, TaskDialogAction,
};
pub(crate) use state::{
    ComposerMode, ComposerState, MessageDialogFocus, TaskDialogFocus, TextEditorState,
};
#[allow(
    unused_imports,
    reason = "shared wrapped-row type is referenced by renderer helpers and tests"
)]
pub(crate) use wrapping::{wrapped_text_rows, wrapped_text_rows_measured, WrappedTextRow};

#[cfg(test)]
pub(crate) use state::ComposerSession;

#[cfg(test)]
mod tests;
