use crate::{
    hud::{
        hud_needs_redraw, AgentDirectory, HudLayoutState, TerminalVisibilityPolicy,
        TerminalVisibilityState,
    },
    terminals::{
        append_debug_log, load_persisted_terminal_sessions_from, mark_terminal_sessions_dirty,
        reconcile_terminal_sessions, resolve_terminal_notes_path, resolve_terminal_sessions_path,
        spawn_attached_terminal_with_presentation, TerminalCameraMarker, TerminalFocusState,
        TerminalHudSurfaceMarker, TerminalManager, TerminalPanel, TerminalPresentation,
        TerminalPresentationStore, TerminalRuntimeSpawner, TerminalSessionPersistenceState,
        VERIFIER_SESSION_PREFIX,
    },
    verification::{start_auto_verify_dispatcher, AutoVerifyConfig},
};
use bevy::{
    camera::visibility::RenderLayers, ecs::system::SystemParam, prelude::*, window::RequestRedraw,
};
use bevy_vello::prelude::VelloView;

const PRESENTATION_EPSILON: f32 = 0.25;
const ALPHA_EPSILON: f32 = 0.01;
const Z_EPSILON: f32 = 0.01;

#[derive(SystemParam)]
pub(crate) struct SceneSetupContext<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    terminal_manager: ResMut<'w, TerminalManager>,
    focus_state: ResMut<'w, TerminalFocusState>,
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

pub(crate) fn request_redraw_while_visuals_active(
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    layout_state: Res<HudLayoutState>,
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
        hud_needs_redraw(&layout_state),
    ) {
        redraws.write(RequestRedraw);
    }
}

pub(crate) fn setup_scene(mut ctx: SceneSetupContext, auto_verify: Option<Res<AutoVerifyConfig>>) {
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
        &mut ctx.focus_state,
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
            &mut ctx.focus_state,
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
            ctx.focus_state
                .focus_terminal(&ctx.terminal_manager, terminal_id);
            #[cfg(test)]
            ctx.terminal_manager
                .replace_test_focus_state(&ctx.focus_state);
            ctx.visibility_state.policy = startup_visibility_policy_for_focus(Some(terminal_id));
            append_debug_log(format!(
                "restored startup focus terminal {} session={}",
                terminal_id.0, session_name
            ));
        }
    }
}
