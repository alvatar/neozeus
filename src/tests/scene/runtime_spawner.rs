//! Test submodule: `runtime_spawner` — extracted from the centralized test bucket.

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

/// Verifies that pending runtime spawner becomes ready when daemon is installed.
#[test]
fn pending_runtime_spawner_becomes_ready_when_daemon_is_installed() {
    let spawner = TerminalRuntimeSpawner::pending_headless();
    assert!(!spawner.is_ready());
    spawner.install_daemon(fake_daemon_resource(Arc::new(FakeDaemonClient::default())));
    assert!(spawner.is_ready());
}


/// Verifies that the startup overlay keeps the user-facing title at `Connecting` through the
/// restore phase.
#[test]
fn startup_connect_title_stays_connecting_during_restore() {
    let state = DaemonConnectionState::with_phase_for_test(
        StartupConnectPhase::Restoring,
        "Restoring sessions…",
    );
    assert_eq!(state.title(), "Connecting");
}


/// Verifies that setup installs the deferred background-connect receiver when the runtime is not yet ready.
#[test]
fn setup_scene_starts_background_connect_when_runtime_is_pending() {
    let mut world = World::default();
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
    world.insert_resource(TerminalRuntimeSpawner::pending_headless());
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(DaemonConnectionState::default());
    world.insert_resource(StartupConnectState::default());
    world.insert_resource(Time::<()>::default());
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let startup_connect = world.resource::<StartupConnectState>();
    assert_eq!(
        world.resource::<DaemonConnectionState>().phase(),
        StartupConnectPhase::Connecting
    );
    assert!(startup_connect.has_receiver());
}


/// Verifies that startup connecting advances to restoring when background connect completes.
#[test]
fn setup_scene_auto_verify_uses_shared_spawn_attach_flow_and_isolates_verifier_terminal() {
    let client = Arc::new(FakeDaemonClient::default());
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
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(DaemonConnectionState::default());
    world.insert_resource(StartupConnectState::default());
    world.insert_resource(Time::<()>::default());
    world.insert_resource(crate::verification::AutoVerifyConfig {
        command: "echo verify".to_owned(),
        delay_ms: 0,
    });
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let terminal_ids = world
        .resource::<crate::terminals::TerminalManager>()
        .terminal_ids()
        .to_vec();
    assert_eq!(terminal_ids.len(), 1);
    let terminal_id = terminal_ids[0];
    assert_eq!(
        world
            .resource::<crate::app::AppSessionState>()
            .focus_intent
            .target,
        crate::app::FocusIntentTarget::Terminal(terminal_id)
    );
    assert_eq!(
        world
            .resource::<crate::hud::TerminalVisibilityState>()
            .policy,
        TerminalVisibilityPolicy::Isolate(terminal_id)
    );
    let presentation_store = world.resource::<crate::terminals::TerminalPresentationStore>();
    assert!(presentation_store.is_startup_bootstrap_pending(terminal_id));
    assert_eq!(
        world.resource::<DaemonConnectionState>().phase(),
        StartupConnectPhase::SettlingVisuals
    );
    let created = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert!(created[0]
        .0
        .starts_with(crate::terminals::VERIFIER_SESSION_PREFIX));
    std::thread::sleep(std::time::Duration::from_millis(20));
    let sent = client.sent_commands.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0],
        (
            created[0].0.clone(),
            crate::terminals::TerminalCommand::SendCommand("echo verify".to_owned()),
        )
    );
}


#[test]
fn startup_connecting_advances_to_restoring_when_background_connect_completes() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let spawner = TerminalRuntimeSpawner::pending_headless();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Ok(fake_daemon_resource(Arc::new(
        FakeDaemonClient::default(),
    ))))
    .expect("test daemon resource should send");

    let mut world = World::default();
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
    world.insert_resource(spawner.clone());
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(DaemonConnectionState::default());
    world.insert_resource(StartupConnectState::with_receiver_for_test(rx));
    world.insert_resource(Time::<()>::default());
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(advance_startup_connecting).unwrap();

    assert!(spawner.is_ready());
    assert_eq!(
        world.resource::<DaemonConnectionState>().phase(),
        StartupConnectPhase::Restoring
    );
}

