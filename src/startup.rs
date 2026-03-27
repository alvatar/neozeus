use crate::{
    hud::{
        hud_needs_redraw, AgentDirectory, HudLayoutState, TerminalVisibilityPolicy,
        TerminalVisibilityState,
    },
    terminals::{
        append_debug_log, load_persisted_terminal_sessions_from, mark_terminal_sessions_dirty,
        reconcile_terminal_sessions, resolve_terminal_notes_path, resolve_terminal_sessions_path,
        spawn_attached_terminal_with_presentation, DaemonSessionInfo, TerminalCameraMarker,
        TerminalFocusState, TerminalHudSurfaceMarker, TerminalLifecycle, TerminalManager,
        TerminalPanel, TerminalPresentation, TerminalPresentationStore, TerminalRuntimeSpawner,
        TerminalSessionPersistenceState, PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
    },
    verification::{start_auto_verify_dispatcher, AutoVerifyConfig, VerificationScenarioConfig},
};
use bevy::{
    camera::visibility::RenderLayers, ecs::system::SystemParam, prelude::*, window::RequestRedraw,
};
use bevy_vello::prelude::VelloView;
use std::collections::BTreeSet;

const PRESENTATION_EPSILON: f32 = 0.25;
const ALPHA_EPSILON: f32 = 0.01;
const Z_EPSILON: f32 = 0.01;

#[derive(Resource, Default, Clone, Debug)]
pub(crate) struct StartupLoadingState {
    pending_terminal_ids: BTreeSet<crate::terminals::TerminalId>,
}

impl StartupLoadingState {
    /// Marks a terminal as still loading during startup.
    ///
    /// The startup flow uses this set to distinguish terminals that have been spawned or restored
    /// from terminals that already have a presentable frame. In practice that lets presentation code
    /// keep placeholders visible until the first real surface arrives.
    pub(crate) fn register(&mut self, terminal_id: crate::terminals::TerminalId) {
        self.pending_terminal_ids.insert(terminal_id);
    }

    /// Removes a terminal from the startup-loading set once its first usable frame has landed.
    ///
    /// Nothing else is tracked here; the set is purely a coarse startup gate, so resolving simply
    /// deletes the terminal id from the backing `BTreeSet`.
    pub(crate) fn resolve(&mut self, terminal_id: crate::terminals::TerminalId) {
        self.pending_terminal_ids.remove(&terminal_id);
    }

    /// Returns whether a specific terminal is still considered startup-pending.
    ///
    /// This is used by presentation code to decide whether to keep showing startup placeholders or
    /// temporary visibility overrides for that terminal.
    pub(crate) fn is_pending(&self, terminal_id: crate::terminals::TerminalId) -> bool {
        self.pending_terminal_ids.contains(&terminal_id)
    }

    /// Returns whether any terminal is still in the startup-loading phase.
    ///
    /// The check is just `set.is_empty()`, but keeping it behind a named method makes the rest of
    /// the startup/presentation code read in terms of domain state instead of container mechanics.
    pub(crate) fn active(&self) -> bool {
        !self.pending_terminal_ids.is_empty()
    }
}

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
    startup_loading: Option<ResMut<'w, StartupLoadingState>>,
}

/// Collapses the three sources of visual work into a single redraw decision.
///
/// The startup and render systems separately know about terminal damage, terminal animation, and HUD
/// animation. This helper keeps the policy simple: if any of those subsystems still has visible work
/// pending, the frame loop should request another redraw.
pub(crate) fn should_request_visual_redraw(
    terminal_work_pending: bool,
    presentation_animating: bool,
    hud_visuals_active: bool,
) -> bool {
    terminal_work_pending || presentation_animating || hud_visuals_active
}

/// Chooses which restored/imported session should receive focus after startup reconciliation.
///
/// The precedence is intentional: reuse the explicitly persisted focus if it is still valid, then
/// fall back to the first restorable session, and finally to the first imported live session. The
/// function only chooses a session name; the caller still has to resolve that name back to a local
/// terminal id.
pub(crate) fn choose_startup_focus_session_name<'a>(
    restored_focus_session: Option<&'a str>,
    restored_session_names: &[&'a str],
    imported_session_names: &[&'a str],
) -> Option<&'a str> {
    restored_focus_session
        .or_else(|| restored_session_names.first().copied())
        .or_else(|| imported_session_names.first().copied())
}

