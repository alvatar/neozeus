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
