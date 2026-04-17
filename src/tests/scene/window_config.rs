//! Test submodule: `window_config` — extracted from the centralized test bucket.

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

/// Verifies the default window-mode policy is borderless fullscreen unless overridden.
#[test]
fn defaults_window_mode_to_borderless_fullscreen() {
    assert_eq!(
        resolve_window_mode(None),
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    );
    assert_eq!(
        resolve_window_mode(Some("fullscreen")),
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    );
}


/// Verifies that explicit `windowed` configuration overrides the fullscreen default, including with
/// surrounding whitespace.
#[test]
fn allows_explicit_windowed_override() {
    assert_eq!(resolve_window_mode(Some("windowed")), WindowMode::Windowed);
    assert_eq!(
        resolve_window_mode(Some(" WINDOWED ")),
        WindowMode::Windowed
    );
}


/// Verifies environment-style parsing of output mode and output dimension overrides.
#[test]
fn parses_output_mode_and_dimensions() {
    assert_eq!(resolve_output_mode(None), OutputMode::Desktop);
    assert_eq!(resolve_output_mode(Some("")), OutputMode::Desktop);
    assert_eq!(
        resolve_output_mode(Some("offscreen")),
        OutputMode::OffscreenVerify
    );
    assert_eq!(
        resolve_output_mode(Some("offscreen-verify")),
        OutputMode::OffscreenVerify
    );
    assert_eq!(resolve_output_dimension(None, 42), 42);
    assert_eq!(resolve_output_dimension(Some(""), 42), 42);
    assert_eq!(resolve_output_dimension(Some("1600"), 42), 1600);
    assert_eq!(resolve_output_dimension(Some("0"), 42), 42);
    assert_eq!(resolve_output_dimension(Some("abc"), 42), 42);
}


/// Verifies the synthetic offscreen window is hidden, undecorated, unfocused, and forced into
/// windowed mode.
#[test]
fn offscreen_synthetic_window_config_is_hidden_and_windowed() {
    let window = primary_window_config_for(&AppOutputConfig {
        mode: OutputMode::OffscreenVerify,
        width: 1600,
        height: 1000,
        scale_factor_override: Some(1.5),
    });
    assert!(!window.visible);
    assert!(!window.decorations);
    assert!(!window.focused);
    assert_eq!(window.mode, WindowMode::Windowed);
    assert_eq!(window.physical_width(), 1600);
    assert_eq!(window.physical_height(), 1000);
    assert_eq!(window.resolution.scale_factor_override(), Some(1.5));
}


/// Verifies that offscreen mode switches to the headless runner and suppresses the normal primary
/// window plugin.
#[test]
fn offscreen_mode_uses_headless_runner_and_no_os_primary_window() {
    let output = AppOutputConfig {
        mode: OutputMode::OffscreenVerify,
        width: 1600,
        height: 1000,
        scale_factor_override: None,
    };
    assert!(uses_headless_runner(&output));
    assert!(primary_window_plugin_config_for(&output).is_none());
}


/// Verifies parsing/validation of the optional window scale-factor override.
#[test]
fn parses_optional_window_scale_factor_override() {
    assert_eq!(resolve_window_scale_factor(None), None);
    assert_eq!(resolve_window_scale_factor(Some("")), None);
    assert_eq!(resolve_window_scale_factor(Some("  ")), None);
    assert_eq!(resolve_window_scale_factor(Some("1.0")), Some(1.0));
    assert_eq!(resolve_window_scale_factor(Some(" 2.5 ")), Some(2.5));
    assert_eq!(resolve_window_scale_factor(Some("0")), None);
    assert_eq!(resolve_window_scale_factor(Some("-1")), None);
    assert_eq!(resolve_window_scale_factor(Some("abc")), None);
}


