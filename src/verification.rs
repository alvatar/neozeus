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
mod tests {
    use super::*;
    use crate::tests::{fake_runtime_spawner, insert_default_hud_resources, surface_with_text};
    use bevy::{ecs::system::RunSystemOnce, window::RequestRedraw};
    use std::sync::Arc;

    #[test]
    fn parses_verification_scenarios() {
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
            resolve_verification_scenario(Some("inspect-switch-latency")),
            Some(VerificationScenario::InspectSwitchLatency)
        );
    }

    #[test]
    fn message_box_scenario_opens_modal_and_spawns_terminal() {
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
        world.insert_resource(crate::hud::AgentDirectory::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(run_verification_scenario).unwrap();

        let modal_state = world.resource::<crate::hud::HudModalState>();
        assert!(modal_state.message_box.visible);
        assert_eq!(modal_state.message_box.text, "follow up");
        assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
        assert!(world.resource::<VerificationScenarioConfig>().applied);
    }

    #[test]
    fn task_dialog_scenario_populates_note_text() {
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
        world.insert_resource(crate::hud::AgentDirectory::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(run_verification_scenario).unwrap();

        let modal_state = world.resource::<crate::hud::HudModalState>();
        assert!(modal_state.task_dialog.visible);
        assert!(modal_state
            .task_dialog
            .text
            .contains("verify bloom layering"));
        assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
    }

    #[test]
    fn inspect_switch_scenario_spawns_two_terminals_and_focuses_second() {
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
        world.insert_resource(crate::hud::AgentDirectory::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::TerminalViewState::default());
        world.insert_resource(crate::terminals::TerminalNotesState::default());
        insert_default_hud_resources(&mut world);
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(run_verification_scenario).unwrap();

        let terminal_ids = world.resource::<TerminalManager>().terminal_ids().to_vec();
        assert_eq!(terminal_ids.len(), 2);
        assert!(world.resource::<VerificationScenarioConfig>().primed);
        assert!(!world.resource::<VerificationScenarioConfig>().applied);
        {
            let mut manager = world.resource_mut::<TerminalManager>();
            manager.get_mut(terminal_ids[0]).unwrap().snapshot.surface =
                Some(surface_with_text(2, 24, 0, "FIRST"));
            manager.get_mut(terminal_ids[0]).unwrap().surface_revision = 1;
            manager.get_mut(terminal_ids[1]).unwrap().snapshot.surface =
                Some(surface_with_text(2, 24, 0, "SECOND"));
            manager.get_mut(terminal_ids[1]).unwrap().surface_revision = 1;
        }
        {
            let mut presentations =
                world.resource_mut::<crate::terminals::TerminalPresentationStore>();
            for terminal_id in &terminal_ids {
                let presented = presentations.get_mut(*terminal_id).unwrap();
                presented.uploaded_revision = 1;
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
