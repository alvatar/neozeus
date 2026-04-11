use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::{
        focus_terminal_without_persist, resolve_app_state_path, restore_app,
        spawn_runtime_terminal_session, AppSessionState, AppStatePersistenceState, VisibilityMode,
    },
    conversations::{
        resolve_conversations_path, restore_persisted_conversations_from_path,
        ConversationPersistenceState, ConversationStore,
    },
    hud::{hud_needs_redraw, HudInputCaptureState, HudLayoutState, TerminalVisibilityState},
    terminals::{
        append_debug_log, refresh_owned_tmux_sessions_now, resolve_terminal_notes_path,
        terminal_readiness_for_id, OwnedTmuxSessionStore, TerminalCameraMarker, TerminalFocusState,
        TerminalHudSurfaceMarker, TerminalManager, TerminalPanel, TerminalPresentation,
        TerminalPresentationStore, TerminalReadiness, TerminalRuntimeSpawner,
        VERIFIER_SESSION_PREFIX,
    },
    verification::{start_auto_verify_dispatcher, AutoVerifyConfig, VerificationScenarioConfig},
    visual_contract::VisualContractState,
};
use bevy::{
    camera::visibility::RenderLayers, ecs::system::SystemParam, prelude::*, window::RequestRedraw,
};
use bevy_vello::prelude::VelloView;
use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
};

const PRESENTATION_EPSILON: f32 = 0.25;
const ALPHA_EPSILON: f32 = 0.01;
const Z_EPSILON: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartupConnectPhase {
    Connecting,
    Restoring,
    Ready,
    Failed,
}

type StartupDaemonConnectResult = Result<crate::terminals::TerminalDaemonClientResource, String>;
type StartupDaemonConnectReceiver = Arc<Mutex<mpsc::Receiver<StartupDaemonConnectResult>>>;

#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub(crate) struct DaemonConnectionState {
    phase: StartupConnectPhase,
    status: String,
}

impl Default for DaemonConnectionState {
    fn default() -> Self {
        Self {
            phase: StartupConnectPhase::Connecting,
            status: "Connecting".to_owned(),
        }
    }
}