/// Verifies parsing of the force-fallback-adapter override and the opt-in default.
#[test]
fn parses_force_fallback_adapter_override() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    assert!(!resolve_force_fallback_adapter(None));
    assert!(!resolve_force_fallback_adapter(Some("")));
    assert!(resolve_force_fallback_adapter(Some("true")));
    assert!(resolve_force_fallback_adapter(Some("1")));
    assert!(!resolve_force_fallback_adapter(Some("false")));
    assert!(!resolve_force_fallback_adapter(Some("0")));
    assert!(!resolve_force_fallback_adapter_for(
        None,
        OutputMode::Desktop
    ));
    assert!(!resolve_force_fallback_adapter_for(
        None,
        OutputMode::OffscreenVerify
    ));
    assert!(resolve_force_fallback_adapter_for(
        Some("yes"),
        OutputMode::Desktop
    ));
}


/// Verifies the auto-disable policy for pipelined rendering on desktop Wayland.
#[test]
fn resolves_disable_pipelined_rendering_for_wayland_desktop_only() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    assert!(resolve_disable_pipelined_rendering_for(
        None,
        OutputMode::Desktop,
        Some("wayland"),
        Some("wayland-1")
    ));
    assert!(resolve_disable_pipelined_rendering_for(
        None,
        OutputMode::Desktop,
        None,
        Some("wayland-1")
    ));
    assert!(!resolve_disable_pipelined_rendering_for(
        None,
        OutputMode::Desktop,
        Some("x11"),
        None
    ));
    assert!(!resolve_disable_pipelined_rendering_for(
        None,
        OutputMode::OffscreenVerify,
        Some("wayland"),
        Some("wayland-1")
    ));
    assert!(resolve_disable_pipelined_rendering_for(
        Some("true"),
        OutputMode::Desktop,
        Some("x11"),
        None
    ));
    assert!(!resolve_disable_pipelined_rendering_for(
        Some("false"),
        OutputMode::Desktop,
        Some("wayland"),
        Some("wayland-1")
    ));
}


/// Verifies that resolves linux window backend policy.
#[test]
fn resolves_linux_window_backend_policy() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    assert_eq!(resolve_linux_window_backend(None), LinuxWindowBackend::Auto);
    assert_eq!(
        resolve_linux_window_backend(Some("x11")),
        LinuxWindowBackend::X11
    );
    assert_eq!(
        resolve_linux_window_backend(Some("wayland")),
        LinuxWindowBackend::Wayland
    );
    assert!(should_force_x11_backend(
        OutputMode::Desktop,
        LinuxWindowBackend::Auto,
        Some("wayland"),
        Some("wayland-1"),
        Some(":0")
    ));
    assert!(should_force_x11_backend(
        OutputMode::Desktop,
        LinuxWindowBackend::X11,
        Some("wayland"),
        Some("wayland-1"),
        Some(":0")
    ));
    assert!(!should_force_x11_backend(
        OutputMode::Desktop,
        LinuxWindowBackend::Wayland,
        Some("wayland"),
        Some("wayland-1"),
        Some(":0")
    ));
    assert!(!should_force_x11_backend(
        OutputMode::Desktop,
        LinuxWindowBackend::Auto,
        Some("wayland"),
        Some("wayland-1"),
        None
    ));
    assert!(!should_force_x11_backend(
        OutputMode::OffscreenVerify,
        LinuxWindowBackend::Auto,
        Some("wayland"),
        Some("wayland-1"),
        Some(":0")
    ));

    let normalized = normalize_output_for_x11_fallback(
        AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1400,
            height: 900,
            scale_factor_override: None,
        },
        true,
        None,
    );
    assert_eq!(normalized.scale_factor_override, Some(1.0));

    let preserved = normalize_output_for_x11_fallback(
        AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1400,
            height: 900,
            scale_factor_override: Some(1.5),
        },
        true,
        None,
    );
    assert_eq!(preserved.scale_factor_override, Some(1.5));

    let explicit_env = normalize_output_for_x11_fallback(
        AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1400,
            height: 900,
            scale_factor_override: None,
        },
        true,
        Some("2.0"),
    );
    assert_eq!(explicit_env.scale_factor_override, None);
}

