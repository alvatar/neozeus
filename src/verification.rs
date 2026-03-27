use crate::{
    hud::{
        AgentDirectory, HudInputCaptureState, HudModalState, TerminalVisibilityPolicy,
        TerminalVisibilityState,
    },
    terminals::{
        append_debug_log, spawn_attached_terminal_with_presentation, RuntimeNotifier,
        TerminalBridge, TerminalCell, TerminalCellContent, TerminalCommand, TerminalFocusState,
        TerminalId, TerminalManager, TerminalNotesState, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalSurface, TerminalViewState, VERIFIER_SESSION_PREFIX,
    },
};
use bevy::{prelude::Resource, prelude::*, window::RequestRedraw};
use bevy_egui::egui;
use std::{env, thread, time::Duration};

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
    InspectSwitchLatency,
}

/// Parses the named built-in verification scenario from an optional raw string.
///
/// The parser accepts the small fixed scenario vocabulary used by the offscreen verification scripts
/// and returns `None` for missing or unknown names so callers can treat the feature as disabled.
pub(crate) fn resolve_verification_scenario(raw: Option<&str>) -> Option<VerificationScenario> {
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
///
/// The check deliberately combines three conditions: the terminal must own a surface snapshot, the
/// presentation store must have uploaded the same surface revision, and the uploaded texture state
/// must be something more meaningful than the placeholder `1x1`/zero-cell bootstrap values.
fn terminal_has_presentable_frame(
    terminal_id: TerminalId,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
) -> bool {
    let Some(terminal) = terminal_manager.get(terminal_id) else {
        return false;
    };
    let Some(presented) = presentation_store.get(terminal_id) else {
        return false;
    };
    terminal.snapshot.surface.is_some()
        && presented.uploaded_revision == terminal.surface_revision
        && presented.texture_state.texture_size != UVec2::ONE
        && presented.texture_state.cell_size != UVec2::ZERO
}

/// Builds a deterministic synthetic terminal surface used by verification scenarios.
///
/// The surface is filled with repeated labeled bands on several rows so image captures have stable,
/// high-contrast content that makes focus changes and bloom behavior visually obvious.
fn seeded_inspect_surface(label: &str, accent: egui::Color32) -> TerminalSurface {
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
                        width: 1,
                    },
                );
                x += 1;
            }
        }
    }
    surface
}

