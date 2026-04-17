//! Test submodule: `startup_restore` — extracted from the centralized test bucket.

#![allow(unused_imports)]

use crate::{
    app::{
        format_startup_panic, normalize_output_for_x11_fallback, primary_window_config_for,
        primary_window_plugin_config_for, resolve_disable_pipelined_rendering_for,
        resolve_force_fallback_adapter, resolve_force_fallback_adapter_for,
        resolve_linux_window_backend, resolve_output_dimension, resolve_output_mode,
        resolve_window_mode, resolve_window_scale_factor, should_force_x11_backend,
        uses_headless_runner, AppOutputConfig, LinuxWindowBackend, OutputMode,
    },
    hud::{HudState, HudWidgetKey, TerminalVisibilityPolicy},
    startup::{
        advance_startup_connecting, choose_startup_focus_session_name,
        request_redraw_while_visuals_active, should_request_visual_redraw,
        startup_visibility_policy_for_focus, DaemonConnectionState, StartupConnectPhase,
        StartupConnectState,
    },
    terminals::{
        TerminalId, TerminalPanel, TerminalPresentation, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalTextureState,
    },
    tests::{
        fake_daemon_resource, fake_runtime_spawner, insert_default_hud_resources,
        insert_terminal_manager_resources, insert_test_hud_state, surface_with_text, temp_dir,
        test_bridge, FakeDaemonClient,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::*,
    window::{RequestRedraw, WindowMode},
};
use std::sync::Arc;



use super::support::*;

/// Verifies that startup restore only marks interactive sessions as startup-pending.
#[test]
fn startup_restore_does_not_mark_disconnected_sessions_as_startup_pending() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-live",
        crate::terminals::TerminalRuntimeState::running("live session"),
    );
    client.set_session_runtime(
        "neozeus-session-dead",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let dir = temp_dir("neozeus-startup-disconnected-not-pending");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        concat!(
            "neozeus state version 1\n",
            "[agent]\n",
            "agent_uid=\"agent-live\"\n",
            "session_name=\"neozeus-session-live\"\n",
            "label=\"LIVE\"\n",
            "kind=\"pi\"\n",
            "order_index=0\n",
            "focused=1\n",
            "[/agent]\n",
            "[agent]\n",
            "agent_uid=\"agent-dead\"\n",
            "session_name=\"neozeus-session-dead\"\n",
            "label=\"DEAD\"\n",
            "kind=\"pi\"\n",
            "order_index=1\n",
            "focused=0\n",
            "[/agent]\n",
        ),
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
    let live_terminal = runtime_index
        .agent_for_session("neozeus-session-live")
        .and_then(|agent_id| runtime_index.primary_terminal(agent_id))
        .expect("live terminal should be attached");
    let dead_terminal = runtime_index
        .agent_for_session("neozeus-session-dead")
        .and_then(|agent_id| runtime_index.primary_terminal(agent_id))
        .expect("dead terminal should be attached");
    let presentation_store = world.resource::<crate::terminals::TerminalPresentationStore>();
    assert!(presentation_store.is_startup_bootstrap_pending(live_terminal));
    assert!(
        !presentation_store.is_startup_bootstrap_pending(dead_terminal),
        "disconnected restored terminals must not stay startup-pending forever"
    );
}


/// Verifies that restoring legacy app-state entries without stable agent uids backfills a new uid
/// and marks app state dirty for rewrite.
#[test]
fn startup_restore_backfills_missing_agent_uid_and_marks_app_state_dirty() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-agent-uid-backfill");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("legacy app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog.order.first().expect("restored agent should exist");
    let restored_uid = catalog.uid(restored_agent).expect("uid should backfill");
    assert!(!restored_uid.trim().is_empty());
    assert_eq!(catalog.find_by_uid(restored_uid), Some(restored_agent));
    assert_eq!(
        world
            .resource::<crate::app::AppStatePersistenceState>()
            .dirty_since_secs,
        Some(0.0)
    );
    let session_metadata = client.session_metadata.lock().unwrap();
    let mirrored = session_metadata
        .get("neozeus-session-a")
        .expect("restored session should mirror app-owned identity back into daemon metadata");
    assert_eq!(mirrored.agent_uid.as_deref(), Some(restored_uid));
    assert_eq!(mirrored.agent_label.as_deref(), Some("ALPHA"));
    assert_eq!(
        mirrored.agent_kind,
        Some(crate::shared::daemon_wire::DaemonAgentKind::Pi)
    );
}


