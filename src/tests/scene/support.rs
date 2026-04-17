//! Shared test-only helpers for this area.
//!
//! Holds the imports, constants, and builders used by per-topic test submodules.
//! Private items are promoted to `pub(super)` so sibling submodules can reach them.

#![allow(unused_imports, dead_code)]

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


pub(super) fn run_synced_hud_view_models(world: &mut World) {
    if !world.contains_resource::<crate::visual_contract::VisualContractState>() {
        world.insert_resource(crate::visual_contract::VisualContractState::default());
    }
    if !world.contains_resource::<crate::terminals::LiveSessionMetricsStore>() {
        world.insert_resource(crate::terminals::LiveSessionMetricsStore::default());
    }
    world
        .run_system_once(crate::visual_contract::sync_visual_contract_state)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();
}

