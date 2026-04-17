use crate::{
    agents::{AgentCatalog, AgentKind, AgentRuntimeIndex},
    app::{
        clear_composer_and_direct_input, focus_agent_without_persist, open_composer,
        spawn_runtime_terminal_session, AppSessionState, ComposerRequest, VisibilityMode,
    },
    composer::ComposerMode,
    conversations::AgentTaskStore,
    hud::{AgentListSelection, AgentListView, HudInputCaptureState, TerminalVisibilityState},
    terminals::{
        append_debug_log, terminal_readiness_for_id, RuntimeNotifier, TerminalBridge, TerminalCell,
        TerminalCellContent, TerminalCommand, TerminalFocusState, TerminalId, TerminalManager,
        TerminalPresentationStore, TerminalRuntimeSpawner, TerminalSurface, TerminalViewState,
        VERIFIER_SESSION_PREFIX,
    },
    visual_contract::{TerminalFrameVisualState, VisualAgentActivity, VisualContractState},
};
use bevy::{ecs::system::SystemParam, prelude::Resource, prelude::*, window::RequestRedraw};
use bevy_egui::egui;
use std::{collections::BTreeMap, env, thread, time::Duration};

#[derive(Resource, Clone)]
pub(crate) struct AutoVerifyConfig {
    pub(crate) command: String,
    pub(crate) delay_ms: u64,
}

