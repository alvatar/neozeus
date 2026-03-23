use crate::{
    app_config::GPU_NOT_FOUND_PANIC_FRAGMENT,
    hud::{
        animate_hud_modules, apply_hud_module_requests, apply_terminal_focus_requests,
        apply_terminal_lifecycle_requests, apply_terminal_send_requests,
        apply_terminal_task_requests, apply_terminal_view_requests, apply_visibility_requests,
        dispatch_hud_intents, handle_hud_module_shortcuts, handle_hud_pointer_input,
        hud_needs_redraw, render_hud_scene, save_hud_layout_if_dirty, setup_hud,
        setup_hud_widget_bloom, sync_hud_offscreen_compositor, sync_hud_widget_bloom,
        sync_structural_hud_layout, AgentDirectory, AgentListBloomBlurMaterial, HudBloomSettings,
        HudIntent, HudModuleRequest, HudOffscreenCompositor, HudPersistenceState, HudState,
        HudWidgetBloom, TerminalFocusRequest, TerminalLifecycleRequest, TerminalSendRequest,
        TerminalTaskRequest, TerminalViewRequest, TerminalVisibilityPolicy,
        TerminalVisibilityRequest, TerminalVisibilityState,
    },
    input::{
        drag_terminal_view, focus_terminal_on_panel_click, handle_global_terminal_spawn_shortcut,
        handle_terminal_direct_input_keyboard, handle_terminal_lifecycle_shortcuts,
        handle_terminal_message_box_keyboard, hide_terminal_on_background_click,
        zoom_terminal_view,
    },
    terminals::{
        append_debug_log, configure_terminal_fonts, load_persisted_terminal_sessions_from,
        mark_terminal_sessions_dirty, reconcile_terminal_sessions, resolve_terminal_notes_path,
        resolve_terminal_sessions_path, save_terminal_notes_if_dirty,
        save_terminal_sessions_if_dirty, spawn_attached_terminal_with_presentation,
        sync_active_terminal_dimensions, sync_terminal_hud_surface, sync_terminal_panel_frames,
        sync_terminal_presentations, sync_terminal_texture, TerminalCameraMarker,
        TerminalDaemonClientResource, TerminalFontState, TerminalGlyphCache,
        TerminalHudSurfaceMarker, TerminalManager, TerminalPanel, TerminalPointerState,
        TerminalPresentation, TerminalPresentationStore, TerminalRuntimeSpawner,
        TerminalSessionPersistenceState, TerminalViewState, VERIFIER_SESSION_PREFIX,
    },
    verification::{start_auto_verify_dispatcher, AutoVerifyConfig},
};
use bevy::{
    camera::visibility::RenderLayers,
    ecs::system::SystemParam,
    prelude::*,
    render::{settings::WgpuSettings, RenderPlugin},
    sprite_render::Material2dPlugin,
    window::{MonitorSelection, RequestRedraw, WindowMode},
    winit::{EventLoopProxyWrapper, WinitSettings},
};
use bevy_vello::{prelude::VelloView, VelloPlugin};
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

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum NeoZeusSet {
    PollTerminal,
    RasterTerminal,
    UiInput,
    HudInput,
    HudIntentDispatch,
    HudCommands,
    PresentTerminal,
    HudAnimation,
    HudRender,
    Redraw,
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
                primary_window: Some(primary_window_config()),
                ..default()
            }),
    )
    .add_plugins((
        VelloPlugin::default(),
        Material2dPlugin::<AgentListBloomBlurMaterial>::default(),
        Material2dPlugin::<crate::hud::HudCompositeMaterial>::default(),
    ));

    let event_loop_proxy = {
        let proxy = app.world().resource::<EventLoopProxyWrapper>();
        (**proxy).clone()
    };
    let daemon_client = TerminalDaemonClientResource::system()?;

    app.insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
        .insert_resource(WinitSettings::desktop_app())
        .insert_resource(TerminalManager::default())
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
        .insert_resource(HudState::default())
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
        .add_message::<TerminalTaskRequest>()
        .configure_sets(
            Update,
            NeoZeusSet::PollTerminal.before(NeoZeusSet::RasterTerminal),
        )
        .configure_sets(
            Update,
            NeoZeusSet::RasterTerminal.before(NeoZeusSet::PresentTerminal),
        )
        .configure_sets(
            Update,
            NeoZeusSet::UiInput
                .before(NeoZeusSet::RasterTerminal)
                .before(NeoZeusSet::PresentTerminal)
                .before(NeoZeusSet::HudIntentDispatch),
        )
        .configure_sets(
            Update,
            NeoZeusSet::HudInput.before(NeoZeusSet::HudIntentDispatch),
        )
        .configure_sets(
            Update,
            NeoZeusSet::HudIntentDispatch.before(NeoZeusSet::HudCommands),
        )
        .configure_sets(
            Update,
            NeoZeusSet::HudCommands
                .before(NeoZeusSet::RasterTerminal)
                .before(NeoZeusSet::PresentTerminal)
                .before(NeoZeusSet::HudAnimation),
        )
        .configure_sets(
            Update,
            NeoZeusSet::PresentTerminal.before(NeoZeusSet::HudAnimation),
        )
        .configure_sets(
            Update,
            NeoZeusSet::HudAnimation.before(NeoZeusSet::HudRender),
        )
        .configure_sets(Update, NeoZeusSet::HudRender.before(NeoZeusSet::Redraw))
        .add_systems(
            Startup,
            (setup_scene, setup_hud, setup_hud_widget_bloom).chain(),
        )
        .add_systems(PostStartup, sync_hud_offscreen_compositor)
        .add_systems(
            Update,
            sync_structural_hud_layout
                .before(NeoZeusSet::UiInput)
                .before(NeoZeusSet::HudInput)
                .before(NeoZeusSet::PresentTerminal),
        )
        .add_systems(
            Update,
            crate::terminals::poll_terminal_snapshots.in_set(NeoZeusSet::PollTerminal),
        )
        .add_systems(
            Update,
            (
                sync_active_terminal_dimensions,
                configure_terminal_fonts,
                sync_terminal_texture,
            )
                .chain()
                .in_set(NeoZeusSet::RasterTerminal),
        )
        .add_systems(
            Update,
            (
                handle_global_terminal_spawn_shortcut,
                handle_terminal_lifecycle_shortcuts,
                handle_terminal_direct_input_keyboard,
                handle_terminal_message_box_keyboard,
                focus_terminal_on_panel_click,
                hide_terminal_on_background_click,
                drag_terminal_view,
                zoom_terminal_view,
            )
                .in_set(NeoZeusSet::UiInput),
        )
        .add_systems(
            Update,
            (handle_hud_pointer_input, handle_hud_module_shortcuts).in_set(NeoZeusSet::HudInput),
        )
        .add_systems(
            Update,
            dispatch_hud_intents.in_set(NeoZeusSet::HudIntentDispatch),
        )
        .add_systems(
            Update,
            (
                apply_terminal_focus_requests,
                apply_visibility_requests,
                apply_hud_module_requests,
                apply_terminal_view_requests,
                apply_terminal_send_requests,
                apply_terminal_task_requests,
                apply_terminal_lifecycle_requests,
            )
                .in_set(NeoZeusSet::HudCommands),
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
            (
                animate_hud_modules,
                save_hud_layout_if_dirty,
                save_terminal_notes_if_dirty,
                save_terminal_sessions_if_dirty,
            )
                .in_set(NeoZeusSet::HudAnimation),
        )
        .add_systems(
            Update,
            (
                render_hud_scene,
                sync_hud_offscreen_compositor,
                sync_hud_widget_bloom,
            )
                .chain()
                .in_set(NeoZeusSet::HudRender),
        )
        .add_systems(
            Update,
            request_redraw_while_visuals_active.in_set(NeoZeusSet::Redraw),
        );

    if let Some(config) = AutoVerifyConfig::from_env() {
        app.insert_resource(config);
    }

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