/// Computes the initial visibility policy once startup focus has been decided.
///
/// If a terminal could be focused, startup goes into isolate mode so the chosen terminal becomes the
/// primary view. If nothing interactive could be focused, the app falls back to `ShowAll` so the
/// restored disconnected sessions remain visible instead of disappearing behind an empty focus slot.
pub(crate) fn startup_visibility_policy_for_focus(
    focused_id: Option<crate::terminals::TerminalId>,
) -> TerminalVisibilityPolicy {
    focused_id
        .map(TerminalVisibilityPolicy::Isolate)
        .unwrap_or(TerminalVisibilityPolicy::ShowAll)
}

/// Filters startup focus candidates down to sessions that are still genuinely interactive.
///
/// The restore logic deliberately refuses to focus exited or disconnected sessions on boot. They may
/// still be restored for visibility and bookkeeping, but only `Running` sessions participate in the
/// focus selection pass.
fn startup_focus_candidate_is_interactive(session: &DaemonSessionInfo) -> bool {
    matches!(session.runtime.lifecycle, TerminalLifecycle::Running)
}

/// Requests another frame while any terminal or HUD visual state is still changing.
///
/// The system inspects three classes of work: terminal snapshots that have not yet been uploaded,
/// terminal panels still animating toward their targets, and HUD modules that report active visual
/// work. If any one of them is still live, a `RequestRedraw` message is emitted so the renderer does
/// not go idle too early.
pub(crate) fn request_redraw_while_visuals_active(
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    layout_state: Res<HudLayoutState>,
    panels: Query<&TerminalPresentation, With<TerminalPanel>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    // A terminal still counts as pending visual work either when fresh damage has not been
    // rasterized yet, or when a newer surface revision exists than the one currently uploaded to
    // the presentation store.
    let terminal_work_pending = terminal_manager.iter().any(|(id, terminal)| {
        terminal.pending_damage.is_some()
            || presentation_store
                .get(id)
                .map(|presented| terminal.surface_revision != presented.uploaded_revision)
                .unwrap_or(false)
    });
    // Panel animation is treated geometrically: any meaningful difference in position, size,
    // alpha, or Z means the panel is still moving and the scene needs another frame.
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

/// Performs scene-level startup before the regular update schedule begins.
///
/// The function creates the terminal camera and hidden HUD composite sprite, resolves persistence
/// paths, loads saved terminal notes, and then chooses one of three mutually exclusive startup
/// paths: auto-verify bootstrap, deterministic verification scenario bootstrap, or normal session
/// restore/import.
pub(crate) fn setup_scene(
    mut ctx: SceneSetupContext,
    auto_verify: Option<Res<AutoVerifyConfig>>,
    verification_scenario: Option<Res<VerificationScenarioConfig>>,
) {
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
    if verification_scenario.is_some() {
        // Verification scenarios want a blank slate and will spawn their own deterministic terminal
        // set later from the update loop, so normal restore/import must be skipped entirely.
        append_debug_log("verification scenario startup: skipping restore/import");
        return;
    }

    restore_startup_terminals(&mut ctx);
}

/// Records a startup-spawned terminal in the optional loading tracker resource.
///
/// The helper keeps the call sites terse and centralizes the "resource may be absent" handling used
/// by tests and stripped-down worlds.
fn register_startup_loading_terminal(
    ctx: &mut SceneSetupContext,
    terminal_id: crate::terminals::TerminalId,
) {
    if let Some(startup_loading) = ctx.startup_loading.as_mut() {
        startup_loading.register(terminal_id);
    }
}

/// Spawns the dedicated verifier terminal used by the auto-verify mode.
///
/// The flow is: create a fresh daemon session with the verifier prefix, attach it into the local
/// terminal/presentation state, isolate it visually, mark it as startup-loading, and then launch the
/// delayed command dispatcher that will feed the verification command into the terminal. If attach
/// fails after the daemon session is created, the session is killed immediately so startup does not
/// leak orphan sessions.
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
    register_startup_loading_terminal(ctx, terminal_id);
    append_debug_log(format!(
        "spawned verifier terminal {} session={}",
        terminal_id.0, session_name
    ));
    start_auto_verify_dispatcher(dispatcher_bridge, ctx.runtime_spawner.notifier(), config);
}