/// Overwrites one managed terminal's snapshot surface with deterministic verification content.
///
/// The helper mutates the terminal in place and bumps its surface revision so the raster/presentation
/// pipeline treats the injected surface as fresh work that must be uploaded.
fn seed_terminal_surface(
    terminal_manager: &mut TerminalManager,
    terminal_id: TerminalId,
    label: &str,
    accent: egui::Color32,
) {
    let Some(terminal) = terminal_manager.get_mut(terminal_id) else {
        return;
    };
    terminal.snapshot.surface = Some(seeded_inspect_surface(label, accent));
    terminal.surface_revision += 1;
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

#[allow(
    clippy::too_many_arguments,
    reason = "verification scenario setup spans terminal spawn, HUD modal state, notes, and visibility"
)]
/// Advances the deterministic verification-scenario state machine during update.
///
/// The system waits out the configured frame delay, spawns however many verifier terminals the
/// selected scenario needs, then mutates focus/visibility/modal/note state into the exact setup the
/// scenario expects. The inspect-switch scenario is special: it primes two terminals first and only
/// marks itself applied once both terminals have presentable uploaded frames, so the final capture
/// measures a real visual switch instead of a partially loaded one.
pub(crate) fn run_verification_scenario(
    config: Option<ResMut<VerificationScenarioConfig>>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut focus_state: ResMut<TerminalFocusState>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    mut modal_state: ResMut<HudModalState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut agent_directory: ResMut<AgentDirectory>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut view_state: ResMut<TerminalViewState>,
    mut notes_state: ResMut<TerminalNotesState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let Some(mut config) = config else {
        return;
    };
    if config.applied {
        return;
    }
    if config.frames_until_apply > 0 {
        config.frames_until_apply -= 1;
        redraws.write(RequestRedraw);
        return;
    }

    let required_terminals = match config.scenario {
        VerificationScenario::InspectSwitchLatency => 2,
        _ => 1,
    };
    while config.terminal_ids.len() < required_terminals {
        let session_name = match runtime_spawner.create_session(VERIFIER_SESSION_PREFIX) {
            Ok(session_name) => session_name,
            Err(error) => {
                append_debug_log(format!("verification scenario spawn failed: {error}"));
                return;
            }
        };
        let (terminal_id, bridge) = match spawn_attached_terminal_with_presentation(
            &mut commands,
            &mut images,
            &mut terminal_manager,
            &mut focus_state,
            &mut presentation_store,
            &runtime_spawner,
            session_name.clone(),
            true,
        ) {
            Ok(result) => result,
            Err(error) => {
                append_debug_log(format!(
                    "verification scenario attach failed for {}: {error}",
                    session_name
                ));
                let _ = runtime_spawner.kill_session(&session_name);
                return;
            }
        };
        let label = match config.terminal_ids.len() {
            0 => "alpha",
            1 => "beta",
            _ => "gamma",
        };
        agent_directory.labels.insert(terminal_id, label.to_owned());
        bridge.send(TerminalCommand::SendCommand(format!(
            "clear; printf '__NZ_VERIFY_{}__\\n'",
            label.to_ascii_uppercase()
        )));
        config.terminal_ids.push(terminal_id);
    }

    match config.scenario {
        VerificationScenario::MessageBoxBloom => {
            let terminal_id = config.terminal_ids[0];
            focus_state.focus_terminal(&terminal_manager, terminal_id);
            visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
            view_state.focus_terminal(Some(terminal_id));
            modal_state.open_message_box(&mut input_capture, terminal_id);
            modal_state.message_box.load_text("follow up");
        }
        VerificationScenario::TaskDialogBloom => {
            let terminal_id = config.terminal_ids[0];
            focus_state.focus_terminal(&terminal_manager, terminal_id);
            visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
            view_state.focus_terminal(Some(terminal_id));
            let Some(session_name) = terminal_manager
                .get(terminal_id)
                .map(|terminal| terminal.session_name.clone())
            else {
                return;
            };
            let note_text = "- [ ] verify bloom layering\n- [ ] keep button text readable";
            let _ = notes_state.set_note_text(&session_name, note_text);
            modal_state.open_task_dialog(&mut input_capture, terminal_id, note_text);
        }
        VerificationScenario::AgentListBloom => {
            let terminal_id = config.terminal_ids[0];
            focus_state.focus_terminal(&terminal_manager, terminal_id);
            visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
            view_state.focus_terminal(Some(terminal_id));
            modal_state.close_message_box();
            modal_state.close_task_dialog();
            input_capture.close_direct_terminal_input();
        }
        VerificationScenario::InspectSwitchLatency => {
            let first = config.terminal_ids[0];
            let second = config.terminal_ids[1];
            modal_state.close_message_box();
            modal_state.close_task_dialog();
            input_capture.close_direct_terminal_input();
            if !config.primed {
                focus_state.focus_terminal(&terminal_manager, first);
                visibility_state.policy = TerminalVisibilityPolicy::Isolate(first);
                view_state.focus_terminal(Some(first));
                seed_terminal_surface(
                    &mut terminal_manager,
                    first,
                    "ALPHA",
                    egui::Color32::from_rgb(132, 56, 44),
                );
                seed_terminal_surface(
                    &mut terminal_manager,
                    second,
                    "BETA",
                    egui::Color32::from_rgb(44, 72, 140),
                );
                config.primed = true;
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(&focus_state);
                redraws.write(RequestRedraw);
                return;
            }
            if !terminal_has_presentable_frame(first, &terminal_manager, &presentation_store)
                || !terminal_has_presentable_frame(second, &terminal_manager, &presentation_store)
            {
                redraws.write(RequestRedraw);
                return;
            }
            focus_state.focus_terminal(&terminal_manager, second);
            visibility_state.policy = TerminalVisibilityPolicy::Isolate(second);
            view_state.focus_terminal(Some(second));
        }
    }
    #[cfg(test)]
    terminal_manager.replace_test_focus_state(&focus_state);
    append_debug_log(format!(
        "verification scenario applied: {:?}",
        config.scenario
    ));
    config.applied = true;
    redraws.write(RequestRedraw);
}

#[cfg(test)]
mod tests;