const PRESENTATION_EPSILON: f32 = 0.25;
const ALPHA_EPSILON: f32 = 0.01;
const Z_EPSILON: f32 = 0.01;

#[derive(SystemParam)]
struct SceneSetupContext<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    terminal_manager: ResMut<'w, TerminalManager>,
    presentation_store: ResMut<'w, TerminalPresentationStore>,
    agent_directory: ResMut<'w, AgentDirectory>,
    runtime_spawner: Res<'w, TerminalRuntimeSpawner>,
    session_persistence: ResMut<'w, TerminalSessionPersistenceState>,
    notes_state: ResMut<'w, crate::terminals::TerminalNotesState>,
    visibility_state: ResMut<'w, TerminalVisibilityState>,
}

pub(crate) fn should_request_visual_redraw(
    terminal_work_pending: bool,
    presentation_animating: bool,
    hud_visuals_active: bool,
) -> bool {
    terminal_work_pending || presentation_animating || hud_visuals_active
}

pub(crate) fn choose_startup_focus_session_name<'a>(
    restored_focus_session: Option<&'a str>,
    restored_session_names: &[&'a str],
    imported_session_names: &[&'a str],
) -> Option<&'a str> {
    restored_focus_session
        .or_else(|| restored_session_names.first().copied())
        .or_else(|| imported_session_names.first().copied())
}

pub(crate) fn startup_visibility_policy_for_focus(
    focused_id: Option<crate::terminals::TerminalId>,
) -> TerminalVisibilityPolicy {
    focused_id
        .map(TerminalVisibilityPolicy::Isolate)
        .unwrap_or(TerminalVisibilityPolicy::ShowAll)
}

