use super::{
    persistence::{
        build_persisted_conversations, parse_persisted_conversations,
        resolve_conversations_path_with, restore_persisted_conversations,
        serialize_persisted_conversations,
    },
    sync_task_notes_projection, AgentTaskStore, ConversationStore, MessageAuthor,
    MessageDeliveryState,
};
use crate::agents::{AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex};
use bevy::{ecs::system::RunSystemOnce, prelude::*};

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

/// Verifies that conversation persistence roundtrips messages by stable agent uid.
#[test]
fn conversation_persistence_roundtrips_messages_by_agent_uid() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut store = ConversationStore::default();
    let conversation_id = store.ensure_conversation(AgentId(1));
    let _ = store.push_message(
        conversation_id,
        MessageAuthor::User,
        "hello\nworld \\\"quoted\\\"".into(),
        MessageDeliveryState::Delivered,
    );
    let _ = store.push_message(
        conversation_id,
        MessageAuthor::User,
        "retry later \\\\ fallback".into(),
        MessageDeliveryState::Failed("transport \"down\"".into()),
    );
    let mut agent_catalog = AgentCatalog::default();
    let agent_id = agent_catalog.create_agent(
        Some("alpha".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    assert_eq!(agent_id, AgentId(1));

    let persisted = build_persisted_conversations(&store, &agent_catalog);
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
    let mut source_catalog = AgentCatalog::default();
    let source_agent = source_catalog.create_agent(
        Some("alpha".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    assert_eq!(source_agent, AgentId(1));
    let persisted = build_persisted_conversations(&source, &source_catalog);

    let mut restored = ConversationStore::default();
    let mut restored_catalog = AgentCatalog::default();
    let restored_agent = restored_catalog.create_agent_with_uid_and_metadata(
        source_catalog.uid(source_agent).unwrap().to_owned(),
        Some("alpha-restored".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
        crate::agents::AgentMetadata::default(),
    );
    let restored_runtime = AgentRuntimeIndex::default();
    restore_persisted_conversations(
        &persisted,
        &restored_catalog,
        &restored_runtime,
        &mut restored,
    );

    let restored_conversation = restored
        .conversation_for_agent(restored_agent)
        .expect("restored conversation should be linked");
    let messages = restored.messages_for(restored_conversation);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "hello");
}

#[test]
fn restore_persisted_conversations_accepts_legacy_session_key_records() {
    let persisted = parse_persisted_conversations(
        "version 1\n[conversation]\nsession=\"neozeus-session-a\"\n[message]\ndelivery=\"delivered\"\nbody=\"hello\"\n",
    );
    let mut restored = ConversationStore::default();
    let restored_catalog = AgentCatalog::default();
    let mut restored_runtime = AgentRuntimeIndex::default();
    restored_runtime.link_terminal(
        AgentId(9),
        crate::terminals::TerminalId(9),
        "neozeus-session-a".into(),
        None,
    );
    restore_persisted_conversations(
        &persisted,
        &restored_catalog,
        &restored_runtime,
        &mut restored,
    );

    let restored_conversation = restored
        .conversation_for_agent(AgentId(9))
        .expect("legacy session keyed conversation should restore");
    assert_eq!(restored.messages_for(restored_conversation)[0].0, "hello");
}

#[test]
fn sync_task_notes_projection_writes_only_agent_uid_notes() {
    let mut world = World::default();
    let mut agent_catalog = AgentCatalog::default();
    let agent_id = agent_catalog.create_agent(
        Some("alpha".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let agent_uid = agent_catalog.uid(agent_id).unwrap().to_owned();
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(
        agent_id,
        crate::terminals::TerminalId(1),
        "neozeus-session-a".into(),
        None,
    );
    let mut tasks = AgentTaskStore::default();
    assert!(tasks.set_text(agent_id, "- [ ] task"));
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(agent_catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(tasks);
    world.insert_resource(crate::terminals::TerminalNotesState::default());

    world.run_system_once(sync_task_notes_projection).unwrap();

    let notes = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(notes.note_text_by_agent_uid(&agent_uid), Some("- [ ] task"));
    assert_eq!(notes.note_text("neozeus-session-a"), None);
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
