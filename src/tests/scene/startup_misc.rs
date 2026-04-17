//! Test submodule: `startup_misc` — extracted from the centralized test bucket.

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

#[test]
fn startup_recovery_status_includes_skipped_live_only_agents_in_title_and_details() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-skipped-live-only-status");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-recoverable\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-live-only\"\nruntime_session_name=\"neozeus-session-missing\"\nlabel=\"BETA\"\nkind=\"terminal\"\norder_index=1\nfocused=0\n[/agent]\n",
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

    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Automatic recovery completed: 1 restored, 0 failed, 1 skipped")
    );
    assert!(status.details.iter().any(|line| {
        line == "startup skipped live-only agent BETA: runtime session unavailable"
    }));
}


/// Verifies the cold-start fallback path that spawns a brand-new initial terminal when restore/import
/// finds nothing usable.
#[test]
fn startup_spawns_initial_terminal_when_no_sessions_exist() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-no-sessions");
    let app_state_path = dir.join("empty-state.v1");
    std::fs::write(&app_state_path, "neozeus state version 4\n").expect("app state should write");
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

    let manager = world.resource::<crate::terminals::TerminalManager>();
    let terminal_ids = manager.terminal_ids();
    assert_eq!(terminal_ids.len(), 1);
    let terminal_id = terminal_ids[0];
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(terminal_id)
    );
    assert_eq!(
        world
            .resource::<crate::hud::TerminalVisibilityState>()
            .policy,
        TerminalVisibilityPolicy::Isolate(terminal_id)
    );
    assert_eq!(client.sessions.lock().unwrap().len(), 1);
    assert!(world
        .resource::<crate::app::AppSessionState>()
        .recovery_status
        .title
        .is_none());
}


/// Verifies that a known missing-GPU startup panic is converted into a friendly user-facing error,
/// while unrelated panics are ignored.
#[test]
fn formats_missing_gpu_startup_panics_as_user_facing_errors() {
    let error = format_startup_panic(&"Unable to find a GPU! renderer init failed")
        .expect("missing gpu panic should be formatted");
    assert!(error.contains("could not find a usable graphics adapter"));
    assert!(format_startup_panic(&"some other panic").is_none());
}
