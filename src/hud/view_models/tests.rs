use super::{
    parse_agent_context_pct_milli, selected_agent_id, selected_agent_list_row_key,
    sync_hud_view_models, sync_info_bar_view_model, AgentListActivity, AgentListRowKey,
    AgentListRowKind, AgentListSelection, AgentListView, ComposerView, ConversationListView,
    InfoBarView, OwnedTmuxOwnerBinding, ThreadView,
};
use crate::{
    agents::{AgentCatalog, AgentKind, AgentMetadata, AgentRuntimeIndex, AgentStatusStore},
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

#[test]
fn selected_agent_list_row_key_returns_none_for_none_selection() {
    assert_eq!(selected_agent_list_row_key(&AgentListSelection::None), None);
}

#[test]
fn selected_agent_id_returns_agent_only_for_agent_selection() {
    assert_eq!(
        selected_agent_id(&AgentListSelection::Agent(crate::agents::AgentId(7))),
        Some(crate::agents::AgentId(7))
    );
    assert_eq!(selected_agent_id(&AgentListSelection::None), None);
    assert_eq!(
        selected_agent_id(&AgentListSelection::OwnedTmux("tmux-7".into())),
        None
    );
}

#[test]
fn selected_agent_list_row_key_returns_agent_row() {
    assert_eq!(
        selected_agent_list_row_key(&AgentListSelection::Agent(crate::agents::AgentId(7))),
        Some(AgentListRowKey::Agent(crate::agents::AgentId(7)))
    );
}

#[test]
fn selected_agent_list_row_key_returns_owned_tmux_row() {
    assert_eq!(
        selected_agent_list_row_key(&AgentListSelection::OwnedTmux("tmux-7".into())),
        Some(AgentListRowKey::OwnedTmux("tmux-7".into()))
    );
}

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

    let mut app_session = AppSessionState::default();
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
    world.insert_resource(AgentListSelection::Agent(agent_id));
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    insert_terminal_manager_resources(&mut world, manager);

    world.run_system_once(sync_hud_view_models).unwrap();

    let agent_list = world.resource::<AgentListView>();
    assert_eq!(agent_list.rows.len(), 1);
    assert_eq!(agent_list.rows[0].label, "ALPHA");
    assert!(agent_list.rows[0].focused);
    match &agent_list.rows[0].kind {
        AgentListRowKind::Agent {
            has_tasks,
            activity,
            context_pct_milli,
            ..
        } => {
            assert!(*has_tasks);
            assert_eq!(*activity, AgentListActivity::Idle);
            assert_eq!(*context_pct_milli, None);
        }
        other => panic!("expected agent row, got {other:?}"),
    }

    let thread = world.resource::<ThreadView>();
    assert_eq!(thread.header, "ALPHA");
    let rows = thread.message_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "hello");

    let composer = world.resource::<ComposerView>();
    assert!(composer.visible);
    assert_eq!(composer.title.as_deref(), Some("Message ALPHA"));
    assert_eq!(composer.text, "hello");
}

#[test]
fn sync_hud_view_models_places_owned_tmux_rows_under_matching_agent() {
    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();

    let mut world = World::default();
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    world.insert_resource(AgentListSelection::None);
    let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
    owned_tmux
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-1".into(),
            owner_agent_uid: agent_uid,
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 1,
        });
    world.insert_resource(owned_tmux);
    world.insert_resource(crate::terminals::TerminalManager::default());

    world.run_system_once(sync_hud_view_models).unwrap();
    let rows = &world.resource::<AgentListView>().rows;
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[0].key, super::AgentListRowKey::Agent(_)));
    assert!(matches!(rows[1].key, super::AgentListRowKey::OwnedTmux(_)));
}

#[test]
fn sync_hud_view_models_prefixes_workdir_agents_with_marker() {
    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent_with_metadata(
        Some("alpha".into()),
        AgentKind::Pi,
        AgentKind::Pi.capabilities(),
        AgentMetadata {
            clone_source_session_path: Some("/tmp/pi-alpha.jsonl".into()),
            is_workdir: true,
        },
    );

    let mut world = World::default();
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    world.insert_resource(AgentListSelection::Agent(agent_id));
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::TerminalManager::default());

    world.run_system_once(sync_hud_view_models).unwrap();

    let rows = &world.resource::<AgentListView>().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "⎇ ALPHA");
}

