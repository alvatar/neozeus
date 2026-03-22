use crate::{
    app_config::GPU_NOT_FOUND_PANIC_FRAGMENT,
    hud::{
        animate_hud_modules, apply_hud_commands, handle_hud_module_shortcuts,
        handle_hud_pointer_input, hud_needs_redraw, render_hud_scene, save_hud_layout_if_dirty,
        setup_hud, AgentDirectory, HudDispatcher, HudPersistenceState, HudState,
        TerminalVisibilityPolicy, TerminalVisibilityState,
    },
    input::{
        drag_terminal_view, focus_terminal_on_panel_click, handle_global_terminal_spawn_shortcut,
        handle_terminal_direct_input_keyboard, handle_terminal_lifecycle_shortcuts,
        handle_terminal_message_box_keyboard, hide_terminal_on_background_click,
        zoom_terminal_view,
    },
    terminals::{
        append_debug_log, configure_terminal_fonts, generate_unique_session_name,
        load_persisted_terminal_sessions_from, mark_terminal_sessions_dirty,
        provision_terminal_target, reconcile_terminal_sessions, resolve_terminal_sessions_path,
        save_terminal_sessions_if_dirty, spawn_attached_terminal_with_presentation,
        sync_terminal_hud_surface, sync_terminal_panel_frames, sync_terminal_presentations,
        sync_terminal_texture, TerminalCameraMarker, TerminalFontState, TerminalGlyphCache,
        TerminalHudSurfaceMarker, TerminalManager, TerminalPanel, TerminalPointerState,
        TerminalPresentation, TerminalPresentationStore, TerminalProvisionTarget,
        TerminalRuntimeSpawner, TerminalSessionPersistenceState, TerminalViewState,
        TmuxClientResource, VERIFIER_TMUX_SESSION_PREFIX,
    },
    verification::{start_auto_verify_dispatcher, AutoVerifyConfig},
};
use bevy::{
    camera::visibility::RenderLayers,
    prelude::*,
    render::{settings::WgpuSettings, RenderPlugin},
    window::RequestRedraw,
    winit::{EventLoopProxyWrapper, WinitSettings},
};
use bevy_vello::VelloPlugin;
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
    HudInput,
    HudCommands,
    PresentTerminal,
    HudAnimation,
    HudRender,
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
                    name: Some(env::var("NEOZEUS_APP_ID").unwrap_or_else(|_| "neozeus".to_owned())),
                    resolution: (1400, 900).into(),
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(VelloPlugin::default());

    let event_loop_proxy = {
        let proxy = app.world().resource::<EventLoopProxyWrapper>();
        (**proxy).clone()
    };

    app.insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
        .insert_resource(WinitSettings::desktop_app())
        .insert_resource(TerminalManager::default())
        .insert_resource(TerminalPresentationStore::default())
        .insert_resource(TerminalRuntimeSpawner::new(event_loop_proxy))
        .insert_resource(TmuxClientResource::system())
        .insert_resource(TerminalSessionPersistenceState::default())
        .insert_resource(TerminalFontState::default())
        .insert_resource(TerminalViewState::default())
        .insert_resource(TerminalPointerState::default())
        .insert_resource(TerminalGlyphCache::default())
        .insert_resource(crate::terminals::TerminalTextRenderer::default())
        .insert_resource(HudState::default())
        .insert_resource(HudDispatcher::default())
        .insert_resource(HudPersistenceState::default())
        .insert_resource(AgentDirectory::default())
        .insert_resource(TerminalVisibilityState::default())
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
                .before(NeoZeusSet::PresentTerminal)
                .before(NeoZeusSet::HudCommands),
        )
        .configure_sets(Update, NeoZeusSet::HudInput.before(NeoZeusSet::HudCommands))
        .configure_sets(
            Update,
            NeoZeusSet::HudCommands.before(NeoZeusSet::HudAnimation),
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
        .add_systems(Startup, (setup_scene, setup_hud).chain())
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
        .add_systems(Update, apply_hud_commands.in_set(NeoZeusSet::HudCommands))
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
                save_terminal_sessions_if_dirty,
            )
                .in_set(NeoZeusSet::HudAnimation),
        )
        .add_systems(Update, render_hud_scene.in_set(NeoZeusSet::HudRender))
        .add_systems(
            Update,
            request_redraw_while_visuals_active.in_set(NeoZeusSet::Redraw),
        );

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

#[allow(
    clippy::too_many_arguments,
    reason = "scene startup now owns camera/HUD-surface plus verifier/recovery/persistence resources together"
)]
fn setup_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    mut agent_directory: ResMut<AgentDirectory>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    tmux_client: Res<TmuxClientResource>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    auto_verify: Option<Res<AutoVerifyConfig>>,
) {
    commands.spawn((Camera2d, RenderLayers::layer(0), TerminalCameraMarker));

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

    session_persistence.path = resolve_terminal_sessions_path();

    if let Some(config) = auto_verify {
        let client = tmux_client.client();
        let Ok(session_name) = generate_unique_session_name(client, VERIFIER_TMUX_SESSION_PREFIX)
        else {
            append_debug_log("verifier terminal spawn failed: could not allocate tmux session");
            return;
        };
        if let Err(error) = provision_terminal_target(
            client,
            &TerminalProvisionTarget::TmuxDetached {
                session_name: session_name.clone(),
            },
        ) {
            append_debug_log(format!(
                "verifier terminal spawn failed for {}: {error}",
                session_name
            ));
            return;
        }
        let (terminal_id, dispatcher_bridge) = spawn_attached_terminal_with_presentation(
            &mut commands,
            &mut images,
            &mut terminal_manager,
            &mut presentation_store,
            &runtime_spawner,
            session_name.clone(),
            true,
        );
        visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
        append_debug_log(format!(
            "spawned verifier terminal {} session={}",
            terminal_id.0, session_name
        ));
        start_auto_verify_dispatcher(
            dispatcher_bridge,
            runtime_spawner.notifier(),
            config.clone(),
        );
        return;
    }

    let persisted = session_persistence
        .path
        .as_ref()
        .map(load_persisted_terminal_sessions_from)
        .unwrap_or_default();
    let live_sessions = match tmux_client.client().list_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            append_debug_log(format!("tmux session discovery failed: {error}"));
            return;
        }
    };
    let reconciled = reconcile_terminal_sessions(&persisted, &live_sessions);
    if !reconciled.prune.is_empty() || !reconciled.import.is_empty() {
        mark_terminal_sessions_dirty(&mut session_persistence, None);
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
        let (terminal_id, _) = spawn_attached_terminal_with_presentation(
            &mut commands,
            &mut images,
            &mut terminal_manager,
            &mut presentation_store,
            &runtime_spawner,
            record.session_name.clone(),
            false,
        );
        append_debug_log(format!(
            "{} terminal {} session={}",
            if restored { "restored" } else { "imported" },
            terminal_id.0,
            record.session_name
        ));
        if let Some(label) = record.label {
            agent_directory.labels.insert(terminal_id, label);
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
        let focused_id = terminal_manager
            .iter()
            .find(|(_, terminal)| terminal.session_name == session_name)
            .map(|(terminal_id, _)| terminal_id);
        if let Some(terminal_id) = focused_id {
            terminal_manager.focus_terminal(terminal_id);
            visibility_state.policy = startup_visibility_policy_for_focus(Some(terminal_id));
            append_debug_log(format!(
                "restored startup focus terminal {} session={}",
                terminal_id.0, session_name
            ));
        }
    }
}