impl DaemonConnectionState {
    #[cfg(test)]
    pub(crate) fn with_phase_for_test(phase: StartupConnectPhase, status: &str) -> Self {
        Self {
            phase,
            status: status.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn phase(&self) -> StartupConnectPhase {
        self.phase
    }

    pub(crate) fn title(&self) -> &'static str {
        match self.phase {
            StartupConnectPhase::Connecting | StartupConnectPhase::Restoring => "Connecting",
            StartupConnectPhase::Ready => "",
            StartupConnectPhase::Failed => "Connection failed",
        }
    }

    pub(crate) fn status(&self) -> &str {
        &self.status
    }

    pub(crate) fn modal_visible(&self) -> bool {
        !matches!(self.phase, StartupConnectPhase::Ready)
    }

    fn set_phase(&mut self, phase: StartupConnectPhase, status: impl Into<String>) {
        self.phase = phase;
        self.status = status.into();
    }

    fn set_ready(&mut self) {
        self.phase = StartupConnectPhase::Ready;
        self.status.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartupConnectWorkflow {
    NotStarted,
    Holding { frames_remaining: u32 },
    AwaitingDaemon,
    RestorePending,
    Finished,
}

#[derive(Resource)]
pub(crate) struct StartupConnectState {
    receiver: Option<StartupDaemonConnectReceiver>,
    workflow: StartupConnectWorkflow,
}

impl Default for StartupConnectState {
    /// Returns the default value for this type.
    fn default() -> Self {
        Self {
            receiver: None,
            workflow: StartupConnectWorkflow::NotStarted,
        }
    }
}

impl StartupConnectState {
    /// Builds a startup-connect state wired to a caller-supplied test receiver.
    #[cfg(test)]
    pub(crate) fn with_receiver_for_test(
        receiver: mpsc::Receiver<StartupDaemonConnectResult>,
    ) -> Self {
        Self {
            receiver: Some(Arc::new(Mutex::new(receiver))),
            workflow: StartupConnectWorkflow::AwaitingDaemon,
        }
    }

    /// Returns whether a background-connect receiver has been installed.
    #[cfg(test)]
    pub(crate) fn has_receiver(&self) -> bool {
        self.receiver.is_some()
    }

    /// Starts the background runtime-connect work if it has not started yet.
    fn start_background_connect(&mut self) {
        if self.receiver.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(Arc::new(Mutex::new(rx)));
        self.workflow = StartupConnectWorkflow::Holding {
            frames_remaining: 8,
        };
        thread::spawn(move || {
            let _ = tx.send(crate::terminals::TerminalDaemonClientResource::system());
        });
    }

    fn connecting_result(&mut self) -> Option<StartupDaemonConnectResult> {
        match &mut self.workflow {
            StartupConnectWorkflow::Holding { frames_remaining } => {
                if *frames_remaining > 0 {
                    *frames_remaining -= 1;
                    None
                } else {
                    self.workflow = StartupConnectWorkflow::AwaitingDaemon;
                    None
                }
            }
            StartupConnectWorkflow::AwaitingDaemon => self
                .receiver
                .as_ref()
                .and_then(|receiver| receiver.lock().ok().and_then(|guard| guard.try_recv().ok())),
            StartupConnectWorkflow::NotStarted
            | StartupConnectWorkflow::RestorePending
            | StartupConnectWorkflow::Finished => None,
        }
    }

    fn mark_restore_pending(&mut self) {
        self.workflow = StartupConnectWorkflow::RestorePending;
    }

    fn take_restore_pending(&mut self) -> bool {
        if self.workflow == StartupConnectWorkflow::RestorePending {
            self.workflow = StartupConnectWorkflow::Finished;
            true
        } else {
            false
        }
    }

    fn awaiting_connect_result(&self) -> bool {
        matches!(
            self.workflow,
            StartupConnectWorkflow::Holding { .. } | StartupConnectWorkflow::AwaitingDaemon
        )
    }

    fn finish(&mut self) {
        self.workflow = StartupConnectWorkflow::Finished;
    }
}

#[derive(SystemParam)]
struct SceneSetupContext<'w, 's> {
    commands: Commands<'w, 's>,
    terminal_manager: ResMut<'w, TerminalManager>,
    focus_state: ResMut<'w, TerminalFocusState>,
    agent_catalog: ResMut<'w, AgentCatalog>,
    runtime_index: ResMut<'w, AgentRuntimeIndex>,
    app_session: ResMut<'w, AppSessionState>,
    aegis_policy: ResMut<'w, crate::aegis::AegisPolicyStore>,
    aegis_runtime: ResMut<'w, crate::aegis::AegisRuntimeStore>,
    selection: Option<ResMut<'w, crate::hud::AgentListSelection>>,
    task_store: Option<ResMut<'w, crate::conversations::AgentTaskStore>>,
    conversations: ResMut<'w, ConversationStore>,
    conversation_persistence: ResMut<'w, ConversationPersistenceState>,
    presentation_store: ResMut<'w, TerminalPresentationStore>,
    runtime_spawner: Res<'w, TerminalRuntimeSpawner>,
    app_state_persistence: ResMut<'w, AppStatePersistenceState>,
    notes_state: ResMut<'w, crate::terminals::TerminalNotesState>,
    input_capture: ResMut<'w, HudInputCaptureState>,
    visibility_state: ResMut<'w, TerminalVisibilityState>,
    view_state: ResMut<'w, crate::terminals::TerminalViewState>,
    owned_tmux_sessions: Option<ResMut<'w, OwnedTmuxSessionStore>>,
    active_terminal_content: Option<ResMut<'w, crate::terminals::ActiveTerminalContentState>>,
    redraws: MessageWriter<'w, RequestRedraw>,
    time: Res<'w, Time>,
}

struct StartupProjectionHandles<'a> {
    selection: &'a mut crate::hud::AgentListSelection,
    owned_tmux_sessions: &'a OwnedTmuxSessionStore,
    active_terminal_content: &'a mut crate::terminals::ActiveTerminalContentState,
}

fn startup_projection_handles<'a>(
    selection: Option<&'a mut crate::hud::AgentListSelection>,
    owned_tmux_sessions: Option<&'a OwnedTmuxSessionStore>,
    active_terminal_content: Option<&'a mut crate::terminals::ActiveTerminalContentState>,
    default_selection: &'a mut crate::hud::AgentListSelection,
    default_owned_tmux_sessions: &'a OwnedTmuxSessionStore,
    default_active_terminal_content: &'a mut crate::terminals::ActiveTerminalContentState,
) -> StartupProjectionHandles<'a> {
    StartupProjectionHandles {
        selection: selection.unwrap_or(default_selection),
        owned_tmux_sessions: owned_tmux_sessions.unwrap_or(default_owned_tmux_sessions),
        active_terminal_content: active_terminal_content.unwrap_or(default_active_terminal_content),
    }
}

#[derive(Clone)]
enum StartupRestoreMode {
    AutoVerify(AutoVerifyConfig),
    VerificationScenario,
    RestoreSessions,
}

