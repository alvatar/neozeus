use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::AppStatePersistenceState,
    app_config::{
        load_neozeus_config, resolve_app_id, resolve_window_title, NeoZeusConfig,
        GPU_NOT_FOUND_PANIC_FRAGMENT,
    },
    conversations::{
        AgentTaskStore, ConversationPersistenceState, ConversationStore, MessageTransportAdapter,
    },
    hud::{
        AgentListBloomBlurMaterial, AgentListView, ComposerView, ConversationListView,
        HudBloomSettings, HudCompositeCaptureConfig, HudOffscreenCompositor, HudPersistenceState,
        HudTextureCaptureConfig, HudWidgetBloom, TerminalVisibilityState, ThreadView,
        WindowCaptureConfig,
    },
    shared::linux_display::LinuxDisplayEnvironment,
    terminals::{
        TerminalFontState, TerminalGlyphCache, TerminalManager, TerminalPointerState,
        TerminalPresentationStore, TerminalRuntimeSpawner, TerminalViewState,
    },
    usage::{default_usage_persistence_state, UsageSnapshot},
    verification::{
        AutoVerifyConfig, VerificationCaptureBarrierState, VerificationScenarioConfig,
        VerificationTerminalSurfaceOverrides,
    },
};

use super::{
    commands::AppCommand,
    output::{
        sync_final_frame_output_target, AppOutputConfig, FinalFrameCaptureConfig,
        FinalFrameOutputState, OutputMode,
    },
    schedule::{configure_app_schedule, NeoZeusSet},
    session::AppSessionState,
};
use bevy::{
    app::ScheduleRunnerPlugin,
    prelude::*,
    render::{settings::WgpuSettings, RenderPlugin},
    sprite_render::Material2dPlugin,
    window::{MonitorSelection, PrimaryWindow, WindowMode},
    winit::{EventLoopProxyWrapper, WinitPlugin, WinitSettings},
};
use bevy_egui::EguiClipboard;
use bevy_vello::VelloPlugin;
use std::{any::Any, env, sync::Arc, time::Duration};

/// Builds the top-level Bevy [`App`] and turns the ugly startup failure modes into normal
/// `Result` errors.
///
/// The important detail here is that Bevy/WGPU can still report "no usable GPU" as a panic during
/// initialization instead of as a recoverable error. [`with_startup_panic_policy`] isolates that
/// process-global hook swap to the smallest possible wrapper around [`configure_app`] and restores
/// the previous hook before returning.
///
/// If the panic payload matches the missing-GPU case, the payload is converted into a user-facing
/// error string; any other panic is rethrown unchanged so genuine bugs do not get silently hidden.
pub(crate) fn build_app() -> Result<App, String> {
    let mut app = App::new();
    with_startup_panic_policy(|| {
        configure_app(&mut app)?;
        Ok(app)
    })
}

