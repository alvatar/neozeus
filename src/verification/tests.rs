use super::*;
use crate::tests::{fake_runtime_spawner, insert_default_hud_resources, surface_with_text};
use bevy::{ecs::system::RunSystemOnce, window::RequestRedraw};
use std::sync::Arc;

/// Covers the string parser for the built-in verification scenarios.
///
/// The assertions verify that every public scenario name is accepted and that empty or missing input
/// disables the feature by returning `None`.
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

/// Verifies the message-box verification scenario's first-application behavior.
///
/// Running the scenario should spawn one verifier terminal, focus it, open the message-box modal, and
/// seed the modal text with the deterministic payload used by the visual test.
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
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(run_verification_scenario).unwrap();

    let app_session = world.resource::<crate::app::AppSessionState>();
    assert!(app_session.composer.message_editor.visible);
    assert_eq!(app_session.composer.message_editor.text, "follow up");
    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
    assert!(world.resource::<VerificationScenarioConfig>().applied);
}

/// Verifies the task-dialog verification scenario seeds the modal with deterministic note content.
///
/// The scenario should open the task dialog for one spawned terminal and preload the text that the
/// bloom verification capture expects to see.
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
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(run_verification_scenario).unwrap();

    let app_session = world.resource::<crate::app::AppSessionState>();
    assert!(app_session.composer.task_editor.visible);
    assert!(app_session
        .composer
        .task_editor
        .text
        .contains("verify bloom layering"));
    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
}

/// Verifies the two-phase behavior of the inspect-switch-latency scenario.
///
/// On the first run the scenario should spawn and prime two terminals but remain unapplied until both
/// have presentable uploaded frames. Once those frames are injected, the second run should focus the
/// second terminal and mark the scenario as applied.
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
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(run_verification_scenario).unwrap();
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

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
        let mut presentations = world.resource_mut::<crate::terminals::TerminalPresentationStore>();
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