fn choose_startup_restore_mode(
    auto_verify: Option<&AutoVerifyConfig>,
    verification_scenario: Option<&VerificationScenarioConfig>,
) -> StartupRestoreMode {
    if let Some(config) = auto_verify {
        StartupRestoreMode::AutoVerify(config.clone())
    } else if verification_scenario.is_some() {
        StartupRestoreMode::VerificationScenario
    } else {
        StartupRestoreMode::RestoreSessions
    }
}

fn startup_restore_status(mode: &StartupRestoreMode) -> &'static str {
    match mode {
        StartupRestoreMode::AutoVerify(_) | StartupRestoreMode::VerificationScenario => {
            "Preparing verification scene…"
        }
        StartupRestoreMode::RestoreSessions => "Restoring sessions…",
    }
}

fn run_startup_restore_mode(ctx: &mut SceneSetupContext, mode: StartupRestoreMode) {
    match mode {
        StartupRestoreMode::AutoVerify(config) => setup_verifier_terminal(ctx, config),
        StartupRestoreMode::VerificationScenario => {}
        StartupRestoreMode::RestoreSessions => restore_startup_terminals(ctx),
    }
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
    contract_visuals_changed: bool,
) -> bool {
    terminal_work_pending
        || presentation_animating
        || hud_visuals_active
        || contract_visuals_changed
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
#[cfg(test)]
pub(crate) fn startup_visibility_policy_for_focus(
    focused_id: Option<crate::terminals::TerminalId>,
) -> crate::hud::TerminalVisibilityPolicy {
    focused_id
        .map(crate::hud::TerminalVisibilityPolicy::Isolate)
        .unwrap_or(crate::hud::TerminalVisibilityPolicy::ShowAll)
}

/// Requests another frame while any terminal or HUD visual state is still changing.
///
/// The system inspects terminal uploads, panel animation, HUD animation, and semantic visual
/// contract changes. If any one of them is still live, a `RequestRedraw` message is emitted so the
/// renderer does not go idle too early.
pub(crate) fn request_redraw_while_visuals_active(
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    layout_state: Res<HudLayoutState>,
    visual_contract: Res<VisualContractState>,
    panels: Query<&TerminalPresentation, With<TerminalPanel>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    // A terminal still counts as pending visual work either when fresh damage has not been
    // rasterized yet, or when a newer surface revision exists than the one currently uploaded to
    // the presentation store.
    let terminal_work_pending = terminal_manager.iter().any(|(id, terminal)| {
        terminal.pending_damage.is_some()
            || presentation_store.get(id).is_some_and(|_| {
                terminal_readiness_for_id(id, &terminal_manager, &presentation_store, None)
                    == TerminalReadiness::Loading
            })
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
        visual_contract.is_changed(),
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
#[allow(
    clippy::type_complexity,
    reason = "exclusive-system wrapper materializes the original startup params via SystemState"
)]
pub(crate) fn setup_scene(world: &mut World) {
    let mut state: bevy::ecs::system::SystemState<(
        SceneSetupContext,
        ResMut<StartupConnectState>,
        ResMut<DaemonConnectionState>,
        Option<Res<AutoVerifyConfig>>,
        Option<Res<VerificationScenarioConfig>>,
    )> = bevy::ecs::system::SystemState::new(world);
    let (mut ctx, mut startup_connect, mut connection_state, auto_verify, verification_scenario) =
        state.get_mut(world);
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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

    if ctx.app_state_persistence.path.is_none() {
        ctx.app_state_persistence.path = resolve_app_state_path();
    }
    if ctx.notes_state.path.is_none() {
        ctx.notes_state.path = resolve_terminal_notes_path();
    }
    if ctx.conversation_persistence.path.is_none() {
        ctx.conversation_persistence.path = resolve_conversations_path();
    }
    if let Some(path) = ctx.notes_state.path.as_ref() {
        let notes = crate::terminals::load_terminal_notes_from(path);
        ctx.notes_state.load(notes);
    }

    if ctx.runtime_spawner.is_ready() {
        let mode =
            choose_startup_restore_mode(auto_verify.as_deref(), verification_scenario.as_deref());
        connection_state.set_phase(
            StartupConnectPhase::Restoring,
            startup_restore_status(&mode),
        );
        run_startup_restore_mode(&mut ctx, mode);
        startup_connect.finish();
        connection_state.set_ready();
        ctx.redraws.write(RequestRedraw);
        state.apply(world);
        return;
    }

    if verification_scenario.is_some() {
        append_debug_log("verification scenario startup: waiting for runtime connection");
    }
    startup_connect.start_background_connect();
    ctx.redraws.write(RequestRedraw);
    state.apply(world);
}

#[allow(
    clippy::too_many_arguments,
    reason = "startup connection advance needs the startup scene resources plus optional verification modes"
)]
#[allow(
    clippy::type_complexity,
    reason = "exclusive-system wrapper materializes the original startup params via SystemState"
)]
/// Advances startup connecting.
pub(crate) fn advance_startup_connecting(world: &mut World) {
    let mut state: bevy::ecs::system::SystemState<(
        SceneSetupContext,
        ResMut<StartupConnectState>,
        ResMut<DaemonConnectionState>,
        Option<Res<AutoVerifyConfig>>,
        Option<Res<VerificationScenarioConfig>>,
    )> = bevy::ecs::system::SystemState::new(world);
    let (mut ctx, mut startup_connect, mut connection_state, auto_verify, verification_scenario) =
        state.get_mut(world);
    macro_rules! finish {
        () => {{
            state.apply(world);
            return;
        }};
    }
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    match connection_state.phase {
        StartupConnectPhase::Connecting => {
            let was_waiting = startup_connect.awaiting_connect_result();
            match startup_connect.connecting_result() {
                Some(Ok(daemon)) => {
                    let mode = choose_startup_restore_mode(
                        auto_verify.as_deref(),
                        verification_scenario.as_deref(),
                    );
                    ctx.runtime_spawner.install_daemon(daemon);
                    startup_connect.mark_restore_pending();
                    connection_state.set_phase(
                        StartupConnectPhase::Restoring,
                        startup_restore_status(&mode),
                    );
                    ctx.redraws.write(RequestRedraw);
                }
                Some(Err(error)) => {
                    startup_connect.finish();
                    connection_state.set_phase(StartupConnectPhase::Failed, error);
                    ctx.redraws.write(RequestRedraw);
                }
                None if was_waiting => {
                    ctx.redraws.write(RequestRedraw);
                }
                None => {}
            }
        }
        StartupConnectPhase::Restoring => {
            if !ctx.runtime_spawner.is_ready() || !startup_connect.take_restore_pending() {
                finish!();
            }
            let mode = choose_startup_restore_mode(
                auto_verify.as_deref(),
                verification_scenario.as_deref(),
            );
            run_startup_restore_mode(&mut ctx, mode);
            connection_state.set_ready();
            ctx.redraws.write(RequestRedraw);
        }
        StartupConnectPhase::Ready | StartupConnectPhase::Failed => {}
    }
    state.apply(world);
}

