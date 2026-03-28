use super::{ComposerMode, ComposerState};
use crate::agents::AgentId;

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

#[test]
fn task_editor_reopens_from_supplied_text_not_stale_buffer() {
    let mut composer = ComposerState::default();
    composer.open_task_editor(AgentId(1), "one");
    composer.task_editor.insert_text("\ntwo");
    composer.close_task_editor();

    composer.open_task_editor(AgentId(1), "fresh");
    assert_eq!(composer.task_editor.text, "fresh");
}
