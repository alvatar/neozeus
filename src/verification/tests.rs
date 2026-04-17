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