/// Records a startup-spawned terminal in the optional loading tracker resource.
///
/// The helper keeps the call sites terse and centralizes the "resource may be absent" handling used
/// by tests and stripped-down worlds.
fn hydrate_startup_owned_tmux_state(ctx: &mut SceneSetupContext) {
    let Some(owned_tmux_sessions) = ctx.owned_tmux_sessions.as_deref_mut() else {
        return;
    };
    if let Err(error) = refresh_owned_tmux_sessions_now(&ctx.runtime_spawner, owned_tmux_sessions) {
        append_debug_log(format!("startup owned tmux refresh failed: {error}"));
    }
}

fn register_startup_loading_terminal(
    ctx: &mut SceneSetupContext,
    terminal_id: crate::terminals::TerminalId,
) {
    ctx.presentation_store.mark_startup_pending(terminal_id);
}

/// Spawns the dedicated verifier terminal used by the auto-verify mode.
///
/// The flow is: create a fresh daemon session with the verifier prefix, attach it into the local
/// terminal/presentation state, isolate it visually, mark it as startup-loading, and then launch the
/// delayed command dispatcher that will feed the verification command into the terminal. If attach
/// fails after the daemon session is created, the session is killed immediately so startup does not
/// leak orphan sessions.
fn setup_verifier_terminal(ctx: &mut SceneSetupContext, config: AutoVerifyConfig) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let (session_name, terminal_id, dispatcher_bridge) = match spawn_runtime_terminal_session(
        &mut ctx.terminal_manager,
        &mut ctx.focus_state,
        &ctx.runtime_spawner,
        VERIFIER_SESSION_PREFIX,
        None,
        None,
        &[],
        true,
    ) {
        Ok(result) => result,
        Err(error) => {
            append_debug_log(format!("verifier terminal spawn failed: {error}"));
            return;
        }
    };
    let default_owned_tmux_sessions = OwnedTmuxSessionStore::default();
    let mut default_active_terminal_content =
        crate::terminals::ActiveTerminalContentState::default();
    let mut default_selection = crate::hud::AgentListSelection::default();
    let projection = startup_projection_handles(
        ctx.selection.as_deref_mut(),
        ctx.owned_tmux_sessions.as_deref(),
        ctx.active_terminal_content.as_deref_mut(),
        &mut default_selection,
        &default_owned_tmux_sessions,
        &mut default_active_terminal_content,
    );
    focus_terminal_without_persist(
        terminal_id,
        VisibilityMode::FocusedOnly,
        &mut ctx.app_session,
        &ctx.agent_catalog,
        &ctx.runtime_index,
        projection.owned_tmux_sessions,
        projection.selection,
        projection.active_terminal_content,
        &mut ctx.terminal_manager,
        &mut ctx.focus_state,
        &mut ctx.input_capture,
        &mut ctx.view_state,
        &mut ctx.visibility_state,
        &mut ctx.redraws,
    );
    register_startup_loading_terminal(ctx, terminal_id);
    append_debug_log(format!(
        "spawned verifier terminal {} session={}",
        terminal_id.0, session_name
    ));
    start_auto_verify_dispatcher(dispatcher_bridge, ctx.runtime_spawner.notifier(), config);
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
pub(crate) fn rehydrate_restored_projection_state(
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    notes_state: &mut crate::terminals::TerminalNotesState,
    task_store: Option<&mut crate::conversations::AgentTaskStore>,
    conversation_persistence: &ConversationPersistenceState,
    conversations: &mut ConversationStore,
    time: &Time,
) {
    if let Some(task_store) = task_store {
        let mut migrated_legacy_notes = false;
        for (agent_id, session_name) in runtime_index.session_bindings() {
            let stable_text = agent_catalog
                .uid(agent_id)
                .and_then(|agent_uid| notes_state.note_text_by_agent_uid(agent_uid));
            if let Some(text) = stable_text {
                let _ = task_store.set_text(agent_id, text);
                continue;
            }
            let legacy_text = notes_state.note_text(session_name).map(str::to_owned);
            if let Some(text) = legacy_text.as_deref() {
                let _ = task_store.set_text(agent_id, text);
                migrated_legacy_notes |= notes_state.remove_legacy_note_text(session_name);
            }
        }
        if migrated_legacy_notes {
            crate::terminals::mark_terminal_notes_dirty(notes_state, Some(time));
        }
    }

    if let Some(path) = conversation_persistence.path.as_ref() {
        restore_persisted_conversations_from_path(
            path,
            agent_catalog,
            runtime_index,
            conversations,
        );
    } else {
        *conversations = ConversationStore::default();
    }
}