#[test]
fn startup_restore_rehydrates_aegis_policy_from_persisted_snapshot() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-legacy-aegis-ignored");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 2\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\naegis_enabled=1\naegis_prompt_text=\"continue cleanly\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog.order.first().expect("restored agent should exist");
    assert_eq!(catalog.uid(restored_agent), Some("agent-uid-1"));
    let _ = catalog;

    let policy = world
        .resource::<crate::aegis::AegisPolicyStore>()
        .policy("agent-uid-1")
        .expect("persisted Aegis policy should restore");
    assert!(policy.enabled);
    assert_eq!(policy.prompt_text, "continue cleanly");
    assert!(world
        .resource::<crate::aegis::AegisRuntimeStore>()
        .state(restored_agent)
        .is_none());
}


#[test]
fn startup_restore_migrates_legacy_session_notes_into_task_store_and_marks_notes_dirty() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-legacy-notes-migration");
    let app_state_path = dir.join("neozeus-state.v1");
    let notes_path = dir.join("notes.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");
    std::fs::write(
        &notes_path,
        "version 2\nnote name=neozeus-session-a\n- [ ] legacy task\n.\n",
    )
    .expect("legacy notes should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    notes_state.path = Some(notes_path);
    world.insert_resource(notes_state);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let restored_agent = *world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .first()
        .expect("restored agent should exist");
    assert_eq!(
        world
            .resource::<crate::conversations::AgentTaskStore>()
            .text(restored_agent),
        Some("- [ ] legacy task")
    );
    let notes_state = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(notes_state.note_text("neozeus-session-a"), None);
    assert_eq!(notes_state.dirty_since_secs, Some(0.0));
}


/// Verifies that startup restore plus owned-tmux sync rebinds recovered tmux children under the
/// restored agent using the stable persisted agent uid.
#[test]
fn startup_restore_rebinds_owned_tmux_children_under_agent() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-owned-tmux-bind");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    world.insert_resource(crate::hud::AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();
    assert_eq!(
        world
            .resource::<crate::terminals::OwnedTmuxSessionStore>()
            .sessions
            .len(),
        1,
        "startup should hydrate owned tmux state before the first interactive poke"
    );
    run_synced_hud_view_models(&mut world);

    let rows = &world.resource::<crate::hud::AgentListView>().rows;
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[0].key, crate::hud::AgentListRowKey::Agent(_)));
    assert!(matches!(
        rows[1].key,
        crate::hud::AgentListRowKey::OwnedTmux(_)
    ));
}


/// Verifies that startup reaps an unpersisted disconnected persistent session instead of importing
/// it back as a dead agent, then falls back to spawning a fresh initial terminal.
#[test]
fn startup_restore_rebinds_multiple_owned_tmux_children_under_correct_agents_and_orphans() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored-a"),
    );
    client.set_session_runtime(
        "neozeus-session-b",
        crate::terminals::TerminalRuntimeState::running("restored-b"),
    );
    let dir = temp_dir("neozeus-startup-owned-tmux-multi-bind");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-uid-2\"\nsession_name=\"neozeus-session-b\"\nlabel=\"BETA\"\nkind=\"terminal\"\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");
    client.owned_tmux_sessions.lock().unwrap().extend([
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-2".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-2".into(),
            display_name: "TEST".into(),
            cwd: "/tmp/a-2".into(),
            attached: false,
            created_unix: 2,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/a-1".into(),
            attached: false,
            created_unix: 1,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-3".into(),
            owner_agent_uid: "agent-uid-2".into(),
            tmux_name: "neozeus-tmux-3".into(),
            display_name: "BETA BUILD".into(),
            cwd: "/tmp/b-1".into(),
            attached: true,
            created_unix: 3,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-4".into(),
            owner_agent_uid: "missing-agent".into(),
            tmux_name: "neozeus-tmux-4".into(),
            display_name: "LOST".into(),
            cwd: "/tmp/orphan".into(),
            attached: false,
            created_unix: 4,
        },
    ]);

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    world.insert_resource(crate::hud::AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();
    assert_eq!(
        world
            .resource::<crate::terminals::OwnedTmuxSessionStore>()
            .sessions
            .len(),
        4,
        "startup should hydrate every owned tmux child before the first interactive poke"
    );
    run_synced_hud_view_models(&mut world);

    let rows = &world.resource::<crate::hud::AgentListView>().rows;
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].label, "ALPHA");
    assert_eq!(rows[1].label, "BUILD");
    assert_eq!(rows[2].label, "TEST");
    assert_eq!(rows[3].label, "BETA");
    assert_eq!(rows[4].label, "BETA BUILD");
    assert!(matches!(
        rows[1].key,
        crate::hud::AgentListRowKey::OwnedTmux(_)
    ));
    assert!(matches!(
        rows[2].key,
        crate::hud::AgentListRowKey::OwnedTmux(_)
    ));
    assert!(matches!(
        rows[4].key,
        crate::hud::AgentListRowKey::OwnedTmux(_)
    ));
}