fn request_redraw_while_visuals_active(
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    hud_state: Res<HudState>,
    panels: Query<&TerminalPresentation, With<TerminalPanel>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let terminal_work_pending = terminal_manager.iter().any(|(id, terminal)| {
        terminal.pending_damage.is_some()
            || presentation_store
                .get(id)
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
        hud_needs_redraw(&hud_state),
    ) {
        redraws.write(RequestRedraw);
    }
}

fn setup_scene(mut ctx: SceneSetupContext, auto_verify: Option<Res<AutoVerifyConfig>>) {
    ctx.commands.spawn((
        Camera2d,
        VelloView,
        RenderLayers::layer(0),
        TerminalCameraMarker,
    ));

    ctx.commands.spawn((
        Sprite {
            color: Color::srgba(0.03, 0.03, 0.04, 0.0),
            custom_size: Some(Vec2::ONE),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 2.9),
        Visibility::Hidden,
        TerminalHudSurfaceMarker,
    ));

    ctx.session_persistence.path = resolve_terminal_sessions_path();
    ctx.notes_state.path = resolve_terminal_notes_path();
    if let Some(path) = ctx.notes_state.path.as_ref() {
        let notes = crate::terminals::load_terminal_notes_from(path);
        ctx.notes_state.load(notes);
    }

    if let Some(config) = auto_verify {
        setup_verifier_terminal(&mut ctx, config.clone());
        return;
    }

    restore_startup_terminals(&mut ctx);
}

fn setup_verifier_terminal(ctx: &mut SceneSetupContext, config: AutoVerifyConfig) {
    let session_name = match ctx.runtime_spawner.create_session(VERIFIER_SESSION_PREFIX) {
        Ok(session_name) => session_name,
        Err(error) => {
            append_debug_log(format!("verifier terminal spawn failed: {error}"));
            return;
        }
    };
    let (terminal_id, dispatcher_bridge) = match spawn_attached_terminal_with_presentation(
        &mut ctx.commands,
        &mut ctx.images,
        &mut ctx.terminal_manager,
        &mut ctx.presentation_store,
        &ctx.runtime_spawner,
        session_name.clone(),
        true,
    ) {
        Ok(result) => result,
        Err(error) => {
            append_debug_log(format!(
                "verifier terminal attach failed for {}: {error}",
                session_name
            ));
            let _ = ctx.runtime_spawner.kill_session(&session_name);
            return;
        }
    };
    ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
    append_debug_log(format!(
        "spawned verifier terminal {} session={}",
        terminal_id.0, session_name
    ));
    start_auto_verify_dispatcher(dispatcher_bridge, ctx.runtime_spawner.notifier(), config);
}

fn restore_startup_terminals(ctx: &mut SceneSetupContext) {
    let persisted = ctx
        .session_persistence
        .path
        .as_ref()
        .map(load_persisted_terminal_sessions_from)
        .unwrap_or_default();
    let live_sessions = match ctx.runtime_spawner.list_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            append_debug_log(format!("daemon session discovery failed: {error}"));
            return;
        }
    };
    let reconciled = reconcile_terminal_sessions(&persisted, &live_sessions);
    if !reconciled.prune.is_empty() || !reconciled.import.is_empty() {
        mark_terminal_sessions_dirty(&mut ctx.session_persistence, None);
    }

    for record in &reconciled.prune {
        append_debug_log(format!(
            "pruned stale terminal session metadata {}",
            record.session_name
        ));
    }

    for record in reconciled.ordered_sessions() {
        let restored = reconciled
            .restore
            .iter()
            .any(|existing| existing.session_name == record.session_name);
        let (terminal_id, _) = match spawn_attached_terminal_with_presentation(
            &mut ctx.commands,
            &mut ctx.images,
            &mut ctx.terminal_manager,
            &mut ctx.presentation_store,
            &ctx.runtime_spawner,
            record.session_name.clone(),
            false,
        ) {
            Ok(result) => result,
            Err(error) => {
                append_debug_log(format!(
                    "startup attach failed for {}: {error}",
                    record.session_name
                ));
                continue;
            }
        };
        append_debug_log(format!(
            "{} terminal {} session={}",
            if restored { "restored" } else { "imported" },
            terminal_id.0,
            record.session_name
        ));
        if let Some(label) = record.label {
            ctx.agent_directory.labels.insert(terminal_id, label);
        }
    }

    let restored_focus_session = reconciled
        .restore
        .iter()
        .find(|record| record.last_focused)
        .map(|record| record.session_name.as_str());
    let restored_session_names = reconciled
        .restore
        .iter()
        .map(|record| record.session_name.as_str())
        .collect::<Vec<_>>();
    let imported_session_names = reconciled
        .import
        .iter()
        .map(|record| record.session_name.as_str())
        .collect::<Vec<_>>();

    if let Some(session_name) = choose_startup_focus_session_name(
        restored_focus_session,
        &restored_session_names,
        &imported_session_names,
    ) {
        let focused_id = ctx
            .terminal_manager
            .iter()
            .find(|(_, terminal)| terminal.session_name == session_name)
            .map(|(terminal_id, _)| terminal_id);
        if let Some(terminal_id) = focused_id {
            ctx.terminal_manager.focus_terminal(terminal_id);
            ctx.visibility_state.policy = startup_visibility_policy_for_focus(Some(terminal_id));
            append_debug_log(format!(
                "restored startup focus terminal {} session={}",
                terminal_id.0, session_name
            ));
        }
    }
}