fn restore_startup_terminals(ctx: &mut SceneSetupContext) {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let mut default_selection = crate::hud::AgentListSelection::None;
    let default_owned_tmux_sessions = OwnedTmuxSessionStore::default();
    let mut default_active_terminal_content =
        crate::terminals::ActiveTerminalContentState::default();
    let projection = startup_projection_handles(
        ctx.selection.as_deref_mut(),
        ctx.owned_tmux_sessions.as_deref(),
        ctx.active_terminal_content.as_deref_mut(),
        &mut default_selection,
        &default_owned_tmux_sessions,
        &mut default_active_terminal_content,
    );
    let summary = restore_app(
        &mut ctx.agent_catalog,
        &mut ctx.runtime_index,
        &mut ctx.app_session,
        projection.selection,
        &mut ctx.terminal_manager,
        &mut ctx.focus_state,
        projection.owned_tmux_sessions,
        projection.active_terminal_content,
        &ctx.runtime_spawner,
        &mut ctx.input_capture,
        &mut ctx.app_state_persistence,
        &mut ctx.aegis_policy,
        &mut ctx.aegis_runtime,
        &mut ctx.visibility_state,
        &mut ctx.view_state,
        Some(&mut ctx.presentation_store),
        &ctx.time,
        &mut ctx.redraws,
    );
    if summary.snapshot_found {
        let status = crate::app::render_recovery_status_summary(
            "Automatic recovery completed",
            &summary,
            vec!["Automatic recovery started from saved snapshot".to_owned()],
        );
        ctx.app_session
            .recovery_status
            .show(status.tone, status.title, status.details);
    } else {
        ctx.app_session.recovery_status.clear();
    }

    hydrate_startup_owned_tmux_state(ctx);

    rehydrate_restored_projection_state(
        &ctx.agent_catalog,
        &ctx.runtime_index,
        &mut ctx.notes_state,
        ctx.task_store.as_deref_mut(),
        &ctx.conversation_persistence,
        &mut ctx.conversations,
        &ctx.time,
    );
}
