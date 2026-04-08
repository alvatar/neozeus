use crate::{
    agents::{AgentCatalog, AgentKind, AgentRuntimeIndex},
    app::AppSessionState,
    conversations::AgentTaskStore,
    hud::{
        AgentListSelection, AgentListView, HudInputCaptureState, TerminalVisibilityPolicy,
        TerminalVisibilityState,
    },
    terminals::{
        append_debug_log, attach_terminal_session, terminal_readiness_for_id, RuntimeNotifier,
        TerminalBridge, TerminalCell, TerminalCellContent, TerminalCommand, TerminalFocusState,
        TerminalId, TerminalManager, TerminalNotesState, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalSurface, TerminalViewState, VERIFIER_SESSION_PREFIX,
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
    agent_catalog: ResMut<'w, AgentCatalog>,
    runtime_index: ResMut<'w, AgentRuntimeIndex>,
    app_session: ResMut<'w, AppSessionState>,
    selection: ResMut<'w, AgentListSelection>,
    task_store: ResMut<'w, AgentTaskStore>,
    visibility_state: ResMut<'w, TerminalVisibilityState>,
    view_state: ResMut<'w, TerminalViewState>,
    notes_state: ResMut<'w, TerminalNotesState>,
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
        VerificationScenarioContext,
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
        let session_name = match ctx
            .runtime_spawner
            .create_session(VERIFIER_SESSION_PREFIX, None)
        {
            Ok(session_name) => session_name,
            Err(error) => {
                append_debug_log(format!("verification scenario spawn failed: {error}"));
                finish!();
            }
        };
        let (terminal_id, bridge) = match attach_terminal_session(
            &mut ctx.terminal_manager,
            &mut ctx.focus_state,
            &ctx.runtime_spawner,
            session_name.clone(),
            true,
        ) {
            Ok(result) => result,
            Err(error) => {
                append_debug_log(format!(
                    "verification scenario attach failed for {}: {error}",
                    session_name
                ));
                let _ = ctx.runtime_spawner.kill_session(&session_name);
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
            let agent_kind = match config.scenario {
                VerificationScenario::WorkingStateIdle
                | VerificationScenario::WorkingStateWorking => AgentKind::Pi,
                _ => AgentKind::Verifier,
            };
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
            ctx.focus_state
                .focus_terminal(&ctx.terminal_manager, terminal_id);
            ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
            ctx.view_state.focus_terminal(Some(terminal_id));
            if let Some(agent_id) = ctx.runtime_index.agent_for_terminal(terminal_id) {
                *ctx.selection = AgentListSelection::Agent(agent_id);
                ctx.input_capture.close_direct_terminal_input();
                ctx.app_session.composer.open_message(agent_id);
                ctx.app_session
                    .composer
                    .message_editor
                    .load_text("follow up");
            }
        }
        VerificationScenario::TaskDialogBloom => {
            let terminal_id = config.terminal_ids[0];
            ctx.focus_state
                .focus_terminal(&ctx.terminal_manager, terminal_id);
            ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
            ctx.view_state.focus_terminal(Some(terminal_id));
            let Some(session_name) = ctx
                .terminal_manager
                .get(terminal_id)
                .map(|terminal| terminal.session_name.clone())
            else {
                finish!();
            };
            let note_text = "- [ ] verify bloom layering\n- [ ] keep button text readable";
            let _ = ctx.notes_state.set_note_text(&session_name, note_text);
            if let Some(agent_id) = ctx.runtime_index.agent_for_terminal(terminal_id) {
                *ctx.selection = AgentListSelection::Agent(agent_id);
                let _ = ctx.task_store.set_text(agent_id, note_text);
                ctx.input_capture.close_direct_terminal_input();
                ctx.app_session
                    .composer
                    .open_task_editor(agent_id, note_text);
            }
        }
        VerificationScenario::AgentListBloom => {
            let terminal_id = config.terminal_ids[0];
            ctx.focus_state
                .focus_terminal(&ctx.terminal_manager, terminal_id);
            ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
            ctx.view_state.focus_terminal(Some(terminal_id));
            if let Some(agent_id) = ctx.runtime_index.agent_for_terminal(terminal_id) {
                *ctx.selection = AgentListSelection::Agent(agent_id);
            }
            ctx.app_session.composer.discard_current_message();
            ctx.app_session.composer.close_task_editor();
            ctx.input_capture.close_direct_terminal_input();
        }
        VerificationScenario::WorkingStateIdle | VerificationScenario::WorkingStateWorking => {
            let terminal_id = config.terminal_ids[0];
            ctx.focus_state
                .focus_terminal(&ctx.terminal_manager, terminal_id);
            ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
            ctx.view_state.focus_terminal(Some(terminal_id));
            if let Some(agent_id) = ctx.runtime_index.agent_for_terminal(terminal_id) {
                *ctx.selection = AgentListSelection::Agent(agent_id);
            }
            ctx.app_session.composer.discard_current_message();
            ctx.app_session.composer.close_task_editor();
            ctx.input_capture.close_direct_terminal_input();
            let working = matches!(config.scenario, VerificationScenario::WorkingStateWorking);
            ctx.verification_overrides
                .set_surface(terminal_id, seeded_activity_contract_surface(working));
        }
        VerificationScenario::InspectSwitchLatency => {
            let first = config.terminal_ids[0];
            let second = config.terminal_ids[1];
            ctx.app_session.composer.discard_current_message();
            ctx.app_session.composer.close_task_editor();
            ctx.input_capture.close_direct_terminal_input();
            if !config.primed {
                ctx.focus_state.focus_terminal(&ctx.terminal_manager, first);
                ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(first);
                ctx.view_state.focus_terminal(Some(first));
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
                #[cfg(test)]
                ctx.terminal_manager
                    .replace_test_focus_state(&ctx.focus_state);
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
            ctx.focus_state
                .focus_terminal(&ctx.terminal_manager, second);
            ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(second);
            ctx.view_state.focus_terminal(Some(second));
        }
    }
    #[cfg(test)]
    ctx.terminal_manager
        .replace_test_focus_state(&ctx.focus_state);
    append_debug_log(format!(
        "verification scenario applied: {:?}",
        config.scenario
    ));
    config.applied = true;
    ctx.redraws.write(RequestRedraw);
    state.apply(world);
}

#[cfg(test)]
mod tests;
