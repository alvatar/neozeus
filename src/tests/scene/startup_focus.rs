//! Test submodule: `startup_focus` — extracted from the centralized test bucket.

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

/// Verifies the startup focus-selection precedence: persisted focus first, then other restored
/// sessions, then imported live sessions.
#[test]
fn startup_focus_prefers_persisted_focus_then_restored_then_imported() {
    assert_eq!(
        choose_startup_focus_session_name(Some("session-b"), &["session-a", "session-b"], &[]),
        Some("session-b")
    );
    assert_eq!(
        choose_startup_focus_session_name(None, &["session-a", "session-b"], &["session-c"]),
        Some("session-a")
    );
    assert_eq!(
        choose_startup_focus_session_name(None, &[], &["session-c", "session-d"]),
        Some("session-c")
    );
    assert_eq!(choose_startup_focus_session_name(None, &[], &[]), None);
}


/// Verifies that startup visibility policy isolates a chosen focus target and otherwise falls back
/// to `ShowAll`.
#[test]
fn startup_visibility_isolate_focused_terminal() {
    assert_eq!(
        startup_visibility_policy_for_focus(Some(TerminalId(7))),
        TerminalVisibilityPolicy::Isolate(TerminalId(7))
    );
    assert_eq!(
        startup_visibility_policy_for_focus(None),
        TerminalVisibilityPolicy::ShowAll
    );
}


/// Verifies that startup focus restoration skips a persisted `last_focused` session if that session
/// comes back disconnected and instead focuses a live restored session.
#[test]
fn startup_focus_skips_disconnected_restored_session() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let dir = temp_dir("neozeus-startup-focus-running-session");
    let sessions_path = dir.join("terminals.v1");
    let persisted = crate::terminals::PersistedTerminalSessions {
        sessions: vec![
            crate::terminals::TerminalSessionRecord {
                session_name: "neozeus-session-dead".to_owned(),
                label: Some("dead".to_owned()),
                creation_index: 0,
                last_focused: true,
            },
            crate::terminals::TerminalSessionRecord {
                session_name: "neozeus-session-live".to_owned(),
                label: Some("live".to_owned()),
                creation_index: 1,
                last_focused: false,
            },
        ],
    };
    std::fs::write(
        &sessions_path,
        crate::terminals::serialize_persisted_terminal_sessions(&persisted),
    )
    .expect("persisted sessions should write");

    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-dead",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );
    client.set_session_runtime(
        "neozeus-session-live",
        crate::terminals::TerminalRuntimeState::running("live session"),
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
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(sessions_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let focus = world.resource::<crate::terminals::TerminalFocusState>();
    let manager = world.resource::<crate::terminals::TerminalManager>();
    let active_id = focus
        .active_id()
        .expect("startup should focus a live terminal");
    let active = manager
        .get(active_id)
        .expect("active terminal should exist");
    assert_eq!(active.session_name, "neozeus-session-live");
    assert_eq!(manager.terminal_ids().len(), 2);
    assert_eq!(client.sessions.lock().unwrap().len(), 2);
}


/// Verifies that when only disconnected sessions are restored, startup leaves them visible but
/// unfocused instead of isolating a dead session.
#[test]
fn startup_leaves_only_disconnected_sessions_visible_and_unfocused() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let dir = temp_dir("neozeus-startup-disconnected-visible");
    let sessions_path = dir.join("terminals.v1");
    let persisted = crate::terminals::PersistedTerminalSessions {
        sessions: vec![crate::terminals::TerminalSessionRecord {
            session_name: "neozeus-session-dead".to_owned(),
            label: Some("dead".to_owned()),
            creation_index: 0,
            last_focused: true,
        }],
    };
    std::fs::write(
        &sessions_path,
        crate::terminals::serialize_persisted_terminal_sessions(&persisted),
    )
    .expect("persisted sessions should write");

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
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(sessions_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let focus = world.resource::<crate::terminals::TerminalFocusState>();
    let manager = world.resource::<crate::terminals::TerminalManager>();
    assert_eq!(focus.active_id(), None);
    assert_eq!(manager.terminal_ids().len(), 1);
    let only_terminal = manager
        .get(manager.terminal_ids()[0])
        .expect("restored terminal should exist");
    assert_eq!(only_terminal.session_name, "neozeus-session-dead");
    assert_eq!(
        world
            .resource::<crate::hud::TerminalVisibilityState>()
            .policy,
        TerminalVisibilityPolicy::ShowAll
    );
    assert_eq!(client.sessions.lock().unwrap().len(), 1);
}