#[test]
fn startup_reaps_unpersisted_disconnected_session_instead_of_restoring_it() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-dead",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let manager = world.resource::<crate::terminals::TerminalManager>();
    assert_eq!(manager.terminal_ids().len(), 1);
    let session_names = manager
        .terminal_ids()
        .iter()
        .map(|terminal_id| {
            manager
                .get(*terminal_id)
                .expect("terminal should exist")
                .session_name
                .clone()
        })
        .collect::<Vec<_>>();
    assert!(!session_names
        .iter()
        .any(|name| name == "neozeus-session-dead"));
    assert!(!client
        .sessions
        .lock()
        .unwrap()
        .contains("neozeus-session-dead"));
    assert_eq!(client.created_sessions.lock().unwrap().len(), 1);
}


#[test]
fn startup_respawns_claude_agent_from_recovery_spec_when_daemon_is_empty() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-claude-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "claude --resume claude-session-1"
    ));
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog
        .order
        .first()
        .expect("restored Claude agent should exist");
    assert_eq!(catalog.uid(restored_agent), Some("agent-uid-1"));
    assert!(matches!(
        catalog.recovery_spec(restored_agent),
        Some(crate::agents::AgentRecoverySpec::Claude { session_id, cwd, .. })
            if session_id == "claude-session-1" && cwd == "/tmp/demo"
    ));
}


#[test]
fn startup_restore_reattaches_live_agent_by_runtime_session_name_when_available() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-live-claude",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-runtime-session-reattach");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-live-claude\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/claude-demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert!(client.sent_commands.lock().unwrap().is_empty());
    let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
    let agent_id = runtime_index
        .agent_for_session("neozeus-live-claude")
        .expect("live session should be reattached");
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.uid(agent_id), Some("agent-uid-1"));
    assert!(matches!(
        catalog.recovery_spec(agent_id),
        Some(crate::agents::AgentRecoverySpec::Claude { session_id, cwd, .. })
            if session_id == "claude-session-1" && cwd == "/tmp/claude-demo"
    ));
}


#[test]
fn startup_restore_preserves_paused_agents_and_projects_them_to_display_bottom() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-live-alpha",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    client.set_session_runtime(
        "neozeus-live-beta",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-restore-paused-agents");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-live-alpha\"\nlabel=\"ALPHA\"\nkind=\"terminal\"\npaused=1\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-uid-2\"\nruntime_session_name=\"neozeus-live-beta\"\nlabel=\"BETA\"\nkind=\"terminal\"\npaused=0\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let display_order = catalog.display_order();
    assert_eq!(display_order.len(), 2);
    assert_eq!(catalog.label(display_order[0]), Some("BETA"));
    assert_eq!(catalog.label(display_order[1]), Some("ALPHA"));
    let alpha_id = catalog
        .find_by_uid("agent-uid-1")
        .expect("alpha should exist");
    assert!(catalog.is_paused(alpha_id));
}


#[test]
fn startup_restore_falls_back_to_recovery_when_runtime_session_is_gone() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-stale-runtime-fallback");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-stale\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "claude --resume claude-session-1"
    ));
}


#[test]
fn startup_restore_reattaches_live_agent_and_respawns_missing_recoverable_agent() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-live-claude",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    client.session_metadata.lock().unwrap().insert(
        "neozeus-live-claude".into(),
        crate::shared::daemon_wire::DaemonSessionMetadata {
            agent_uid: Some("agent-uid-1".into()),
            agent_label: Some("ALPHA".into()),
            agent_kind: Some(crate::shared::daemon_wire::DaemonAgentKind::Claude),
        },
    );
    let dir = temp_dir("neozeus-startup-mixed-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/claude-demo\"\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-uid-2\"\nlabel=\"BETA\"\nkind=\"codex\"\nrecovery_mode=\"codex\"\nrecovery_session_id=\"codex-thread-1\"\nrecovery_cwd=\"/tmp/codex-demo\"\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert_eq!(client.created_sessions.lock().unwrap().len(), 1);
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "codex resume codex-thread-1 -C /tmp/codex-demo"
    ));
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.order.len(), 2);
    assert!(catalog
        .order
        .iter()
        .copied()
        .any(|agent_id| catalog.uid(agent_id) == Some("agent-uid-1")));
    assert!(catalog
        .order
        .iter()
        .copied()
        .any(|agent_id| catalog.uid(agent_id) == Some("agent-uid-2")));
}


