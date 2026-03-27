use super::{sync_hud_view_models, AgentListView, ComposerView, ConversationListView, ThreadView};
use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::AppSessionState,
    conversations::{AgentTaskStore, ConversationStore, MessageAuthor, MessageDeliveryState},
    tests::{insert_terminal_manager_resources, test_bridge},
};
use bevy::{ecs::system::RunSystemOnce, prelude::*};

#[test]
fn sync_hud_view_models_derives_agent_rows_and_threads() {
    let (bridge, _) = test_bridge();
    let mut manager = crate::terminals::TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentCapabilities::terminal_defaults(),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut app_session = AppSessionState {
        active_agent: Some(agent_id),
        ..Default::default()
    };
    app_session.composer.session = Some(crate::ui::ComposerSession {
        mode: crate::ui::ComposerMode::Message { agent_id },
    });
    app_session.composer.message_editor.visible = true;
    app_session.composer.message_editor.text = "hello".into();

    let mut tasks = AgentTaskStore::default();
    tasks.set_text(agent_id, "- [ ] follow up");

    let mut conversations = ConversationStore::default();
    let conversation_id = conversations.ensure_conversation(agent_id);
    conversations.push_message(
        conversation_id,
        MessageAuthor::User,
        "hello".into(),
        MessageDeliveryState::Delivered,
    );

    let mut world = World::default();
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(app_session);
    world.insert_resource(tasks);
    world.insert_resource(conversations);
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    insert_terminal_manager_resources(&mut world, manager);

    world.run_system_once(sync_hud_view_models).unwrap();

    let agent_list = world.resource::<AgentListView>();
    assert_eq!(agent_list.rows.len(), 1);
    assert_eq!(agent_list.rows[0].label, "alpha");
    assert!(agent_list.rows[0].focused);
    assert!(agent_list.rows[0].has_tasks);

    let thread = world.resource::<ThreadView>();
    assert_eq!(thread.messages.len(), 1);
    assert_eq!(thread.messages[0].body, "hello");

    let composer = world.resource::<ComposerView>();
    assert!(composer.visible);
    assert_eq!(composer.text, "hello");
}