/// Runs startup-only initialization behind the narrow missing-GPU panic policy.
///
/// This is still a process-global hook swap because Rust panic hooks are global, but the mutation
/// is now scoped to one helper that does nothing except: install the temporary forwarding hook,
/// execute the startup closure under `catch_unwind`, and restore the prior hook before returning.
///
/// Safety/policy notes:
/// - only the known missing-GPU startup panic is suppressed from the temporary hook path
/// - non-matching panics are rethrown unchanged after the hook is restored
/// - ordinary `Result` errors from the closure pass through untouched
fn with_startup_panic_policy<T>(run: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let previous_hook = Arc::new(std::panic::take_hook());
    let forwarding_hook = previous_hook.clone();
    std::panic::set_hook(Box::new(move |info| {
        if panic_payload_message(info.payload()).is_some_and(is_missing_gpu_panic) {
            return;
        }
        (*forwarding_hook)(info);
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));

    let restore_hook = previous_hook.clone();
    std::panic::set_hook(Box::new(move |info| (*restore_hook)(info)));

    match result {
        Ok(result) => result,
        Err(payload) => {
            if let Some(error) = format_startup_panic(payload.as_ref()) {
                Err(error)
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

/// Resolves the window mode from an optional raw configuration string.
///
/// Only `windowed` is treated as an explicit override. Any missing, empty, or unrecognized value
/// falls back to borderless fullscreen on the current monitor, which is the project's default
/// startup mode.
pub(crate) fn resolve_window_mode(raw: Option<&str>) -> WindowMode {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("windowed") => WindowMode::Windowed,
        _ => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
    }
}

/// Parses an optional scale-factor override for deterministic window sizing.
///
/// The value is trimmed, parsed as `f32`, and rejected unless it is both finite and strictly
/// positive. Invalid input is treated as "no override" instead of as an error so callers can keep
/// using environment variables without hard-failing startup.
pub(crate) fn resolve_window_scale_factor(raw: Option<&str>) -> Option<f32> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
}

/// Parses the explicit `force_fallback_adapter` flag from a raw string.
///
/// The function accepts the usual truthy spellings (`1`, `true`, `yes`, `on`) after trimming and
/// lowercasing the input. Missing or empty input defaults to `false`: software fallback is an
/// explicit compatibility knob, not the normal desktop startup path.
pub(crate) fn resolve_force_fallback_adapter(raw: Option<&str>) -> bool {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Chooses the effective fallback-adapter policy for the current output mode.
///
/// An explicit environment/config value always wins and is delegated to
/// [`resolve_force_fallback_adapter`]. When nothing is specified, both desktop and offscreen mode
/// now default to `false`; forcing a software adapter is opt-in so the normal app path can use the
/// real GPU when one is available.
pub(crate) fn resolve_force_fallback_adapter_for(
    raw: Option<&str>,
    _output_mode: OutputMode,
) -> bool {
    resolve_force_fallback_adapter(raw)
}

/// Chooses whether Bevy's pipelined rendering plugin should be disabled for the current runtime.
///
/// On this host the desktop Wayland + GL/EGL render path is unstable and panics from render-worker
/// threads during surface creation/preparation. Disabling pipelined rendering keeps the render work
/// on the main path and avoids that EGL threading failure. Offscreen mode keeps the normal plugin
/// stack. `NEOZEUS_DISABLE_PIPELINED_RENDERING` can explicitly override the auto policy.
pub(crate) fn resolve_disable_pipelined_rendering_for(
    raw: Option<&str>,
    output_mode: OutputMode,
    session_type: Option<&str>,
    wayland_display: Option<&str>,
) -> bool {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    if let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) {
        return matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
    }
    !output_mode.is_offscreen()
        && LinuxDisplayEnvironment::new(session_type, wayland_display, None).prefers_wayland()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinuxWindowBackend {
    Auto,
    Wayland,
    X11,
}

/// Resolves linux window backend.
pub(crate) fn resolve_linux_window_backend(raw: Option<&str>) -> LinuxWindowBackend {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("wayland") => LinuxWindowBackend::Wayland,
        Some(value) if value.eq_ignore_ascii_case("x11") => LinuxWindowBackend::X11,
        _ => LinuxWindowBackend::Auto,
    }
}

/// Returns whether force x11 backend.
pub(crate) fn should_force_x11_backend(
    output_mode: OutputMode,
    backend: LinuxWindowBackend,
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> bool {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if output_mode.is_offscreen() {
        return false;
    }
    match backend {
        LinuxWindowBackend::Wayland => false,
        LinuxWindowBackend::X11 => {
            LinuxDisplayEnvironment::new(session_type, wayland_display, display)
                .x11_display_present()
        }
        LinuxWindowBackend::Auto => {
            let env = LinuxDisplayEnvironment::new(session_type, wayland_display, display);
            env.x11_display_present() && env.prefers_wayland()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LinuxWindowBackendEnvMutation {
    set_winit_unix_backend: Option<&'static str>,
    set_xdg_session_type: Option<&'static str>,
    clear_wayland_display: bool,
}

fn linux_window_backend_env_mutation(
    output_mode: OutputMode,
    backend: LinuxWindowBackend,
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> Option<LinuxWindowBackendEnvMutation> {
    should_force_x11_backend(output_mode, backend, session_type, wayland_display, display)
        .then_some(LinuxWindowBackendEnvMutation {
            set_winit_unix_backend: Some("x11"),
            set_xdg_session_type: Some("x11"),
            clear_wayland_display: true,
        })
}

fn apply_linux_window_backend_env_mutation(mutation: LinuxWindowBackendEnvMutation) {
    if let Some(value) = mutation.set_winit_unix_backend {
        env::set_var("WINIT_UNIX_BACKEND", value);
    }
    if let Some(value) = mutation.set_xdg_session_type {
        env::set_var("XDG_SESSION_TYPE", value);
    }
    if mutation.clear_wayland_display {
        env::remove_var("WAYLAND_DISPLAY");
    }
}

/// Handles apply linux window backend policy.
fn apply_linux_window_backend_policy(output_mode: OutputMode) -> bool {
    let mutation = linux_window_backend_env_mutation(
        output_mode,
        resolve_linux_window_backend(env::var("NEOZEUS_LINUX_WINDOW_BACKEND").ok().as_deref()),
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
        env::var("WAYLAND_DISPLAY").ok().as_deref(),
        env::var("DISPLAY").ok().as_deref(),
    );
    let Some(mutation) = mutation else {
        return false;
    };

    apply_linux_window_backend_env_mutation(mutation);
    true
}

/// Handles normalize output for x11 fallback.
pub(crate) fn normalize_output_for_x11_fallback(
    mut output: AppOutputConfig,
    forced_x11: bool,
    explicit_scale_factor: Option<&str>,
) -> AppOutputConfig {
    if forced_x11
        && output.scale_factor_override.is_none()
        && explicit_scale_factor
            .map(str::trim)
            .is_none_or(|value| value.is_empty())
    {
        output.scale_factor_override = Some(1.0);
    }
    output
}

#[allow(
    dead_code,
    reason = "compatibility wrapper retained for scene facade tests"
)]
/// Convenience wrapper that builds the primary-window plugin config with default application
/// metadata.
///
/// This exists mainly for tests and compatibility call sites that do not care about a loaded
/// [`NeoZeusConfig`]. The real logic lives in [`primary_window_plugin_config_for_with_config`].
pub(crate) fn primary_window_plugin_config_for(output: &AppOutputConfig) -> Option<Window> {
    primary_window_plugin_config_for_with_config(output, &NeoZeusConfig::default())
}

/// Builds the `WindowPlugin` configuration for the primary window when a real OS window should
/// exist.
///
/// Offscreen mode deliberately returns `None` here so Bevy does not create a native window through
/// the plugin path. In normal mode it delegates to [`primary_window_config_for_with_config`] so the
/// exact same window settings are used by both plugin-driven and manually spawned primary windows.
fn primary_window_plugin_config_for_with_config(
    output: &AppOutputConfig,
    config: &NeoZeusConfig,
) -> Option<Window> {
    (!output.mode.is_offscreen()).then(|| primary_window_config_for_with_config(output, config))
}

/// Returns whether the app should run through Bevy's schedule runner instead of the Winit event
/// loop.
///
/// At the moment this is equivalent to "offscreen output mode is enabled". Keeping the predicate in
/// one place avoids scattering that policy through bootstrap code and tests.
pub(crate) fn uses_headless_runner(output: &AppOutputConfig) -> bool {
    output.mode.is_offscreen()
}

#[allow(
    dead_code,
    reason = "compatibility wrapper retained for scene facade tests"
)]
/// Convenience wrapper that builds a concrete [`Window`] using the default application config.
///
/// This is mostly used by tests; production code typically calls
/// [`primary_window_config_for_with_config`] after loading config from disk.
pub(crate) fn primary_window_config_for(output: &AppOutputConfig) -> Window {
    primary_window_config_for_with_config(output, &NeoZeusConfig::default())
}

/// Builds the concrete [`Window`] settings used for the primary render target.
///
/// The function folds together output-mode policy, application metadata, and the optional scale
/// factor override. Offscreen mode forces a hidden windowed configuration because the app still
/// needs a logical primary window resource even when no native window should be shown.
pub(crate) fn primary_window_config_for_with_config(
    output: &AppOutputConfig,
    config: &NeoZeusConfig,
) -> Window {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let resolution = if let Some(scale_factor) = output.scale_factor_override {
        bevy::window::WindowResolution::new(output.width, output.height)
            .with_scale_factor_override(scale_factor)
    } else {
        (output.width, output.height).into()
    };
    Window {
        title: resolve_window_title(config),
        name: Some(resolve_app_id(config)),
        mode: if output.mode.is_offscreen() {
            WindowMode::Windowed
        } else {
            resolve_window_mode(env::var("NEOZEUS_WINDOW_MODE").ok().as_deref())
        },
        visible: !output.mode.is_offscreen(),
        decorations: !output.mode.is_offscreen(),
        focused: !output.mode.is_offscreen(),
        resolution,
        ..default()
    }
}

/// Fills the freshly created [`App`] with plugins, resources, messages, and schedule wiring.
///
/// The work is done in four stages: load configuration, decide runtime/output mode, install the
/// correct Bevy plugin stack for that mode, and finally seed all domain resources used by the HUD,
/// terminal runtime, persistence, and verification paths. The function is intentionally long because
/// it is the single composition root for the application.
///
/// A notable quirk is the offscreen path: Winit is disabled, a synthetic primary window is spawned
/// manually, and the runtime spawner is built without an OS event-loop proxy so verification can run
/// headlessly.
fn configure_app(app: &mut App) -> Result<(), String> {
    let neozeus_config = load_neozeus_config()?;
    let initial_output = AppOutputConfig::from_env();
    let forced_x11 = apply_linux_window_backend_policy(initial_output.mode);
    let output = normalize_output_for_x11_fallback(
        initial_output,
        forced_x11,
        env::var("NEOZEUS_WINDOW_SCALE_FACTOR").ok().as_deref(),
    );
    let hud_capture = HudTextureCaptureConfig::from_env();
    let hud_composite_capture = HudCompositeCaptureConfig::from_env();
    let window_capture = WindowCaptureConfig::from_env();
    let final_frame_capture = FinalFrameCaptureConfig::from_env();
    let auto_verify = AutoVerifyConfig::from_env();
    let verification_scenario = VerificationScenarioConfig::from_env();
    let winit_settings = if output.mode.is_offscreen()
        || hud_capture.is_some()
        || hud_composite_capture.is_some()
        || window_capture.is_some()
        || final_frame_capture.is_some()
        || auto_verify.is_some()
        || verification_scenario.is_some()
    {
        WinitSettings::game()
    } else {
        WinitSettings::desktop_app()
    };

    // Build the common plugin stack first, then choose between a normal Winit-driven app and a
    // headless schedule runner depending on the selected output mode.
    let disable_pipelined_rendering = resolve_disable_pipelined_rendering_for(
        env::var("NEOZEUS_DISABLE_PIPELINED_RENDERING")
            .ok()
            .as_deref(),
        output.mode,
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
        env::var("WAYLAND_DISPLAY").ok().as_deref(),
    );
    let mut default_plugins = DefaultPlugins
        .build()
        .disable::<bevy::gizmos::GizmoPlugin>()
        .disable::<bevy::gizmos_render::GizmoRenderPlugin>()
        .set(RenderPlugin {
            render_creation: WgpuSettings {
                force_fallback_adapter: resolve_force_fallback_adapter_for(
                    env::var("NEOZEUS_FORCE_FALLBACK_ADAPTER").ok().as_deref(),
                    output.mode,
                ),
                ..default()
            }
            .into(),
            ..default()
        })
        .set(WindowPlugin {
            primary_window: primary_window_plugin_config_for_with_config(&output, &neozeus_config),
            ..default()
        });
    if disable_pipelined_rendering {
        default_plugins = default_plugins
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    }
    if uses_headless_runner(&output) {
        app.add_plugins(default_plugins.disable::<WinitPlugin>())
            .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
                1.0 / 60.0,
            )));
    } else {
        app.add_plugins(default_plugins);
    }
    app.add_plugins((
        VelloPlugin::default(),
        Material2dPlugin::<AgentListBloomBlurMaterial>::default(),
    ));

    if uses_headless_runner(&output) {
        // Without Winit there is no automatically spawned `PrimaryWindow`, but the rest of the app
        // still expects that resource graph to exist for sizing and render-target routing.
        app.world_mut().spawn((
            primary_window_config_for_with_config(&output, &neozeus_config),
            PrimaryWindow,
        ));
    }

    let runtime_spawner = if uses_headless_runner(&output) {
        TerminalRuntimeSpawner::pending_headless()
    } else {
        let proxy = app.world().resource::<EventLoopProxyWrapper>();
        TerminalRuntimeSpawner::pending((**proxy).clone())
    };

    if let Some(hud_capture) = hud_capture {
        app.insert_resource(hud_capture);
    }
    if let Some(hud_composite_capture) = hud_composite_capture {
        app.insert_resource(hud_composite_capture);
    }
    if let Some(window_capture) = window_capture {
        app.insert_resource(window_capture);
    }
    if let Some(final_frame_capture) = final_frame_capture {
        app.insert_resource(final_frame_capture);
    }
    if let Some(config) = auto_verify {
        app.insert_resource(config);
    }
    if let Some(config) = verification_scenario {
        app.insert_resource(config);
    }

    // Seed every long-lived domain resource up front. Most systems assume these resources exist and
    // mutate them directly instead of lazily creating state during update.
    app.insert_resource(output)
        .insert_resource(FinalFrameOutputState::default())
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
        .insert_resource(winit_settings)
        .insert_resource(TerminalManager::default())
        .insert_resource(crate::terminals::TerminalFocusState::default())
        .insert_resource(TerminalPresentationStore::default())
        .insert_resource(runtime_spawner)
        .insert_resource(AppStatePersistenceState::default())
        .insert_resource(crate::terminals::TerminalNotesState::default())
        .insert_resource(TerminalFontState::default())
        .insert_resource(TerminalViewState::default())
        .insert_resource(TerminalPointerState::default())
        .insert_resource(TerminalGlyphCache::default())
        .insert_resource(crate::terminals::TerminalTextRenderer::default())
        .insert_resource(crate::hud::HudLayoutState::default())
        .insert_resource(crate::hud::AgentListUiState::default())
        .insert_resource(crate::hud::ConversationListUiState::default())
        .insert_resource(crate::hud::InfoBarUiState)
        .insert_resource(crate::hud::ThreadPaneUiState)
        .insert_resource(crate::hud::HudInputCaptureState::default())
        .insert_resource(HudPersistenceState::default())
        .insert_resource(crate::hud::HudLayerRegistry::default())
        .insert_resource(HudOffscreenCompositor::default())
        .insert_resource(HudBloomSettings::default())
        .insert_resource(HudWidgetBloom::default())
        .insert_resource(AgentCatalog::default())
        .insert_resource(AgentRuntimeIndex::default())
        .insert_resource(crate::agents::AgentStatusStore::default())
        .insert_resource(crate::aegis::AegisPolicyStore::default())
        .insert_resource(crate::aegis::AegisRuntimeStore::default())
        .insert_resource(crate::aegis::AegisStatusTracker::default())
        .insert_resource(crate::visual_contract::VisualContractState::default())
        .insert_resource(VerificationTerminalSurfaceOverrides::default())
        .insert_resource(VerificationCaptureBarrierState::default())
        .insert_resource(AppSessionState::default())
        .insert_resource(crate::hud::AgentListSelection::default())
        .insert_resource(EguiClipboard::default())
        .insert_resource(ConversationStore::default())
        .insert_resource(ConversationPersistenceState::default())
        .insert_resource(AgentTaskStore::default())
        .insert_resource(MessageTransportAdapter)
        .insert_resource(AgentListView::default())
        .insert_resource(ConversationListView::default())
        .insert_resource(ThreadView::default())
        .insert_resource(ComposerView::default())
        .insert_resource(crate::hud::InfoBarView::default())
        .insert_resource(UsageSnapshot::default())
        .insert_resource(default_usage_persistence_state())
        .insert_resource(TerminalVisibilityState::default())
        .insert_resource(crate::terminals::OwnedTmuxSessionStore::default())
        .insert_resource(crate::terminals::LiveSessionMetricsStore::default())
        .insert_resource(crate::terminals::ActiveTerminalContentState::default())
        .insert_resource(crate::terminals::ActiveTerminalContentSyncState::default())
        .insert_resource(crate::text_selection::TerminalTextSelectionState::default())
        .insert_resource(crate::text_selection::AgentListTextSelectionState::default())
        .insert_resource(crate::text_selection::PrimarySelectionState::default())
        .insert_resource(crate::text_selection::PrimarySelectionOwnerState::default())
        .insert_resource(crate::startup::DaemonConnectionState::default())
        .insert_resource(crate::startup::StartupConnectState::default())
        .add_message::<AppCommand>();

    configure_app_schedule(app);
    app.add_systems(
        Update,
        sync_final_frame_output_target
            .before(NeoZeusSet::PresentTerminal)
            .before(NeoZeusSet::HudRender),
    );
    Ok(())
}

/// Converts a captured startup panic into a user-facing error string when the panic matches the
/// known missing-GPU failure.
///
/// The function intentionally only recognizes one startup class: the Bevy/WGPU "no adapter found"
/// panic. Everything else returns `None` so the caller can resume unwinding and preserve normal bug
/// visibility.
pub(crate) fn format_startup_panic(payload: &(dyn Any + Send)) -> Option<String> {
    let message = panic_payload_message(payload)?;
    if !is_missing_gpu_panic(message) {
        return None;
    }

    Some(
        "neozeus failed to start: Bevy/WGPU could not find a usable graphics adapter. \
This environment is either headless or missing graphics/software-rendering drivers. \
Run it in a graphical session with a working GPU, or install a software renderer such as Mesa/llvmpipe."
            .to_owned(),
    )
}

/// Detects whether a panic message is the specific WGPU "no graphics adapter" failure we know how
/// to recover from.
///
/// Matching is deliberately done by substring against the stable fragment exposed by
/// [`GPU_NOT_FOUND_PANIC_FRAGMENT`] instead of by exact text so minor surrounding wording changes do
/// not break the recovery path.
fn is_missing_gpu_panic(message: &str) -> bool {
    message.contains(GPU_NOT_FOUND_PANIC_FRAGMENT)
}

/// Extracts a string view from the two panic payload shapes this code cares about.
///
/// Rust panics commonly carry either an owned `String` or a `&'static str`. Any other payload type
/// is ignored and returns `None`, which is fine here because the startup recovery path only needs to
/// inspect text panics emitted by upstream libraries.
fn panic_payload_message(payload: &(dyn Any + Send)) -> Option<&str> {
    if let Some(message) = payload.downcast_ref::<String>() {
        Some(message.as_str())
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        Some(*message)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        format_startup_panic, linux_window_backend_env_mutation, with_startup_panic_policy,
        LinuxWindowBackend, LinuxWindowBackendEnvMutation,
    };
    use crate::app::output::OutputMode;

    #[test]
    fn startup_panic_policy_converts_known_missing_gpu_panics() {
        let error = with_startup_panic_policy::<()>(|| {
            panic!(
                "{}: renderer init failed",
                crate::app_config::GPU_NOT_FOUND_PANIC_FRAGMENT
            )
        })
        .expect_err("missing gpu panic should be converted into an error");

        assert!(error.contains("could not find a usable graphics adapter"));
    }

    #[test]
    fn startup_panic_policy_rethrows_unrelated_panics() {
        let panic = std::panic::catch_unwind(|| {
            let _ = with_startup_panic_policy::<()>(|| panic!("bug"));
        })
        .expect_err("non-startup panic should keep unwinding");

        let message = panic
            .downcast_ref::<&'static str>()
            .copied()
            .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
        assert_eq!(message, Some("bug"));
    }

    #[test]
    fn linux_window_backend_env_mutation_only_exists_for_forced_x11_cases() {
        let forced = linux_window_backend_env_mutation(
            OutputMode::Desktop,
            LinuxWindowBackend::Auto,
            Some("wayland"),
            Some("wayland-1"),
            Some(":0"),
        );
        assert_eq!(
            forced,
            Some(LinuxWindowBackendEnvMutation {
                set_winit_unix_backend: Some("x11"),
                set_xdg_session_type: Some("x11"),
                clear_wayland_display: true,
            })
        );

        assert_eq!(
            linux_window_backend_env_mutation(
                OutputMode::Desktop,
                LinuxWindowBackend::Wayland,
                Some("wayland"),
                Some("wayland-1"),
                Some(":0"),
            ),
            None
        );
        assert_eq!(
            linux_window_backend_env_mutation(
                OutputMode::OffscreenVerify,
                LinuxWindowBackend::Auto,
                Some("wayland"),
                Some("wayland-1"),
                Some(":0"),
            ),
            None
        );
    }

    #[test]
    fn startup_panic_formatter_ignores_unrelated_messages() {
        assert!(format_startup_panic(&"some other panic").is_none());
    }
}