impl AutoVerifyConfig {
    /// Reads the auto-verify command configuration from the environment.
    ///
    /// Auto-verify is enabled only when a command string is provided. The delay defaults to 1500 ms
    /// so the spawned verifier terminal has a short chance to settle before the command is injected.
    pub(crate) fn from_env() -> Option<Self> {
        Some(Self {
            command: env::var("NEOZEUS_AUTOVERIFY_COMMAND").ok()?,
            delay_ms: env::var("NEOZEUS_AUTOVERIFY_DELAY_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1500),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VerificationScenario {
    MessageBoxBloom,
    TaskDialogBloom,
    AgentListBloom,
    AgentContextBloom,
    OwnedTmuxOrphanSelection,
    OwnedTmuxLiveSelection,
    WorkingStateIdle,
    WorkingStateWorking,
    InspectSwitchLatency,
}

/// Parses the named built-in verification scenario from an optional raw string.
///
/// The parser accepts the small fixed scenario vocabulary used by the offscreen verification scripts
/// and returns `None` for missing or unknown names so callers can treat the feature as disabled.
fn resolve_verification_scenario(raw: Option<&str>) -> Option<VerificationScenario> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("message-box-bloom") => {
            Some(VerificationScenario::MessageBoxBloom)
        }
        Some(value) if value.eq_ignore_ascii_case("task-dialog-bloom") => {
            Some(VerificationScenario::TaskDialogBloom)
        }
        Some(value) if value.eq_ignore_ascii_case("agent-list-bloom") => {
            Some(VerificationScenario::AgentListBloom)
        }
        Some(value) if value.eq_ignore_ascii_case("agent-context-bloom") => {
            Some(VerificationScenario::AgentContextBloom)
        }
        Some(value) if value.eq_ignore_ascii_case("owned-tmux-orphan-selection") => {
            Some(VerificationScenario::OwnedTmuxOrphanSelection)
        }
        Some(value) if value.eq_ignore_ascii_case("owned-tmux-live-selection") => {
            Some(VerificationScenario::OwnedTmuxLiveSelection)
        }
        Some(value) if value.eq_ignore_ascii_case("working-state-idle") => {
            Some(VerificationScenario::WorkingStateIdle)
        }
        Some(value) if value.eq_ignore_ascii_case("working-state-working") => {
            Some(VerificationScenario::WorkingStateWorking)
        }
        Some(value) if value.eq_ignore_ascii_case("inspect-switch-latency") => {
            Some(VerificationScenario::InspectSwitchLatency)
        }
        _ => None,
    }
}

#[derive(Resource, Clone, Debug)]
pub(crate) struct VerificationScenarioConfig {
    pub(crate) scenario: VerificationScenario,
    pub(crate) frames_until_apply: u32,
    pub(crate) primed: bool,
    pub(crate) applied: bool,
    pub(crate) terminal_ids: Vec<TerminalId>,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct VerificationTerminalSurfaceOverrides {
    surfaces: BTreeMap<TerminalId, TerminalSurface>,
    presentation_revision: u64,
}

impl VerificationTerminalSurfaceOverrides {
    pub(crate) fn clear(&mut self) {
        if self.surfaces.is_empty() {
            return;
        }
        self.surfaces.clear();
        self.bump_presentation_revision();
    }

    pub(crate) fn set_surface(&mut self, terminal_id: TerminalId, surface: TerminalSurface) {
        let changed = self.surfaces.get(&terminal_id) != Some(&surface);
        if changed {
            self.surfaces.insert(terminal_id, surface);
            self.bump_presentation_revision();
        }
    }

    pub(crate) fn surface_for(&self, terminal_id: TerminalId) -> Option<&TerminalSurface> {
        self.surfaces.get(&terminal_id)
    }

    pub(crate) fn presentation_override_revision_for(
        &self,
        terminal_id: TerminalId,
    ) -> Option<u64> {
        self.surfaces
            .contains_key(&terminal_id)
            .then_some(self.presentation_revision)
    }

    fn bump_presentation_revision(&mut self) {
        self.presentation_revision = self.presentation_revision.wrapping_add(1);
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VerificationCaptureBarrierState {
    ready: bool,
}

impl VerificationCaptureBarrierState {
    pub(crate) fn ready(&self) -> bool {
        self.ready
    }
}

impl VerificationScenarioConfig {
    /// Reads the verification-scenario configuration from the environment.
    ///
    /// The scenario name is mandatory; when present, the config starts in an unapplied state with a
    /// small default frame delay so the startup/render pipeline can settle before deterministic setup
    /// begins.
    pub(crate) fn from_env() -> Option<Self> {
        Some(Self {
            scenario: resolve_verification_scenario(
                env::var("NEOZEUS_VERIFY_SCENARIO").ok().as_deref(),
            )?,
            frames_until_apply: env::var("NEOZEUS_VERIFY_DELAY_FRAMES")
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(2),
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        })
    }
}

/// Returns whether a terminal has a fully uploaded, non-placeholder frame ready for inspection.
fn terminal_has_presentable_frame(
    terminal_id: TerminalId,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
    verification_overrides: &VerificationTerminalSurfaceOverrides,
) -> bool {
    terminal_readiness_for_id(
        terminal_id,
        terminal_manager,
        presentation_store,
        verification_overrides.presentation_override_revision_for(terminal_id),
    )
    .is_ready_for_capture()
}

/// Builds a deterministic synthetic terminal surface used by verification scenarios.
///
/// The surface is filled with repeated labeled bands on several rows so image captures have stable,
/// high-contrast content that makes focus changes and bloom behavior visually obvious.
fn seeded_inspect_surface(label: &str, accent: egui::Color32) -> TerminalSurface {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let cols = 120;
    let rows = 38;
    let mut surface = TerminalSurface::new(cols, rows);
    let pattern = format!(" {label} ");
    for row in [8usize, 12, 16, 20, 24, 28] {
        let mut x = 4usize;
        while x + pattern.len() < cols.saturating_sub(4) {
            for ch in pattern.chars() {
                surface.set_cell(
                    x,
                    row,
                    TerminalCell {
                        content: TerminalCellContent::Single(ch),
                        fg: egui::Color32::WHITE,
                        bg: accent,
                        style: Default::default(),
                        width: 1,
                        selected: false,
                    },
                );
                x += 1;
            }
        }
    }
    surface
}

/// Writes one text payload into the synthetic verification surface.
///
/// The helper packs the full string into a single terminal cell because the deterministic
/// verification surfaces only need stable, visually distinctive content, not faithful terminal
/// wrapping semantics.
fn set_surface_text(surface: &mut TerminalSurface, x: usize, y: usize, text: &str) {
    let mut chars = text.chars();
    let Some(base) = chars.next() else {
        surface.set_cell(x, y, TerminalCell::default());
        return;
    };
    let extra = chars.collect::<Vec<_>>();
    surface.set_cell(
        x,
        y,
        TerminalCell {
            content: TerminalCellContent::from_parts(base, Some(&extra)),
            ..Default::default()
        },
    );
}

fn seeded_activity_contract_surface(working: bool) -> TerminalSurface {
    let mut surface = TerminalSurface::new(120, 8);
    set_surface_text(&mut surface, 0, 0, "neozeus working-state contract");
    set_surface_text(&mut surface, 0, 2, "status contract surface");
    if working {
        set_surface_text(&mut surface, 1, 3, "⠋ Working...");
    } else {
        set_surface_text(&mut surface, 0, 3, "ready");
    }
    set_surface_text(&mut surface, 0, 7, "footer");
    surface
}

fn seed_terminal_surface(
    overrides: &mut VerificationTerminalSurfaceOverrides,
    terminal_id: TerminalId,
    label: &str,
    accent: egui::Color32,
) {
    overrides.set_surface(terminal_id, seeded_inspect_surface(label, accent));
}

fn verification_agent_kind(scenario: VerificationScenario) -> AgentKind {
    match scenario {
        VerificationScenario::WorkingStateIdle | VerificationScenario::WorkingStateWorking => {
            AgentKind::Pi
        }
        VerificationScenario::MessageBoxBloom
        | VerificationScenario::TaskDialogBloom
        | VerificationScenario::AgentListBloom
        | VerificationScenario::AgentContextBloom
        | VerificationScenario::OwnedTmuxOrphanSelection
        | VerificationScenario::OwnedTmuxLiveSelection
        | VerificationScenario::InspectSwitchLatency => AgentKind::Verifier,
    }
}

fn focus_verification_agent_for_terminal(
    ctx: &mut VerificationScenarioContext,
    terminal_id: TerminalId,
) -> Option<crate::agents::AgentId> {
    let agent_id = ctx.runtime_index.agent_for_terminal(terminal_id)?;
    let mut focus_ctx = crate::app::FocusMutationContext {
        session: &mut ctx.app_session,
        projection: crate::app::FocusProjectionContext {
            agent_catalog: &ctx.agent_catalog,
            runtime_index: &ctx.runtime_index,
            owned_tmux_sessions: &ctx.owned_tmux_sessions,
            selection: &mut ctx.selection,
            active_terminal_content: &mut ctx.active_terminal_content,
            terminal_manager: &mut ctx.terminal_manager,
            focus_state: &mut ctx.focus_state,
            input_capture: &mut ctx.input_capture,
            view_state: &mut ctx.view_state,
            visibility_state: &mut ctx.visibility_state,
        },
        redraws: &mut ctx.redraws,
    };
    focus_agent_without_persist(agent_id, VisibilityMode::FocusedOnly, &mut focus_ctx);
    Some(agent_id)
}

fn focus_verification_owned_tmux(
    ctx: &mut VerificationScenarioContext,
    session_uid: &str,
) {
    let mut owned_tmux_ctx = crate::app::OwnedTmuxContext {
        app_session: &mut ctx.app_session,
        selection: &mut ctx.selection,
        agent_catalog: &ctx.agent_catalog,
        runtime_index: &ctx.runtime_index,
        terminal_manager: &mut ctx.terminal_manager,
        focus_state: &mut ctx.focus_state,
        input_capture: &mut ctx.input_capture,
        view_state: &mut ctx.view_state,
        visibility_state: &mut ctx.visibility_state,
        runtime_spawner: &ctx.runtime_spawner,
        owned_tmux_sessions: &mut ctx.owned_tmux_sessions,
        active_terminal_content: &mut ctx.active_terminal_content,
        redraws: &mut ctx.redraws,
    };
    crate::app::select_owned_tmux(session_uid, &mut owned_tmux_ctx);
}

fn clear_verification_ui(ctx: &mut VerificationScenarioContext) {
    clear_composer_and_direct_input(
        &mut ctx.app_session,
        &mut ctx.input_capture,
        &mut ctx.redraws,
    );
    ctx.agent_list_state.show_selected_context = false;
}

fn sync_verification_test_focus_state(_ctx: &mut VerificationScenarioContext) {
    #[cfg(test)]
    _ctx.terminal_manager
        .replace_test_focus_state(&_ctx.focus_state);
}

/// Starts a background worker that injects the configured auto-verify command after a delay.
///
/// The worker sleeps off-thread, logs the dispatch, sends the command through the terminal bridge,
/// and then wakes the runtime notifier so the app notices the newly queued command promptly.
pub(crate) fn start_auto_verify_dispatcher(
    bridge: TerminalBridge,
    notifier: RuntimeNotifier,
    config: AutoVerifyConfig,
) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(config.delay_ms));
        append_debug_log(format!(
            "auto-verify command dispatched: {}",
            config.command
        ));
        bridge.send(TerminalCommand::SendCommand(config.command));
        notifier.wake();
    });
}

#[derive(SystemParam)]
struct VerificationScenarioContext<'w> {
    terminal_manager: ResMut<'w, TerminalManager>,
    focus_state: ResMut<'w, TerminalFocusState>,
    presentation_store: ResMut<'w, TerminalPresentationStore>,
    verification_overrides: ResMut<'w, VerificationTerminalSurfaceOverrides>,
    runtime_spawner: Res<'w, TerminalRuntimeSpawner>,
    input_capture: ResMut<'w, HudInputCaptureState>,
    agent_list_state: ResMut<'w, crate::hud::AgentListUiState>,
    agent_catalog: ResMut<'w, AgentCatalog>,
    runtime_index: ResMut<'w, AgentRuntimeIndex>,
    owned_tmux_sessions: ResMut<'w, crate::terminals::OwnedTmuxSessionStore>,
    active_terminal_content: ResMut<'w, crate::terminals::ActiveTerminalContentState>,
    app_session: ResMut<'w, AppSessionState>,
    selection: ResMut<'w, AgentListSelection>,
    task_store: ResMut<'w, AgentTaskStore>,
    visibility_state: ResMut<'w, TerminalVisibilityState>,
    view_state: ResMut<'w, TerminalViewState>,
    redraws: MessageWriter<'w, RequestRedraw>,
}

