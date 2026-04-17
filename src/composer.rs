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
    message_box_body_rect, message_box_rect, message_box_shortcut_button_at,
    message_box_shortcut_button_rects, message_box_visible_cols, rename_agent_dialog_rect,
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
mod tests {
    use super::{create_agent_dialog_rect, ComposerMode, ComposerState};
    use crate::agents::AgentId;
    use bevy::window::Window;

    /// Verifies that message composer preserves per agent drafts.
    #[test]
    fn message_composer_preserves_per_agent_drafts() {
        let mut composer = ComposerState::default();
        composer.open_message(AgentId(1));
        composer.message_editor.insert_text("alpha");
        composer.cancel_preserving_draft();

        composer.open_message(AgentId(2));
        composer.message_editor.insert_text("beta");
        composer.cancel_preserving_draft();

        composer.open_message(AgentId(1));
        assert_eq!(composer.message_editor.text, "alpha");
        assert_eq!(
            composer.session.unwrap().mode,
            ComposerMode::Message {
                agent_id: AgentId(1)
            }
        );
    }

    /// Verifies that task editor reopens from supplied text not stale buffer.
    #[test]
    fn task_editor_reopens_from_supplied_text_not_stale_buffer() {
        let mut composer = ComposerState::default();
        composer.open_task_editor(AgentId(1), "one");
        composer.task_editor.insert_text("\ntwo");
        composer.close_task_editor();

        composer.open_task_editor(AgentId(1), "fresh");
        assert_eq!(composer.task_editor.text, "fresh");
    }

    /// Verifies that the create-agent dialog is centered in the window instead of top-aligned like the
    /// message box.
    #[test]
    fn create_agent_dialog_rect_is_centered() {
        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };

        let rect = create_agent_dialog_rect(&window);
        assert!((rect.x - (1400.0 - rect.w) * 0.5).abs() < 0.01);
        assert!((rect.y - (900.0 - rect.h) * 0.5).abs() < 0.01);
    }
}