#[test]
fn sync_hud_view_models_orders_multiple_owned_tmux_rows_and_marks_selected_child() {
    let mut catalog = AgentCatalog::default();
    let alpha = catalog.create_agent(
        Some("ALPHA".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let beta = catalog.create_agent(
        Some("BETA".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let alpha_uid = catalog.uid(alpha).unwrap().to_owned();
    let beta_uid = catalog.uid(beta).unwrap().to_owned();

    let mut world = World::default();
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    world.insert_resource(AgentListSelection::OwnedTmux("tmux-a2".into()));
    world.insert_resource(crate::terminals::TerminalManager::default());
    let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
    owned_tmux.sessions = vec![
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-b1".into(),
            owner_agent_uid: beta_uid,
            tmux_name: "neozeus-tmux-b1".into(),
            display_name: "BETA BUILD".into(),
            cwd: "/tmp/beta".into(),
            attached: false,
            created_unix: 3,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a2".into(),
            owner_agent_uid: alpha_uid.clone(),
            tmux_name: "neozeus-tmux-a2".into(),
            display_name: "ALPHA TEST".into(),
            cwd: "/tmp/alpha-2".into(),
            attached: true,
            created_unix: 2,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-orphan".into(),
            owner_agent_uid: "missing-agent".into(),
            tmux_name: "neozeus-tmux-orphan".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/orphan".into(),
            attached: false,
            created_unix: 4,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a1".into(),
            owner_agent_uid: alpha_uid,
            tmux_name: "neozeus-tmux-a1".into(),
            display_name: "ALPHA BUILD".into(),
            cwd: "/tmp/alpha-1".into(),
            attached: false,
            created_unix: 1,
        },
    ];
    world.insert_resource(owned_tmux);

    world.run_system_once(sync_hud_view_models).unwrap();
    let rows = &world.resource::<AgentListView>().rows;
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[0].label, "ALPHA");
    assert_eq!(rows[1].label, "ALPHA BUILD");
    assert_eq!(rows[2].label, "ALPHA TEST");
    assert_eq!(rows[3].label, "BETA");
    assert_eq!(rows[4].label, "BETA BUILD");
    assert_eq!(rows[5].label, "BUILD");
    assert!(!rows[0].focused);
    assert!(!rows[1].focused);
    assert!(rows[2].focused);
    assert!(matches!(
        rows[5].kind,
        AgentListRowKind::OwnedTmux {
            owner: OwnedTmuxOwnerBinding::Orphan,
            ..
        }
    ));
    assert_eq!(rows.iter().filter(|row| row.focused).count(), 1);
}

#[test]
fn sync_hud_view_models_clears_thread_and_conversation_selection_for_tmux_rows() {
    let mut catalog = AgentCatalog::default();
    let alpha = catalog.create_agent(
        Some("ALPHA".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let alpha_uid = catalog.uid(alpha).unwrap().to_owned();
    let mut conversations = ConversationStore::default();
    let conversation_id = conversations.ensure_conversation(alpha);
    conversations.push_message(
        conversation_id,
        MessageAuthor::User,
        "hello".into(),
        MessageDeliveryState::Delivered,
    );

    let mut world = World::default();
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(conversations);
    world.insert_resource(AgentListSelection::OwnedTmux("tmux-a1".into()));
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
    owned_tmux
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a1".into(),
            owner_agent_uid: alpha_uid,
            tmux_name: "neozeus-tmux-a1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/alpha-1".into(),
            attached: false,
            created_unix: 1,
        });
    world.insert_resource(owned_tmux);

    world.run_system_once(sync_hud_view_models).unwrap();

    let conversations = &world.resource::<ConversationListView>().rows;
    assert_eq!(conversations.len(), 1);
    assert!(!conversations[0].selected);
    let thread = world.resource::<ThreadView>();
    assert_eq!(thread.header, "No thread selected");
    assert!(thread.message_rows().is_empty());
}

#[test]
fn sync_hud_view_models_projects_selected_tmux_row_only() {
    let mut catalog = AgentCatalog::default();
    let alpha = catalog.create_agent(
        Some("ALPHA".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let alpha_uid = catalog.uid(alpha).unwrap().to_owned();

    let mut world = World::default();
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentListSelection::OwnedTmux("tmux-a1".into()));
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
    owned_tmux
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a1".into(),
            owner_agent_uid: alpha_uid,
            tmux_name: "neozeus-tmux-a1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/alpha-1".into(),
            attached: false,
            created_unix: 1,
        });
    world.insert_resource(owned_tmux);

    world.run_system_once(sync_hud_view_models).unwrap();
    let rows = &world.resource::<AgentListView>().rows;
    assert_eq!(rows.iter().filter(|row| row.focused).count(), 1);
    assert!(matches!(rows[0].key, AgentListRowKey::Agent(_)));
    assert!(!rows[0].focused);
    assert!(matches!(rows[1].key, AgentListRowKey::OwnedTmux(_)));
    assert!(rows[1].focused);
}

#[test]
fn sync_hud_view_models_tmux_rows_have_no_activity_state() {
    let mut catalog = AgentCatalog::default();
    let alpha = catalog.create_agent(
        Some("ALPHA".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let alpha_uid = catalog.uid(alpha).unwrap().to_owned();

    let mut world = World::default();
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentListSelection::None);
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
    owned_tmux
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a1".into(),
            owner_agent_uid: alpha_uid,
            tmux_name: "neozeus-tmux-a1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/alpha-1".into(),
            attached: false,
            created_unix: 1,
        });
    world.insert_resource(owned_tmux);

    world.run_system_once(sync_hud_view_models).unwrap();
    let rows = &world.resource::<AgentListView>().rows;
    assert!(matches!(rows[1].kind, AgentListRowKind::OwnedTmux { .. }));
}