/// Advances the deterministic verification-scenario state machine during update.
///
/// The system waits out the configured frame delay, spawns however many verifier terminals the
/// selected scenario needs, then mutates focus/visibility/modal/note state into the exact setup the
/// scenario expects. The inspect-switch scenario is special: it primes two terminals first and only
/// marks itself applied once both terminals have presentable uploaded frames, so the final capture
/// measures a real visual switch instead of a partially loaded one.
fn selected_agent_row_is_focused(
    selection: &AgentListSelection,
    agent_list: &AgentListView,
) -> bool {
    match selection {
        AgentListSelection::Agent(agent_id) => agent_list.rows.iter().any(|row| {
            row.focused && matches!(row.key, crate::hud::AgentListRowKey::Agent(row_id) if row_id == *agent_id)
        }),
        AgentListSelection::None | AgentListSelection::OwnedTmux(_) => false,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "verification readiness compares one scenario contract against multiple authoritative state stores"
)]
fn verification_capture_ready(
    scenario: VerificationScenario,
    app_session: &AppSessionState,
    selection: &AgentListSelection,
    agent_list: &AgentListView,
    agent_list_state: &crate::hud::AgentListUiState,
    active_terminal_content: &crate::terminals::ActiveTerminalContentState,
    focus_state: &TerminalFocusState,
    runtime_index: &AgentRuntimeIndex,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
    verification_overrides: &VerificationTerminalSurfaceOverrides,
    visual_contract: &VisualContractState,
) -> bool {
    let active_terminal_ready = focus_state.active_id().is_some_and(|terminal_id| {
        terminal_has_presentable_frame(
            terminal_id,
            terminal_manager,
            presentation_store,
            verification_overrides,
        )
    });
    let active_terminal_matches = |expected_activity| {
        focus_state.active_id().is_some_and(|terminal_id| {
            terminal_has_presentable_frame(
                terminal_id,
                terminal_manager,
                presentation_store,
                verification_overrides,
            ) && visual_contract.frame_for_terminal(terminal_id) == TerminalFrameVisualState::Hidden
                && runtime_index
                    .agent_for_terminal(terminal_id)
                    .is_some_and(|agent_id| {
                        visual_contract.activity_for_agent(agent_id) == expected_activity
                    })
        })
    };
    match scenario {
        VerificationScenario::MessageBoxBloom => {
            active_terminal_ready && app_session.composer.message_editor.visible
        }
        VerificationScenario::TaskDialogBloom => {
            active_terminal_ready && app_session.composer.task_editor.visible
        }
        VerificationScenario::AgentListBloom => {
            active_terminal_ready && selected_agent_row_is_focused(selection, agent_list)
        }
        VerificationScenario::AgentContextBloom => {
            active_terminal_ready
                && selected_agent_row_is_focused(selection, agent_list)
                && agent_list_state.show_selected_context
        }
        VerificationScenario::OwnedTmuxOrphanSelection => {
            !matches!(selection, AgentListSelection::OwnedTmux(_))
                && active_terminal_content.selected_owned_tmux_session_uid().is_none()
                && agent_list
                    .rows
                    .iter()
                    .all(|row| !matches!(row.key, crate::hud::AgentListRowKey::OwnedTmux(_)))
        }
        VerificationScenario::OwnedTmuxLiveSelection => {
            focus_state.active_id().is_some_and(|terminal_id| {
                terminal_readiness_for_id(
                    terminal_id,
                    terminal_manager,
                    presentation_store,
                    active_terminal_content.presentation_override_revision_for(terminal_id),
                )
                .is_ready_for_capture()
            }) && matches!(selection, AgentListSelection::OwnedTmux(_))
                && agent_list.rows.iter().any(|row| row.focused
                    && matches!(row.key, crate::hud::AgentListRowKey::OwnedTmux(_)))
                && active_terminal_content.last_error().is_none()
        }
        VerificationScenario::WorkingStateIdle => {
            active_terminal_matches(VisualAgentActivity::Idle)
        }
        VerificationScenario::WorkingStateWorking => {
            active_terminal_matches(VisualAgentActivity::Working)
        }
        VerificationScenario::InspectSwitchLatency => active_terminal_ready,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "verification capture readiness derives from scenario, app, HUD, terminal, and visual-contract state"
)]
pub(crate) fn sync_verification_capture_barrier(
    verification_scenario: Option<Res<VerificationScenarioConfig>>,
    app_session: Res<AppSessionState>,
    selection: Res<AgentListSelection>,
    agent_list: Res<AgentListView>,
    agent_list_state: Res<crate::hud::AgentListUiState>,
    active_terminal_content: Res<crate::terminals::ActiveTerminalContentState>,
    focus_state: Res<TerminalFocusState>,
    runtime_index: Res<AgentRuntimeIndex>,
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    verification_overrides: Res<VerificationTerminalSurfaceOverrides>,
    visual_contract: Res<VisualContractState>,
    mut barrier: ResMut<VerificationCaptureBarrierState>,
) {
    let ready = match verification_scenario {
        Some(scenario) => {
            scenario.applied
                && verification_capture_ready(
                    scenario.scenario,
                    &app_session,
                    &selection,
                    &agent_list,
                    &agent_list_state,
                    &active_terminal_content,
                    &focus_state,
                    &runtime_index,
                    &terminal_manager,
                    &presentation_store,
                    &verification_overrides,
                    &visual_contract,
                )
        }
        None => true,
    };
    if barrier.ready != ready {
        barrier.ready = ready;
    }
}

