use crate::{
    app::schedule::configure_app_schedule,
    app_config::GPU_NOT_FOUND_PANIC_FRAGMENT,
    hud::{
        AgentDirectory, AgentListBloomBlurMaterial, HudBloomSettings, HudCompositeCaptureConfig,
        HudIntent, HudModuleRequest, HudOffscreenCompositor, HudPersistenceState,
        HudTextureCaptureConfig, HudWidgetBloom, TerminalFocusRequest, TerminalLifecycleRequest,
        TerminalSendRequest, TerminalTaskRequest, TerminalViewRequest, TerminalVisibilityRequest,
        TerminalVisibilityState, WindowCaptureConfig,
    },
    terminals::{
        TerminalDaemonClientResource, TerminalFontState, TerminalGlyphCache, TerminalManager,
        TerminalPointerState, TerminalPresentationStore, TerminalRuntimeSpawner,
        TerminalSessionPersistenceState, TerminalViewState,
    },
    verification::AutoVerifyConfig,
};
use bevy::{
    prelude::*,
    render::{settings::WgpuSettings, RenderPlugin},
    sprite_render::Material2dPlugin,
    window::{MonitorSelection, WindowMode},
    winit::{EventLoopProxyWrapper, WinitSettings},
};
use bevy_vello::VelloPlugin;
use std::{any::Any, env, sync::Arc};

pub(crate) fn build_app() -> Result<App, String> {
    let mut app = App::new();
    let previous_hook = Arc::new(std::panic::take_hook());
    let forwarding_hook = previous_hook.clone();

    // Bevy/WGPU still reports missing-adapter startup failure through a panic path in practice.
    // We intercept only that specific startup panic so the user gets a clear error message while
    // leaving all other panics untouched.
    std::panic::set_hook(Box::new(move |info| {
        if panic_payload_message(info.payload()).is_some_and(is_missing_gpu_panic) {
            return;
        }
        (*forwarding_hook)(info);
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| configure_app(&mut app)));

    let restore_hook = previous_hook.clone();
    std::panic::set_hook(Box::new(move |info| (*restore_hook)(info)));

    match result {
        Ok(Ok(())) => Ok(app),
        Ok(Err(error)) => Err(error),
        Err(payload) => {
            if let Some(error) = format_startup_panic(payload.as_ref()) {
                Err(error)
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

pub(crate) fn resolve_window_mode(raw: Option<&str>) -> WindowMode {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("windowed") => WindowMode::Windowed,
        _ => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
    }
}

pub(crate) fn resolve_window_scale_factor(raw: Option<&str>) -> Option<f32> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
}

pub(crate) fn resolve_force_fallback_adapter(raw: Option<&str>) -> bool {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(true)
}

fn primary_window_config() -> Window {
    let resolution = if let Some(scale_factor) =
        resolve_window_scale_factor(env::var("NEOZEUS_WINDOW_SCALE_FACTOR").ok().as_deref())
    {
        bevy::window::WindowResolution::new(1400, 900).with_scale_factor_override(scale_factor)
    } else {
        (1400, 900).into()
    };
    Window {
        title: env::var("NEOZEUS_WINDOW_TITLE").unwrap_or_else(|_| "neozeus".to_owned()),
        name: Some(env::var("NEOZEUS_APP_ID").unwrap_or_else(|_| "neozeus".to_owned())),
        mode: resolve_window_mode(env::var("NEOZEUS_WINDOW_MODE").ok().as_deref()),
        resolution,
        ..default()
    }
}

fn configure_app(app: &mut App) -> Result<(), String> {
    let hud_capture = HudTextureCaptureConfig::from_env();
    let hud_composite_capture = HudCompositeCaptureConfig::from_env();
    let window_capture = WindowCaptureConfig::from_env();
    let auto_verify = AutoVerifyConfig::from_env();
    let winit_settings = if hud_capture.is_some()
        || hud_composite_capture.is_some()
        || window_capture.is_some()
        || auto_verify.is_some()
    {
        WinitSettings::game()
    } else {
        WinitSettings::desktop_app()
    };

    app.add_plugins(
        DefaultPlugins
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    force_fallback_adapter: resolve_force_fallback_adapter(
                        env::var("NEOZEUS_FORCE_FALLBACK_ADAPTER").ok().as_deref(),
                    ),
                    ..default()
                }
                .into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(primary_window_config()),
                ..default()
            }),
    )
    .add_plugins((
        VelloPlugin::default(),
        Material2dPlugin::<AgentListBloomBlurMaterial>::default(),
    ));

    let event_loop_proxy = {
        let proxy = app.world().resource::<EventLoopProxyWrapper>();
        (**proxy).clone()
    };
    let daemon_client = TerminalDaemonClientResource::system()?;

    if let Some(hud_capture) = hud_capture {
        app.insert_resource(hud_capture);
    }
    if let Some(hud_composite_capture) = hud_composite_capture {
        app.insert_resource(hud_composite_capture);
    }
    if let Some(window_capture) = window_capture {
        app.insert_resource(window_capture);
    }
    if let Some(config) = auto_verify {
        app.insert_resource(config);
    }

    app.insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
        .insert_resource(winit_settings)
        .insert_resource(TerminalManager::default())
        .insert_resource(crate::terminals::TerminalFocusState::default())
        .insert_resource(TerminalPresentationStore::default())
        .insert_resource(daemon_client.clone())
        .insert_resource(TerminalRuntimeSpawner::new(event_loop_proxy, daemon_client))
        .insert_resource(TerminalSessionPersistenceState::default())
        .insert_resource(crate::terminals::TerminalNotesState::default())
        .insert_resource(TerminalFontState::default())
        .insert_resource(TerminalViewState::default())
        .insert_resource(TerminalPointerState::default())
        .insert_resource(TerminalGlyphCache::default())
        .insert_resource(crate::terminals::TerminalTextRenderer::default())
        .insert_resource(crate::hud::HudLayoutState::default())
        .insert_resource(crate::hud::HudModalState::default())
        .insert_resource(crate::hud::HudInputCaptureState::default())
        .insert_resource(HudPersistenceState::default())
        .insert_resource(HudOffscreenCompositor::default())
        .insert_resource(HudBloomSettings::default())
        .insert_resource(HudWidgetBloom::default())
        .insert_resource(AgentDirectory::default())
        .insert_resource(TerminalVisibilityState::default())
        .add_message::<HudIntent>()
        .add_message::<TerminalFocusRequest>()
        .add_message::<TerminalVisibilityRequest>()
        .add_message::<HudModuleRequest>()
        .add_message::<TerminalViewRequest>()
        .add_message::<TerminalSendRequest>()
        .add_message::<TerminalLifecycleRequest>()
        .add_message::<TerminalTaskRequest>();

    configure_app_schedule(app);
    Ok(())
}

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

fn is_missing_gpu_panic(message: &str) -> bool {
    message.contains(GPU_NOT_FOUND_PANIC_FRAGMENT)
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> Option<&str> {
    if let Some(message) = payload.downcast_ref::<String>() {
        Some(message.as_str())
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        Some(*message)
    } else {
        None
    }
}