#[test]
fn startup_imported_live_sessions_serialize_runtime_binding_truthfully_on_save() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-live-terminal",
        crate::terminals::TerminalRuntimeState::running("imported"),
    );
    client.session_metadata.lock().unwrap().insert(
        "neozeus-session-live-terminal".into(),
        crate::shared::daemon_wire::DaemonSessionMetadata {
            agent_uid: Some("agent-live".into()),
            agent_label: Some("LIVE".into()),
            agent_kind: Some(crate::shared::daemon_wire::DaemonAgentKind::Terminal),
        },
    );
    let dir = temp_dir("neozeus-startup-import-save-runtime-binding");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path.clone()),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::app::save_app_state_if_dirty)
        .unwrap();

    let persisted = crate::app::load_persisted_app_state_from(&app_state_path);
    let imported = persisted
        .agents
        .iter()
        .find(|record| record.agent_uid.as_deref() == Some("agent-live"))
        .expect("imported live session should persist");
    assert_eq!(imported.label.as_deref(), Some("LIVE"));
    assert_eq!(
        imported.kind,
        crate::shared::app_state_file::PersistedAgentKind::Terminal
    );
    assert_eq!(
        imported.runtime_session_name.as_deref(),
        Some("neozeus-session-live-terminal")
    );
    assert!(imported.recovery.is_none());
}


#[test]
fn startup_restore_reattaches_live_only_agent_when_runtime_is_still_live() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-live-only",
        crate::terminals::TerminalRuntimeState::running("live-only"),
    );
    let dir = temp_dir("neozeus-startup-live-only-reattach");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-live\"\nruntime_session_name=\"neozeus-session-live-only\"\nlabel=\"LIVE\"\nkind=\"terminal\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert!(client.sent_commands.lock().unwrap().is_empty());
    let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
    let agent_id = runtime_index
        .agent_for_session("neozeus-session-live-only")
        .expect("live-only agent should be reattached");
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.uid(agent_id), Some("agent-live"));
    assert_eq!(catalog.label(agent_id), Some("LIVE"));
    assert_eq!(
        catalog.kind(agent_id),
        Some(crate::agents::AgentKind::Terminal)
    );
    assert!(catalog.recovery_spec(agent_id).is_none());
}


#[test]
fn startup_restore_reports_invalid_pi_recovery_without_blocking_other_agents() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-invalid-pi-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        format!(
            "neozeus state version 4\n[agent]\nagent_uid=\"agent-bad\"\nlabel=\"BROKEN-PI\"\nkind=\"pi\"\nrecovery_mode=\"pi\"\nrecovery_session_path=\"{}\"\nrecovery_cwd=\"/tmp/missing\"\norder_index=0\n[/agent]\n[agent]\nagent_uid=\"agent-good\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=1\n[/agent]\n",
            dir.join("missing-session.jsonl").display()
        ),
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "claude --resume claude-session-1"
    ));
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.order.len(), 1);
    assert_eq!(catalog.uid(catalog.order[0]), Some("agent-good"));
    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Automatic recovery completed: 1 restored, 1 failed")
    );
    assert!(status
        .details
        .iter()
        .any(|line| line.contains("BROKEN-PI") && line.contains("Pi session path missing")));
}


#[test]
fn startup_restore_reports_invalid_claude_recovery_and_does_not_spawn_default_agent() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-invalid-claude-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert!(world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .is_empty());
    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Automatic recovery completed: 0 restored, 1 failed")
    );
    assert!(status
        .details
        .iter()
        .any(|line| line.contains("ALPHA") && line.contains("Claude session id missing")));
}


#[test]
fn startup_restore_reports_invalid_codex_recovery_and_does_not_spawn_default_agent() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-invalid-codex-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-2\"\nlabel=\"BETA\"\nkind=\"codex\"\nrecovery_mode=\"codex\"\nrecovery_session_id=\"codex-thread-1\"\nrecovery_cwd=\"\"\norder_index=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert!(world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .is_empty());
    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Automatic recovery completed: 0 restored, 1 failed")
    );
    assert!(status
        .details
        .iter()
        .any(|line| line.contains("BETA") && line.contains("Codex cwd missing")));
}


#[test]
fn startup_respawns_codex_agent_from_recovery_spec_when_daemon_is_empty() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-codex-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-2\"\nlabel=\"BETA\"\nkind=\"codex\"\nrecovery_mode=\"codex\"\nrecovery_session_id=\"codex-thread-1\"\nrecovery_cwd=\"/tmp/codex-demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/codex-demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "codex resume codex-thread-1 -C /tmp/codex-demo"
    ));
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog
        .order
        .first()
        .expect("restored Codex agent should exist");
    assert_eq!(catalog.uid(restored_agent), Some("agent-uid-2"));
    assert!(matches!(
        catalog.recovery_spec(restored_agent),
        Some(crate::agents::AgentRecoverySpec::Codex { session_id, cwd, .. })
            if session_id == "codex-thread-1" && cwd == "/tmp/codex-demo"
    ));
}

