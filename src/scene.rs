use crate::{
    app_config::{EVA_DEMO_Z, GPU_NOT_FOUND_PANIC_FRAGMENT},
    hud::{sync_eva_vector_demo, ui_overlay, EvaVectorDemoMarker, EvaVectorDemoState},
    input::{drag_terminal_view, forward_keyboard_input, zoom_terminal_view},
    terminals::{
        append_debug_log, configure_terminal_fonts, spawn_terminal_instance,
        sync_terminal_hud_surface, sync_terminal_panel_frames, sync_terminal_presentations,
        sync_terminal_texture, TerminalCameraMarker, TerminalFontState, TerminalGlyphCache,
        TerminalHudSurfaceMarker, TerminalManager, TerminalPanel, TerminalPointerState,
        TerminalPresentation, TerminalPresentationStore, TerminalRuntimeSpawner, TerminalViewState,
    },
    verification::{start_auto_verify_dispatcher, AutoVerifyConfig},
};
use bevy::{
    camera::visibility::NoFrustumCulling,
    prelude::*,
    render::{settings::WgpuSettings, RenderPlugin},
    window::RequestRedraw,
    winit::{EventLoopProxyWrapper, WinitSettings},
};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use bevy_vello::{
    prelude::{VelloScene2d, VelloView},
    VelloPlugin,
};
use std::{any::Any, env, sync::Arc};

pub(crate) fn build_app() -> Result<App, String> {
    let mut app = App::new();
    let previous_hook = Arc::new(std::panic::take_hook());
    let forwarding_hook = previous_hook.clone();

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
        Ok(()) => Ok(app),
        Err(payload) => {
            if let Some(error) = format_startup_panic(payload.as_ref()) {
                Err(error)
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum NeoZeusSet {
    PollTerminal,
    RasterTerminal,
    UiInput,
    PresentTerminal,
    Redraw,
}

fn configure_app(app: &mut App) {
    app.add_plugins(
        DefaultPlugins
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    force_fallback_adapter: true,
                    ..default()
                }
                .into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: env::var("NEOZEUS_WINDOW_TITLE")
                        .unwrap_or_else(|_| "neozeus".to_owned()),
                    resolution: (1400, 900).into(),
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins((EguiPlugin::default(), VelloPlugin::default()));

    let event_loop_proxy = {
        let proxy = app.world().resource::<EventLoopProxyWrapper>();
        (**proxy).clone()
    };

    app.insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
        .insert_resource(WinitSettings::desktop_app())
        .insert_resource(TerminalManager::default())
        .insert_resource(TerminalPresentationStore::default())
        .insert_resource(TerminalRuntimeSpawner::new(event_loop_proxy))
        .insert_resource(TerminalFontState::default())
        .insert_resource(TerminalViewState::default())
        .insert_resource(TerminalPointerState::default())
        .insert_resource(TerminalGlyphCache::default())
        .insert_resource(crate::terminals::TerminalTextRenderer::default())
        .insert_resource(EvaVectorDemoState::default())
        .configure_sets(
            Update,
            (
                NeoZeusSet::PollTerminal,
                NeoZeusSet::RasterTerminal,
                NeoZeusSet::UiInput,
                NeoZeusSet::PresentTerminal,
                NeoZeusSet::Redraw,
            )
                .chain(),
        )
        .add_systems(Startup, setup_scene)
        .add_systems(
            Update,
            crate::terminals::poll_terminal_snapshots.in_set(NeoZeusSet::PollTerminal),
        )
        .add_systems(
            Update,
            (configure_terminal_fonts, sync_terminal_texture).in_set(NeoZeusSet::RasterTerminal),
        )
        .add_systems(
            Update,
            (
                drag_terminal_view,
                zoom_terminal_view,
                forward_keyboard_input,
            )
                .in_set(NeoZeusSet::UiInput),
        )
        .add_systems(
            Update,
            (
                sync_terminal_presentations,
                sync_terminal_panel_frames,
                sync_terminal_hud_surface,
            )
                .in_set(NeoZeusSet::PresentTerminal),
        )
        .add_systems(
            Update,
            (sync_eva_vector_demo, request_redraw_while_visuals_active).in_set(NeoZeusSet::Redraw),
        )
        .add_systems(EguiPrimaryContextPass, ui_overlay);

    if let Some(config) = AutoVerifyConfig::from_env() {
        app.insert_resource(config);
    }
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

const PRESENTATION_EPSILON: f32 = 0.25;
const ALPHA_EPSILON: f32 = 0.01;
const Z_EPSILON: f32 = 0.01;

pub(crate) fn should_request_visual_redraw(
    terminal_work_pending: bool,
    presentation_animating: bool,
    eva_demo_enabled: bool,
) -> bool {
    terminal_work_pending || presentation_animating || eva_demo_enabled
}

fn request_redraw_while_visuals_active(
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    eva_demo: Res<EvaVectorDemoState>,
    panels: Query<&TerminalPresentation, With<TerminalPanel>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let terminal_work_pending = terminal_manager.terminals().iter().any(|(id, terminal)| {
        terminal.pending_damage.is_some()
            || presentation_store
                .get(*id)
                .map(|presented| terminal.surface_revision != presented.uploaded_revision)
                .unwrap_or(false)
    });
    let presentation_animating = panels.iter().any(|presentation| {
        presentation
            .current_position
            .distance(presentation.target_position)
            > PRESENTATION_EPSILON
            || presentation.current_size.distance(presentation.target_size) > PRESENTATION_EPSILON
            || (presentation.current_alpha - presentation.target_alpha).abs() > ALPHA_EPSILON
            || (presentation.current_z - presentation.target_z).abs() > Z_EPSILON
    });

    if should_request_visual_redraw(
        terminal_work_pending,
        presentation_animating,
        eva_demo.enabled,
    ) {
        redraws.write(RequestRedraw);
    }
}

fn setup_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    auto_verify: Option<Res<AutoVerifyConfig>>,
) {
    commands.spawn((Camera2d, VelloView, TerminalCameraMarker));

    commands.spawn((
        Sprite {
            color: Color::srgba(0.03, 0.03, 0.04, 0.0),
            custom_size: Some(Vec2::ONE),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 2.9),
        Visibility::Hidden,
        TerminalHudSurfaceMarker,
    ));

    commands.spawn((
        VelloScene2d::default(),
        Transform::from_xyz(0.0, 0.0, EVA_DEMO_Z),
        NoFrustumCulling,
        EvaVectorDemoMarker,
    ));

    let Ok(primary_id) = spawn_terminal_instance(
        &mut commands,
        &mut images,
        &mut terminal_manager,
        &mut presentation_store,
        &runtime_spawner,
    ) else {
        append_debug_log("failed to spawn primary terminal");
        return;
    };
    if let Some(config) = auto_verify {
        if let Some(bridge) = terminal_manager
            .terminals()
            .get(&primary_id)
            .map(|terminal| terminal.bridge.clone())
        {
            start_auto_verify_dispatcher(bridge, runtime_spawner.notifier(), config.clone());
        }
    }
}
