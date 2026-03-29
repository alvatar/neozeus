use super::{
    persistence::{
        build_persisted_conversations, parse_persisted_conversations,
        resolve_conversations_path_with, restore_persisted_conversations,
        serialize_persisted_conversations,
    },
    AgentTaskStore, ConversationStore, MessageAuthor, MessageDeliveryState,
};
use crate::agents::{AgentId, AgentRuntimeIndex};

/// Verifies that ensure conversation is stable per agent.
#[test]
fn ensure_conversation_is_stable_per_agent() {
    let mut store = ConversationStore::default();
    let first = store.ensure_conversation(AgentId(1));
    let second = store.ensure_conversation(AgentId(1));
    assert_eq!(first, second);
}

/// Verifies that push message appends to conversation history.
#[test]
fn push_message_appends_to_conversation_history() {
    let mut store = ConversationStore::default();
    let conversation = store.ensure_conversation(AgentId(5));
    let message_id = store.push_message(
        conversation,
        MessageAuthor::User,
        "hello".into(),
        MessageDeliveryState::Pending,
    );
    let messages = store.messages_for(conversation);
    assert_eq!(messages.len(), 1);
    assert_eq!(message_id, 1);
    assert_eq!(messages[0].0, "hello");
}

/// Verifies that task store clear done and consume next update text.
#[test]
fn task_store_clear_done_and_consume_next_update_text() {
    let mut tasks = AgentTaskStore::default();
    let agent_id = AgentId(9);
    assert!(tasks.set_text(agent_id, "- [x] old\n- [ ] next"));
    assert!(tasks.clear_done(agent_id));
    assert_eq!(tasks.text(agent_id), Some("- [ ] next"));

    let consumed = tasks
        .consume_next(agent_id)
        .expect("next task should exist");
    assert_eq!(consumed, "next");
    assert_eq!(tasks.text(agent_id), Some("- [x] next"));
}

/// Verifies that conversation persistence roundtrips messages by session name.
#[test]
fn conversation_persistence_roundtrips_messages_by_session_name() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut store = ConversationStore::default();
    let conversation_id = store.ensure_conversation(AgentId(1));
    let _ = store.push_message(
        conversation_id,
        MessageAuthor::User,
        "hello\nworld".into(),
        MessageDeliveryState::Delivered,
    );
    let _ = store.push_message(
        conversation_id,
        MessageAuthor::User,
        "retry later".into(),
        MessageDeliveryState::Failed("transport".into()),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(
        AgentId(1),
        crate::terminals::TerminalId(7),
        "neozeus-session-a".into(),
        None,
    );

    let persisted = build_persisted_conversations(&store, &runtime_index);
    let text = serialize_persisted_conversations(&persisted);
    let parsed = parse_persisted_conversations(&text);
    assert_eq!(parsed, persisted);
}

/// Verifies that restore persisted conversations reattaches to restored agents.
#[test]
fn restore_persisted_conversations_reattaches_to_restored_agents() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut source = ConversationStore::default();
    let conversation_id = source.ensure_conversation(AgentId(1));
    let _ = source.push_message(
        conversation_id,
        MessageAuthor::User,
        "hello".into(),
        MessageDeliveryState::Delivered,
    );
    let mut source_runtime = AgentRuntimeIndex::default();
    source_runtime.link_terminal(
        AgentId(1),
        crate::terminals::TerminalId(1),
        "neozeus-session-a".into(),
        None,
    );
    let persisted = build_persisted_conversations(&source, &source_runtime);

    let mut restored = ConversationStore::default();
    let mut restored_runtime = AgentRuntimeIndex::default();
    restored_runtime.link_terminal(
        AgentId(9),
        crate::terminals::TerminalId(9),
        "neozeus-session-a".into(),
        None,
    );
    restore_persisted_conversations(&persisted, &restored_runtime, &mut restored);

    let restored_conversation = restored
        .conversation_for_agent(AgentId(9))
        .expect("restored conversation should be linked");
    let messages = restored.messages_for(restored_conversation);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "hello");
}

/// Verifies that conversations path prefers state home then home state then config.
#[test]
fn conversations_path_prefers_state_home_then_home_state_then_config() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    assert_eq!(
        resolve_conversations_path_with(Some("/tmp/state"), Some("/tmp/home"), Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/state/neozeus/conversations.v1"
        ))
    );
    assert_eq!(
        resolve_conversations_path_with(None, Some("/tmp/home"), Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/home/.local/state/neozeus/conversations.v1"
        ))
    );
    assert_eq!(
        resolve_conversations_path_with(None, None, Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/config/neozeus/conversations.v1"
        ))
    );
}
