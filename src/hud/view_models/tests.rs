use super::{
    parse_agent_context_pct_milli, sync_hud_view_models, sync_info_bar_view_model, AgentListView,
    ComposerView, ConversationListView, InfoBarView, ThreadView,
};
use crate::{
    agents::{AgentCatalog, AgentKind, AgentRuntimeIndex, AgentStatus, AgentStatusStore},
    app::AppSessionState,
    conversations::{AgentTaskStore, ConversationStore, MessageAuthor, MessageDeliveryState},
    tests::{insert_terminal_manager_resources, surface_with_text, test_bridge},
    usage::{ClaudeUsageData, OpenAiUsageData, UsagePersistenceState, UsageSnapshot},
};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

/// Verifies that sync hud view models derives agent rows and threads.
#[test]
fn sync_hud_view_models_derives_agent_rows_and_threads() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
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
    app_session.composer.session = Some(crate::composer::ComposerSession {
        mode: crate::composer::ComposerMode::Message { agent_id },
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
    world.insert_resource(AgentStatusStore::default());
    insert_terminal_manager_resources(&mut world, manager);

    world.run_system_once(sync_hud_view_models).unwrap();

    let agent_list = world.resource::<AgentListView>();
    assert_eq!(agent_list.rows.len(), 1);
    assert_eq!(agent_list.rows[0].label, "alpha");
    assert!(agent_list.rows[0].focused);
    assert!(agent_list.rows[0].has_tasks);
    assert_eq!(agent_list.rows[0].status, AgentStatus::Unknown);

    let thread = world.resource::<ThreadView>();
    let rows = thread.message_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "hello");

    let composer = world.resource::<ComposerView>();
    assert!(composer.visible);
    assert_eq!(composer.title.as_deref(), Some("Message alpha"));
    assert_eq!(composer.text, "hello");
}

#[test]
fn sync_hud_view_models_carries_agent_working_status_into_rows() {
    let (bridge, _) = test_bridge();
    let mut manager = crate::terminals::TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some({
        let mut surface = crate::tests::surface_with_text(8, 120, 0, "header");
        surface.set_text_cell(1, 3, "⠋ Working...");
        surface
    });

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    insert_terminal_manager_resources(&mut world, manager);

    world
        .run_system_once(crate::agents::sync_agent_status)
        .unwrap();
    world.run_system_once(sync_hud_view_models).unwrap();

    let agent_list = world.resource::<AgentListView>();
    assert_eq!(agent_list.rows[0].status, AgentStatus::Working);
}

#[test]
fn sync_hud_view_models_leaves_missing_context_empty() {
    assert_eq!(
        synced_context_pct(AgentKind::Terminal, surface_with_text(8, 120, 0, "header")),
        None
    );
}

#[test]
fn parse_agent_context_pct_milli_parses_pi_footer_context_percentage() {
    let mut surface = surface_with_text(8, 120, 0, "header");
    surface.set_text_cell(
        0,
        7,
        "claude-opus-4-6 (high) Ctx(auto):░░░░░░░░░░(42.5%) Session:██████░░░░(59.0%) Week:█░░░░░░░░░(14.0%) ↑0 ↓0",
    );

    assert_eq!(parse_agent_context_pct_milli(&surface), Some(42_500));
}

#[test]
fn parse_agent_context_pct_milli_parses_codex_footer_remaining_context() {
    let mut surface = surface_with_text(8, 120, 0, "header");
    surface.set_text_cell(0, 7, "  gpt-5.4 default · 83% left · ~/code");

    assert_eq!(parse_agent_context_pct_milli(&surface), Some(17_000));
}

fn synced_context_pct(kind: AgentKind, surface: crate::terminals::TerminalSurface) -> Option<i32> {
    let (bridge, _) = test_bridge();
    let mut manager = crate::terminals::TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some(surface);

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(Some("alpha".into()), kind, kind.capabilities());
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut world = World::default();
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    insert_terminal_manager_resources(&mut world, manager);

    world.run_system_once(sync_hud_view_models).unwrap();
    world.resource::<AgentListView>().rows[0].context_pct_milli
}