pub(crate) fn run_verification_scenario(world: &mut World) {
    let mut state: bevy::ecs::system::SystemState<(
        Option<ResMut<VerificationScenarioConfig>>,
        VerificationScenarioContext<'_>,
    )> = bevy::ecs::system::SystemState::new(world);
    let (config, mut ctx) = state.get_mut(world);
    macro_rules! finish {
        () => {{
            state.apply(world);
            return;
        }};
    }
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let Some(mut config) = config else {
        finish!();
    };
    if config.applied || !ctx.runtime_spawner.is_ready() {
        finish!();
    }
    if !config.primed && config.terminal_ids.is_empty() {
        ctx.verification_overrides.clear();
    }
    if config.frames_until_apply > 0 {
        config.frames_until_apply -= 1;
        ctx.redraws.write(RequestRedraw);
        finish!();
    }

    let required_terminals = match config.scenario {
        VerificationScenario::InspectSwitchLatency => 2,
        _ => 1,
    };
    while config.terminal_ids.len() < required_terminals {
        let (session_name, terminal_id, bridge) = match spawn_runtime_terminal_session(
            &mut crate::app::SpawnRuntimeTerminalSessionContext {
                terminal_manager: &mut ctx.terminal_manager,
                focus_state: &mut ctx.focus_state,
                runtime_spawner: &ctx.runtime_spawner,
            },
            crate::app::SpawnRuntimeTerminalSessionRequest {
                prefix: VERIFIER_SESSION_PREFIX,
                working_directory: None,
                startup_command: None,
                env_overrides: &[],
                focus: true,
            },
        ) {
            Ok(result) => result,
            Err(error) => {
                append_debug_log(format!("verification scenario spawn failed: {error}"));
                finish!();
            }
        };
        let label = match config.terminal_ids.len() {
            0 => "alpha",
            1 => "beta",
            _ => "gamma",
        };
        if ctx.runtime_index.agent_for_terminal(terminal_id).is_none() {
            let label = ctx
                .agent_catalog
                .validate_new_label(Some(label))
                .expect("verification labels must remain unique");
            let agent_kind = verification_agent_kind(config.scenario);
            let agent_id =
                ctx.agent_catalog
                    .create_agent(label, agent_kind, agent_kind.capabilities());
            let runtime = ctx
                .terminal_manager
                .get(terminal_id)
                .map(|terminal| &terminal.snapshot.runtime);
            ctx.runtime_index
                .link_terminal(agent_id, terminal_id, session_name.clone(), runtime);
        }
        bridge.send(TerminalCommand::SendCommand(format!(
            "clear; printf '__NZ_VERIFY_{}__\\n'",
            label.to_ascii_uppercase()
        )));
        config.terminal_ids.push(terminal_id);
    }

    match config.scenario {
        VerificationScenario::MessageBoxBloom => {
            let terminal_id = config.terminal_ids[0];
            if let Some(agent_id) = focus_verification_agent_for_terminal(&mut ctx, terminal_id) {
                open_composer(
                    &ComposerRequest {
                        mode: ComposerMode::Message { agent_id },
                    },
                    &mut ctx.app_session,
                    &mut ctx.input_capture,
                    &ctx.runtime_index,
                    &ctx.task_store,
                    &mut ctx.redraws,
                );
                ctx.app_session
                    .composer
                    .message_editor
                    .load_text("follow up");
            }
        }
        VerificationScenario::TaskDialogBloom => {
            let terminal_id = config.terminal_ids[0];
            let note_text = "- [ ] verify bloom layering\n- [ ] keep button text readable";
            if let Some(agent_id) = focus_verification_agent_for_terminal(&mut ctx, terminal_id) {
                let _ = ctx.task_store.set_text(agent_id, note_text);
                open_composer(
                    &ComposerRequest {
                        mode: ComposerMode::TaskEdit { agent_id },
                    },
                    &mut ctx.app_session,
                    &mut ctx.input_capture,
                    &ctx.runtime_index,
                    &ctx.task_store,
                    &mut ctx.redraws,
                );
            }
        }
        VerificationScenario::AgentListBloom => {
            let terminal_id = config.terminal_ids[0];
            let _ = focus_verification_agent_for_terminal(&mut ctx, terminal_id);
            clear_verification_ui(&mut ctx);
        }
        VerificationScenario::AgentContextBloom => {
            let terminal_id = config.terminal_ids[0];
            let _ = focus_verification_agent_for_terminal(&mut ctx, terminal_id);
            clear_verification_ui(&mut ctx);
            ctx.agent_list_state.show_selected_context = true;
        }
        VerificationScenario::OwnedTmuxOrphanSelection => {
            let terminal_id = config.terminal_ids[0];
            let _ = focus_verification_agent_for_terminal(&mut ctx, terminal_id);
            clear_verification_ui(&mut ctx);
            seed_terminal_surface(
                &mut ctx.verification_overrides,
                terminal_id,
                "OWNER",
                egui::Color32::from_rgb(132, 56, 44),
            );
            let _ = ctx
                .owned_tmux_sessions
                .replace_sessions(vec![crate::terminals::OwnedTmuxSessionInfo {
                    session_uid: "verify-owned-orphan".into(),
                    owner_agent_uid: "missing-agent".into(),
                    tmux_name: "verify-owned-orphan".into(),
                    display_name: "B-1".into(),
                    cwd: "/tmp/b1".into(),
                    attached: false,
                    created_unix: 0,
                }]);
            focus_verification_owned_tmux(&mut ctx, "verify-owned-orphan");
        }
        VerificationScenario::OwnedTmuxLiveSelection => {
            let terminal_id = config.terminal_ids[0];
            if let Some(agent_id) = focus_verification_agent_for_terminal(&mut ctx, terminal_id) {
                clear_verification_ui(&mut ctx);
                let Some(owner_agent_uid) = ctx.agent_catalog.uid(agent_id).map(str::to_owned) else {
                    finish!();
                };
                let session = match ctx.runtime_spawner.create_owned_tmux_session(
                    &owner_agent_uid,
                    "B-1",
                    None,
                    "printf \"TMUX VERIFY\\nTMUX VERIFY\\n\"",
                ) {
                    Ok(session) => session,
                    Err(error) => {
                        append_debug_log(format!("verification live tmux create failed: {error}"));
                        finish!();
                    }
                };
                let _ = ctx.owned_tmux_sessions.replace_sessions(vec![session.clone()]);
                focus_verification_owned_tmux(&mut ctx, &session.session_uid);
            }
        }
        VerificationScenario::WorkingStateIdle | VerificationScenario::WorkingStateWorking => {
            let terminal_id = config.terminal_ids[0];
            let _ = focus_verification_agent_for_terminal(&mut ctx, terminal_id);
            clear_verification_ui(&mut ctx);
            let working = matches!(config.scenario, VerificationScenario::WorkingStateWorking);
            ctx.verification_overrides
                .set_surface(terminal_id, seeded_activity_contract_surface(working));
        }
        VerificationScenario::InspectSwitchLatency => {
            let first = config.terminal_ids[0];
            let second = config.terminal_ids[1];
            clear_verification_ui(&mut ctx);
            if !config.primed {
                let _ = focus_verification_agent_for_terminal(&mut ctx, first);
                seed_terminal_surface(
                    &mut ctx.verification_overrides,
                    first,
                    "ALPHA",
                    egui::Color32::from_rgb(132, 56, 44),
                );
                seed_terminal_surface(
                    &mut ctx.verification_overrides,
                    second,
                    "BETA",
                    egui::Color32::from_rgb(44, 72, 140),
                );
                config.primed = true;
                sync_verification_test_focus_state(&mut ctx);
                ctx.redraws.write(RequestRedraw);
                finish!();
            }
            if !terminal_has_presentable_frame(
                first,
                &ctx.terminal_manager,
                &ctx.presentation_store,
                &ctx.verification_overrides,
            ) || !terminal_has_presentable_frame(
                second,
                &ctx.terminal_manager,
                &ctx.presentation_store,
                &ctx.verification_overrides,
            ) {
                ctx.redraws.write(RequestRedraw);
                finish!();
            }
            let _ = focus_verification_agent_for_terminal(&mut ctx, second);
        }
    }
    sync_verification_test_focus_state(&mut ctx);
    append_debug_log(format!(
        "verification scenario applied: {:?}",
        config.scenario
    ));
    config.applied = true;
    ctx.redraws.write(RequestRedraw);
    state.apply(world);
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{fake_runtime_spawner, insert_default_hud_resources, surface_with_text};
    use bevy::{
        ecs::system::RunSystemOnce,
        prelude::{Time, UVec2, Window},
        window::{PrimaryWindow, RequestRedraw},
    };
    use std::sync::Arc;

    fn init_verification_runtime_resources(world: &mut World) {
        world.init_resource::<Messages<RequestRedraw>>();
        world.spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            PrimaryWindow,
        ));
    }

    /// Covers the string parser for the built-in verification scenarios.
    ///
    /// The assertions verify that every public scenario name is accepted and that empty or missing input
    /// disables the feature by returning `None`.
    #[test]
    fn parses_verification_scenarios() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        assert_eq!(resolve_verification_scenario(None), None);
        assert_eq!(resolve_verification_scenario(Some("")), None);
        assert_eq!(
            resolve_verification_scenario(Some("message-box-bloom")),
            Some(VerificationScenario::MessageBoxBloom)
        );
        assert_eq!(
            resolve_verification_scenario(Some("task-dialog-bloom")),
            Some(VerificationScenario::TaskDialogBloom)
        );
        assert_eq!(
            resolve_verification_scenario(Some("agent-list-bloom")),
            Some(VerificationScenario::AgentListBloom)
        );
        assert_eq!(
            resolve_verification_scenario(Some("agent-context-bloom")),
            Some(VerificationScenario::AgentContextBloom)
        );
        assert_eq!(
            resolve_verification_scenario(Some("owned-tmux-orphan-selection")),
            Some(VerificationScenario::OwnedTmuxOrphanSelection)
        );
        assert_eq!(
            resolve_verification_scenario(Some("owned-tmux-live-selection")),
            Some(VerificationScenario::OwnedTmuxLiveSelection)
        );
        assert_eq!(
            resolve_verification_scenario(Some("working-state-idle")),
            Some(VerificationScenario::WorkingStateIdle)
        );
        assert_eq!(
            resolve_verification_scenario(Some("working-state-working")),
            Some(VerificationScenario::WorkingStateWorking)
        );
        assert_eq!(
            resolve_verification_scenario(Some("inspect-switch-latency")),
            Some(VerificationScenario::InspectSwitchLatency)
        );
    }

    /// Verifies the message-box verification scenario's first-application behavior.
    ///
    /// Running the scenario should spawn one verifier terminal, focus it, open the message-box modal, and
    /// seed the modal text with the deterministic payload used by the visual test.
    #[test]
    fn message_box_scenario_opens_modal_and_spawns_terminal() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let client = Arc::new(crate::tests::FakeDaemonClient::default());
        let mut world = World::default();
        world.insert_resource(VerificationScenarioConfig {
            scenario: VerificationScenario::MessageBoxBloom,
            frames_until_apply: 0,
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        });
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        world.insert_resource(crate::terminals::TerminalFocusState::default());
        world.insert_resource(crate::terminals::TerminalPresentationStore::default());
        world.insert_resource(fake_runtime_spawner(client));
        world.insert_resource(crate::agents::AgentCatalog::default());
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        world.insert_resource(crate::app::AppSessionState::default());
        world.insert_resource(crate::conversations::AgentTaskStore::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        world
            .resource_mut::<crate::hud::HudInputCaptureState>()
            .direct_input_terminal = Some(crate::terminals::TerminalId(777));
        init_verification_runtime_resources(&mut world);

        world.run_system_once(run_verification_scenario).unwrap();

        let app_session = world.resource::<crate::app::AppSessionState>();
        assert!(app_session.composer.message_editor.visible);
        assert_eq!(app_session.composer.message_editor.text, "follow up");
        let terminal_ids = world.resource::<TerminalManager>().terminal_ids().to_vec();
        assert_eq!(terminal_ids.len(), 1);
        let terminal_id = terminal_ids[0];
        let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
        let agent_id = runtime_index
            .agent_for_terminal(terminal_id)
            .expect("scenario should bind agent");
        assert_eq!(
            world
                .resource::<crate::terminals::TerminalFocusState>()
                .active_id(),
            Some(terminal_id)
        );
        assert_eq!(
            *world.resource::<crate::hud::AgentListSelection>(),
            crate::hud::AgentListSelection::Agent(agent_id)
        );
        assert_eq!(
            world
                .resource::<crate::hud::TerminalVisibilityState>()
                .policy,
            crate::hud::TerminalVisibilityPolicy::Isolate(terminal_id)
        );
        assert_eq!(
            world
                .resource::<crate::hud::HudInputCaptureState>()
                .direct_input_terminal,
            None
        );
        assert!(world.resource::<VerificationScenarioConfig>().applied);
    }

    /// Verifies the task-dialog verification scenario seeds the modal with deterministic note content.
    ///
    /// The scenario should open the task dialog for one spawned terminal and preload the text that the
    /// bloom verification capture expects to see.
    #[test]
    fn task_dialog_scenario_populates_note_text() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let client = Arc::new(crate::tests::FakeDaemonClient::default());
        let mut world = World::default();
        world.insert_resource(VerificationScenarioConfig {
            scenario: VerificationScenario::TaskDialogBloom,
            frames_until_apply: 0,
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        });
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        world.insert_resource(crate::terminals::TerminalFocusState::default());
        world.insert_resource(crate::terminals::TerminalPresentationStore::default());
        world.insert_resource(fake_runtime_spawner(client));
        world.insert_resource(crate::agents::AgentCatalog::default());
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        world.insert_resource(crate::app::AppSessionState::default());
        world.insert_resource(crate::conversations::AgentTaskStore::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        init_verification_runtime_resources(&mut world);

        world.run_system_once(run_verification_scenario).unwrap();

        let app_session = world.resource::<crate::app::AppSessionState>();
        assert!(app_session.composer.task_editor.visible);
        assert!(app_session
            .composer
            .task_editor
            .text
            .contains("verify bloom layering"));
        let terminal_ids = world.resource::<TerminalManager>().terminal_ids().to_vec();
        assert_eq!(terminal_ids.len(), 1);
        let terminal_id = terminal_ids[0];
        let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
        let agent_id = runtime_index
            .agent_for_terminal(terminal_id)
            .expect("scenario should bind agent");
        assert_eq!(
            *world.resource::<crate::hud::AgentListSelection>(),
            crate::hud::AgentListSelection::Agent(agent_id)
        );
        assert_eq!(
            world
                .resource::<crate::hud::TerminalVisibilityState>()
                .policy,
            crate::hud::TerminalVisibilityPolicy::Isolate(terminal_id)
        );
        let terminal_manager = world.resource::<TerminalManager>();
        let session_name = terminal_manager
            .get(terminal_id)
            .expect("scenario terminal should exist")
            .session_name
            .clone();
        assert_eq!(
            world
                .resource::<crate::terminals::TerminalNotesState>()
                .note_text(&session_name),
            None
        );
    }

    #[test]
    fn agent_list_scenario_clears_existing_composer_and_direct_input() {
        let client = Arc::new(crate::tests::FakeDaemonClient::default());
        let mut world = World::default();
        world.insert_resource(VerificationScenarioConfig {
            scenario: VerificationScenario::AgentListBloom,
            frames_until_apply: 0,
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        });
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        world.insert_resource(crate::terminals::TerminalFocusState::default());
        world.insert_resource(crate::terminals::TerminalPresentationStore::default());
        world.insert_resource(fake_runtime_spawner(client));
        world.insert_resource(crate::agents::AgentCatalog::default());
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        let mut app_session = crate::app::AppSessionState::default();
        app_session
            .composer
            .open_message(crate::agents::AgentId(999));
        world.insert_resource(app_session);
        world.insert_resource(crate::conversations::AgentTaskStore::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        world
            .resource_mut::<crate::hud::HudInputCaptureState>()
            .direct_input_terminal = Some(crate::terminals::TerminalId(777));
        init_verification_runtime_resources(&mut world);

        world.run_system_once(run_verification_scenario).unwrap();

        let app_session = world.resource::<crate::app::AppSessionState>();
        assert!(!app_session.composer.message_editor.visible);
        assert!(!app_session.composer.task_editor.visible);
        assert_eq!(
            world
                .resource::<crate::hud::HudInputCaptureState>()
                .direct_input_terminal,
            None
        );
        let terminal_id = world.resource::<TerminalManager>().terminal_ids()[0];
        let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
        let agent_id = runtime_index
            .agent_for_terminal(terminal_id)
            .expect("scenario should bind agent");
        assert_eq!(
            *world.resource::<crate::hud::AgentListSelection>(),
            crate::hud::AgentListSelection::Agent(agent_id)
        );
    }

    #[test]
    fn agent_context_scenario_enables_selected_context_overlay() {
        let client = Arc::new(crate::tests::FakeDaemonClient::default());
        let mut world = World::default();
        world.insert_resource(VerificationScenarioConfig {
            scenario: VerificationScenario::AgentContextBloom,
            frames_until_apply: 0,
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        });
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        world.insert_resource(crate::terminals::TerminalFocusState::default());
        world.insert_resource(crate::terminals::TerminalPresentationStore::default());
        world.insert_resource(fake_runtime_spawner(client));
        world.insert_resource(crate::agents::AgentCatalog::default());
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        world.insert_resource(crate::app::AppSessionState::default());
        world.insert_resource(crate::conversations::AgentTaskStore::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        init_verification_runtime_resources(&mut world);

        world.run_system_once(run_verification_scenario).unwrap();

        assert!(
            world
                .resource::<crate::hud::AgentListUiState>()
                .show_selected_context
        );
        let terminal_id = world.resource::<TerminalManager>().terminal_ids()[0];
        let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
        let agent_id = runtime_index
            .agent_for_terminal(terminal_id)
            .expect("scenario should bind agent");
        assert_eq!(
            *world.resource::<crate::hud::AgentListSelection>(),
            crate::hud::AgentListSelection::Agent(agent_id)
        );
    }

    #[test]
    fn working_state_scenario_seeds_pi_agent_with_working_surface() {
        let client = Arc::new(crate::tests::FakeDaemonClient::default());
        let mut world = World::default();
        world.insert_resource(VerificationScenarioConfig {
            scenario: VerificationScenario::WorkingStateWorking,
            frames_until_apply: 0,
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        });
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        world.insert_resource(crate::terminals::TerminalFocusState::default());
        world.insert_resource(crate::terminals::TerminalPresentationStore::default());
        world.insert_resource(fake_runtime_spawner(client));
        world.insert_resource(crate::agents::AgentCatalog::default());
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        world.insert_resource(crate::app::AppSessionState::default());
        world.insert_resource(crate::conversations::AgentTaskStore::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        init_verification_runtime_resources(&mut world);

        world.run_system_once(run_verification_scenario).unwrap();

        let terminal_ids = world.resource::<TerminalManager>().terminal_ids().to_vec();
        assert_eq!(terminal_ids.len(), 1);
        let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
        let agent_catalog = world.resource::<crate::agents::AgentCatalog>();
        let agent_id = runtime_index
            .agent_for_terminal(terminal_ids[0])
            .expect("scenario should bind agent");
        assert_eq!(
            agent_catalog.kind(agent_id),
            Some(crate::agents::AgentKind::Pi)
        );
        let surface = world
            .resource::<crate::verification::VerificationTerminalSurfaceOverrides>()
            .surface_for(terminal_ids[0])
            .expect("scenario should seed a verification override surface");
        assert!(surface_with_text(8, 120, 0, "header").rows <= surface.rows);
        assert_eq!(surface.cell(1, 3).content.to_owned_string(), "⠋ Working...");
    }

    #[test]
    fn working_state_capture_barrier_waits_for_presented_visual_contract() {
        let client = Arc::new(crate::tests::FakeDaemonClient::default());
        let mut world = World::default();
        world.insert_resource(VerificationScenarioConfig {
            scenario: VerificationScenario::WorkingStateWorking,
            frames_until_apply: 0,
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        });
        world.insert_resource(crate::verification::VerificationCaptureBarrierState::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        world.insert_resource(crate::terminals::TerminalFocusState::default());
        world.insert_resource(crate::terminals::TerminalPresentationStore::default());
        world.insert_resource(fake_runtime_spawner(client));
        world.insert_resource(crate::agents::AgentCatalog::default());
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        world.insert_resource(crate::agents::AgentStatusStore::default());
        world.insert_resource(Time::<()>::default());
        world.insert_resource(crate::app::AppSessionState::default());
        world.insert_resource(crate::conversations::AgentTaskStore::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        init_verification_runtime_resources(&mut world);

        world.run_system_once(run_verification_scenario).unwrap();
        world
            .run_system_once(crate::terminals::sync_terminal_projection_entities)
            .unwrap();
        world
            .run_system_once(sync_verification_capture_barrier)
            .unwrap();
        assert!(
            !world
                .resource::<crate::verification::VerificationCaptureBarrierState>()
                .ready(),
            "barrier must stay closed before status derivation and uploaded presentation agree"
        );

        let terminal_id = world.resource::<TerminalManager>().terminal_ids()[0];
        {
            let override_revision = world
                .resource::<crate::verification::VerificationTerminalSurfaceOverrides>()
                .presentation_override_revision_for(terminal_id)
                .expect("working-state scenario should install a verification override surface");
            let mut presentations = world.resource_mut::<crate::terminals::TerminalPresentationStore>();
            let presented = presentations
                .get_mut(terminal_id)
                .expect("projection should exist");
            presented.uploaded_active_override_revision = Some(override_revision);
            presented.texture_state = crate::terminals::TerminalTextureState {
                texture_size: UVec2::new(960, 160),
                cell_size: UVec2::new(8, 20),
            };
        }

        world
            .run_system_once(crate::agents::sync_agent_status)
            .unwrap();
        world
            .run_system_once(crate::visual_contract::sync_visual_contract_state)
            .unwrap();
        world
            .run_system_once(sync_verification_capture_barrier)
            .unwrap();

        assert!(
            world
                .resource::<crate::verification::VerificationCaptureBarrierState>()
                .ready(),
            "barrier should open only after scenario surface, derived status, visual contract, and uploaded frame agree"
        );
    }

    /// Verifies the two-phase behavior of the inspect-switch-latency scenario.
    ///
    /// On the first run the scenario should spawn and prime two terminals but remain unapplied until both
    /// have presentable uploaded frames. Once those frames are injected, the second run should focus the
    /// second terminal and mark the scenario as applied.
    #[test]
    fn inspect_switch_scenario_spawns_two_terminals_and_focuses_second() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let client = Arc::new(crate::tests::FakeDaemonClient::default());
        let mut world = World::default();
        world.insert_resource(VerificationScenarioConfig {
            scenario: VerificationScenario::InspectSwitchLatency,
            frames_until_apply: 0,
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        });
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        world.insert_resource(crate::terminals::TerminalFocusState::default());
        world.insert_resource(crate::terminals::TerminalPresentationStore::default());
        world.insert_resource(fake_runtime_spawner(client));
        world.insert_resource(crate::agents::AgentCatalog::default());
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        world.insert_resource(crate::app::AppSessionState::default());
        world.insert_resource(crate::conversations::AgentTaskStore::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        init_verification_runtime_resources(&mut world);

        world.run_system_once(run_verification_scenario).unwrap();
        world
            .run_system_once(crate::terminals::sync_terminal_projection_entities)
            .unwrap();

        let terminal_ids = world.resource::<TerminalManager>().terminal_ids().to_vec();
        assert_eq!(terminal_ids.len(), 2);
        assert!(world.resource::<VerificationScenarioConfig>().primed);
        assert!(!world.resource::<VerificationScenarioConfig>().applied);
        let override_revision = world
            .resource::<crate::verification::VerificationTerminalSurfaceOverrides>()
            .presentation_override_revision_for(terminal_ids[0])
            .expect("primed inspect-switch scenario should install override surfaces");
        {
            let mut presentations = world.resource_mut::<crate::terminals::TerminalPresentationStore>();
            for terminal_id in &terminal_ids {
                let presented = presentations.get_mut(*terminal_id).unwrap();
                presented.uploaded_active_override_revision = Some(override_revision);
                presented.texture_state = crate::terminals::TerminalTextureState {
                    texture_size: UVec2::new(320, 120),
                    cell_size: UVec2::new(8, 16),
                };
            }
        }

        world.run_system_once(run_verification_scenario).unwrap();

        let focus_state = world.resource::<crate::terminals::TerminalFocusState>();
        assert_eq!(focus_state.active_id(), terminal_ids.get(1).copied());
        assert!(world.resource::<VerificationScenarioConfig>().applied);
    }
}