#[test]
fn sync_hud_view_models_routes_unknown_owned_tmux_to_orphan_row() {
    let mut world = World::default();
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(ConversationListView::default());
    world.insert_resource(ThreadView::default());
    world.insert_resource(ComposerView::default());
    world.insert_resource(AgentStatusStore::default());
    world.insert_resource(AgentListSelection::None);
    let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
    owned_tmux
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-orphan".into(),
            owner_agent_uid: "missing-agent".into(),
            tmux_name: "neozeus-tmux-orphan".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 1,
        });
    world.insert_resource(owned_tmux);
    world.insert_resource(crate::terminals::TerminalManager::default());

    world.run_system_once(sync_hud_view_models).unwrap();
    let rows = &world.resource::<AgentListView>().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "BUILD");
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
    world.insert_resource(AgentListSelection::None);
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    insert_terminal_manager_resources(&mut world, manager);

    world
        .run_system_once(crate::agents::sync_agent_status)
        .unwrap();
    world.run_system_once(sync_hud_view_models).unwrap();

    let agent_list = world.resource::<AgentListView>();
    match &agent_list.rows[0].kind {
        AgentListRowKind::Agent { activity, .. } => {
            assert_eq!(*activity, AgentListActivity::Working)
        }
        other => panic!("expected agent row, got {other:?}"),
    }
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
    world.insert_resource(AgentListSelection::None);
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    insert_terminal_manager_resources(&mut world, manager);

    world.run_system_once(sync_hud_view_models).unwrap();
    match &world.resource::<AgentListView>().rows[0].kind {
        AgentListRowKind::Agent {
            context_pct_milli, ..
        } => *context_pct_milli,
        other => panic!("expected agent row, got {other:?}"),
    }
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
            requests_limit: 100,
            requests_remaining: 60,
            tokens_limit: 1_000,
            tokens_remaining: 250,
            requests_resets_at: "4h55m".into(),
            tokens_resets_at: "4d00h".into(),
            available: true,
        },
    });
    world.insert_resource(test_usage_persistence_state());
    world.insert_resource(InfoBarView::default());

    world.run_system_once(sync_info_bar_view_model).unwrap();

    let info = world.resource::<InfoBarView>();
    assert_eq!(info.claude_session.label, "Claude Session:");
    assert_eq!(info.claude_session.pct_milli, 42_000);
    assert_eq!(info.claude_session.detail_text, "(5m)");
    assert_eq!(info.claude_week.label, "Week:");
    assert_eq!(info.claude_week.pct_milli, 10_000);
    assert_eq!(info.claude_week.detail_text, "(2h00m)");
    assert_eq!(info.openai_session.label, "OpenAI Session:");
    assert_eq!(info.openai_session.pct_milli, 40_000);
    assert_eq!(info.openai_session.detail_text, "(4h55m)");
    assert_eq!(info.openai_week.label, "Week:");
    assert_eq!(info.openai_week.pct_milli, 75_000);
    assert_eq!(info.openai_week.detail_text, "(4d00h)");
}

#[test]
fn sync_info_bar_view_model_handles_unavailable_sources() {
    let mut world = World::default();
    world.insert_resource(UsageSnapshot::default());
    world.insert_resource(test_usage_persistence_state());
    world.insert_resource(InfoBarView::default());

    world.run_system_once(sync_info_bar_view_model).unwrap();

    let info = world.resource::<InfoBarView>();
    assert!(!info.claude_session.available);
    assert!(!info.openai_session.available);
}

#[test]
fn sync_info_bar_view_model_reports_claude_backoff() {
    let persistence = test_usage_persistence_state();
    fs::create_dir_all(&persistence.state_dir).unwrap();
    fs::write(
        &persistence.claude_backoff_until_path,
        format!("{}", current_unix() + 120),
    )
    .unwrap();

    let mut world = World::default();
    world.insert_resource(UsageSnapshot {
        claude: ClaudeUsageData {
            session_pct: 12.0,
            available: true,
            ..Default::default()
        },
        ..Default::default()
    });
    world.insert_resource(persistence);
    world.insert_resource(InfoBarView::default());

    world.run_system_once(sync_info_bar_view_model).unwrap();
    assert_eq!(
        world.resource::<InfoBarView>().claude_session.detail_text,
        "RL"
    );
}

fn current_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