fn temp_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("neozeus-view-models-{name}-{unique}"))
}

fn test_usage_persistence_state() -> UsagePersistenceState {
    let state_dir = temp_path("usage-state");
    UsagePersistenceState {
        state_dir: state_dir.clone(),
        claude_cache_path: state_dir.join("claude-cache.json"),
        openai_cache_path: state_dir.join("openai-cache.json"),
        claude_log_path: state_dir.join("claude.log"),
        openai_log_path: state_dir.join("openai.log"),
        claude_backoff_until_path: state_dir.join("claude-backoff.txt"),
        claude_refresh_lock_path: state_dir.join("claude-refresh.lock"),
        openai_refresh_lock_path: state_dir.join("openai-refresh.lock"),
        helper_script_path: PathBuf::from("scripts/usage_fetch.py"),
        python_program: PathBuf::from("python3"),
        last_claude_refresh_attempt_secs: None,
        last_openai_refresh_attempt_secs: None,
    }
}

/// Verifies that the info bar derives Zeus-style usage rows from the normalized usage snapshot.
#[test]
fn sync_info_bar_view_model_derives_usage_rows() {
    let mut world = World::default();
    world.insert_resource(UsageSnapshot {
        claude: ClaudeUsageData {
            session_pct: 42.0,
            week_pct: 10.0,
            session_resets_at: "5m".into(),
            week_resets_at: "2h".into(),
            available: true,
            ..Default::default()
        },
        openai: OpenAiUsageData {
            requests_pct_milli: 40_000,
            tokens_pct_milli: 75_000,
            requests_resets_at: "45s".into(),
            tokens_resets_at: "24h".into(),
            available: true,
            ..Default::default()
        },
    });
    world.insert_resource(test_usage_persistence_state());
    world.insert_resource(InfoBarView::default());

    world.run_system_once(sync_info_bar_view_model).unwrap();

    let info_bar = world.resource::<InfoBarView>();
    assert_eq!(info_bar.claude_session.label, "Claude Session:");
    assert_eq!(info_bar.claude_session.pct_milli, 42_000);
    assert_eq!(info_bar.claude_session.detail_text, "(5m)");
    assert_eq!(info_bar.claude_week.detail_text, "(2h00m)");
    assert_eq!(info_bar.openai_session.label, "OpenAI Session:");
    assert_eq!(info_bar.openai_session.pct_milli, 40_000);
    assert_eq!(info_bar.openai_session.detail_text, "(45s)");
    assert_eq!(info_bar.openai_week.detail_text, "(1d00h)");
}

/// Verifies that unavailable providers produce explicit unavailable session text without panicking.
#[test]
fn sync_info_bar_view_model_marks_unavailable_providers_explicitly() {
    let mut world = World::default();
    world.insert_resource(UsageSnapshot::default());
    world.insert_resource(test_usage_persistence_state());
    world.insert_resource(InfoBarView::default());

    world.run_system_once(sync_info_bar_view_model).unwrap();

    let info_bar = world.resource::<InfoBarView>();
    assert_eq!(info_bar.claude_session.detail_text, "(unavailable)");
    assert_eq!(info_bar.claude_week.detail_text, "");
    assert_eq!(info_bar.openai_session.detail_text, "(unavailable)");
    assert_eq!(info_bar.openai_week.detail_text, "");
}

/// Verifies that Claude backoff is surfaced as an explicit rate-limited UI state instead of a
/// generic unavailable marker.
#[test]
fn sync_info_bar_view_model_marks_rate_limited_claude_explicitly() {
    let persistence = test_usage_persistence_state();
    fs::create_dir_all(&persistence.state_dir).unwrap();
    let backoff_until = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 900;
    fs::write(
        &persistence.claude_backoff_until_path,
        backoff_until.to_string(),
    )
    .unwrap();

    let mut world = World::default();
    world.insert_resource(UsageSnapshot::default());
    world.insert_resource(persistence);
    world.insert_resource(InfoBarView::default());

    world.run_system_once(sync_info_bar_view_model).unwrap();

    let info_bar = world.resource::<InfoBarView>();
    assert_eq!(info_bar.claude_session.detail_text, "(rate limited)");
}