/// Spawns the fallback initial terminal when normal restore/import cannot produce one.
///
/// This path is used both for cold starts and for failure recovery during startup. It creates a new
/// persistent daemon session, attaches it locally, isolates it as the visible terminal, marks session
/// persistence dirty, and logs the reason so startup behavior is debuggable after the fact.
fn spawn_initial_terminal(ctx: &mut SceneSetupContext, reason: &str) {
    let session_name = match ctx
        .runtime_spawner
        .create_session(PERSISTENT_SESSION_PREFIX)
    {
        Ok(session_name) => session_name,
        Err(error) => {
            append_debug_log(format!("initial terminal spawn failed ({reason}): {error}"));
            return;
        }
    };
    let (terminal_id, _) = match spawn_attached_terminal_with_presentation(
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
                "initial terminal attach failed for {} ({reason}): {error}",
                session_name
            ));
            let _ = ctx.runtime_spawner.kill_session(&session_name);
            return;
        }
    };
    ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
    register_startup_loading_terminal(ctx, terminal_id);
    mark_terminal_sessions_dirty(&mut ctx.session_persistence, None);
    append_debug_log(format!(
        "spawned initial terminal {} session={} reason={reason}",
        terminal_id.0, session_name
    ));
}

/// Restores persisted sessions, imports any extra live daemon sessions, and reconstructs startup
/// focus/visibility state.
///
/// The procedure is intentionally conservative:
/// - load persisted session metadata,
/// - ask the daemon for the currently live sessions,
/// - reconcile persisted vs live names into restore/import/prune buckets,
/// - attach every surviving session without auto-focusing,
/// - rebuild labels and focus from the reconciled metadata,
/// - and only spawn a brand-new terminal if nothing usable remains.
///
/// A subtle but important rule lives here: disconnected/exited sessions may still be restored for
/// visibility, but they are filtered out of focus selection so startup does not land on a dead
/// terminal.
fn restore_startup_terminals(ctx: &mut SceneSetupContext) {
    let persisted = ctx
        .session_persistence
        .path
        .as_ref()
        .map(load_persisted_terminal_sessions_from)
        .unwrap_or_default();
    let live_session_infos = match ctx.runtime_spawner.list_session_infos() {
        Ok(sessions) => sessions,
        Err(error) => {
            append_debug_log(format!("daemon session discovery failed: {error}"));
            spawn_initial_terminal(ctx, "session discovery failed");
            return;
        }
    };
    let live_sessions = live_session_infos
        .iter()
        .map(|session| session.session_id.clone())
        .collect::<Vec<_>>();
    let reconciled = reconcile_terminal_sessions(&persisted, &live_sessions);
    if !reconciled.prune.is_empty() || !reconciled.import.is_empty() {
        // Reconciliation changed what should be persisted, so schedule a save even though the actual
        // write still happens later through the debounced persistence system.
        mark_terminal_sessions_dirty(&mut ctx.session_persistence, None);
    }

    for record in &reconciled.prune {
        append_debug_log(format!(
            "pruned stale terminal session metadata {}",
            record.session_name
        ));
    }

    // Attach every session in the reconciled order so the restored local creation order mirrors the
    // persisted order instead of the daemon's arbitrary listing order.
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
        register_startup_loading_terminal(ctx, terminal_id);
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

    let live_session_lookup = live_session_infos
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect::<std::collections::HashMap<_, _>>();
    // Focus restoration is name-based first because the local terminal ids only exist after the
    // attach loop above has recreated them.
    let restored_focus_session = reconciled
        .restore
        .iter()
        .find(|record| {
            record.last_focused
                && live_session_lookup
                    .get(record.session_name.as_str())
                    .is_some_and(|session| startup_focus_candidate_is_interactive(session))
        })
        .map(|record| record.session_name.as_str());
    let restored_session_names = reconciled
        .restore
        .iter()
        .filter(|record| {
            live_session_lookup
                .get(record.session_name.as_str())
                .is_some_and(|session| startup_focus_candidate_is_interactive(session))
        })
        .map(|record| record.session_name.as_str())
        .collect::<Vec<_>>();
    let imported_session_names = reconciled
        .import
        .iter()
        .filter(|record| {
            live_session_lookup
                .get(record.session_name.as_str())
                .is_some_and(|session| startup_focus_candidate_is_interactive(session))
        })
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
    } else if !ctx.terminal_manager.terminal_ids().is_empty() {
        ctx.focus_state.clear_active_terminal();
        #[cfg(test)]
        ctx.terminal_manager
            .replace_test_focus_state(&ctx.focus_state);
        ctx.visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
        append_debug_log("startup restored terminals but none are interactive; leaving them visible and unfocused");
    } else {
        spawn_initial_terminal(ctx, "no restored or imported sessions");
    }
}
