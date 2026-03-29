use super::{
    fake_runtime_spawner, insert_default_hud_resources, insert_terminal_manager_resources,
    insert_test_hud_state, pressed_text, snapshot_test_hud_state, test_bridge, FakeDaemonClient,
};
use crate::terminals::{
    kill_active_terminal_session_and_remove as kill_active_terminal, TerminalManager,
    TerminalNotesState, TerminalPanel, TerminalPanelFrame, TerminalPresentationStore,
    TerminalViewState,
};
use crate::{
    app::{
        AgentCommand as AppAgentCommand, AppCommand, AppSessionState, AppStatePersistenceState,
        ComposerCommand as AppComposerCommand, CreateAgentDialogField,
        CreateAgentKind as AppCreateAgentKind, TaskCommand as AppTaskCommand, WidgetCommand,
    },
    composer::{
        create_agent_name_field_rect, message_box_action_buttons, message_box_rect,
        task_dialog_action_buttons,
    },
    hud::{
        handle_hud_module_shortcuts, handle_hud_pointer_input, AgentListDragState,
        AgentListUiState, AgentListView, HudDragState, HudRect, HudState, HudWidgetKey,
        TerminalVisibilityPolicy, TerminalVisibilityState,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{keyboard::KeyboardInput, mouse::MouseWheel},
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use std::{sync::Arc, time::Duration};

/// Initializes the app-command message resource in a test world.
fn init_hud_commands(world: &mut World) {
    world.init_resource::<Messages<AppCommand>>();
}

/// Drains queued app commands from a test world.
fn drain_hud_commands(world: &mut World) -> Vec<AppCommand> {
    world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap()
}

/// Handles run app commands.
fn run_app_commands(world: &mut World) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if !world.contains_resource::<Time<()>>() {
        world.insert_resource(Time::<()>::default());
    }
    if !world.contains_resource::<Assets<Image>>() {
        world.insert_resource(Assets::<Image>::default());
    }
    if !world.contains_resource::<TerminalPresentationStore>() {
        world.insert_resource(TerminalPresentationStore::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalRuntimeSpawner>() {
        world.insert_resource(fake_runtime_spawner(Arc::new(FakeDaemonClient::default())));
    }
    if !world.contains_resource::<crate::conversations::ConversationStore>() {
        world.insert_resource(crate::conversations::ConversationStore::default());
    }
    if !world.contains_resource::<crate::conversations::AgentTaskStore>() {
        world.insert_resource(crate::conversations::AgentTaskStore::default());
    }
    if !world.contains_resource::<crate::conversations::ConversationPersistenceState>() {
        world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    }
    if !world.contains_resource::<crate::conversations::MessageTransportAdapter>() {
        world.insert_resource(crate::conversations::MessageTransportAdapter);
    }
    if !world.contains_resource::<TerminalNotesState>() {
        world.insert_resource(TerminalNotesState::default());
    }
    if !world.contains_resource::<AppStatePersistenceState>() {
        world.insert_resource(AppStatePersistenceState::default());
    }
    if !world.contains_resource::<TerminalVisibilityState>() {
        world.insert_resource(TerminalVisibilityState::default());
    }
    if !world.contains_resource::<TerminalViewState>() {
        world.insert_resource(TerminalViewState::default());
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    crate::app::run_apply_app_commands(world);
    world
        .run_system_once(crate::conversations::sync_task_notes_projection)
        .unwrap();
}

/// Verifies that widget toggles snap visibility immediately instead of fading over later animation
/// ticks.
#[test]
fn toggling_widgets_snaps_alpha_immediately() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    hud_state.insert_default_module(HudWidgetKey::ConversationList);
    let conversation_rect = HudRect {
        x: 332.0,
        y: 112.0,
        w: 320.0,
        h: 320.0,
    };
    hud_state.set_module_shell_state(
        HudWidgetKey::ConversationList,
        true,
        conversation_rect,
        conversation_rect,
        1.0,
        1.0,
    );
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::AgentList,
        )));
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::ConversationList,
        )));
    run_app_commands(&mut world);

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(
        hud_state.module_enabled(HudWidgetKey::AgentList),
        Some(false)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::AgentList),
        Some(0.0)
    );
    assert_eq!(
        hud_state.module_enabled(HudWidgetKey::ConversationList),
        Some(false)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::ConversationList),
        Some(0.0)
    );

    world.insert_resource(Messages::<AppCommand>::default());
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::AgentList,
        )));
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::ConversationList,
        )));
    run_app_commands(&mut world);

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(
        hud_state.module_enabled(HudWidgetKey::AgentList),
        Some(true)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::AgentList),
        Some(1.0)
    );
    assert_eq!(
        hud_state.module_enabled(HudWidgetKey::ConversationList),
        Some(true)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::ConversationList),
        Some(1.0)
    );
}

/// Verifies that resetting a HUD module restores the baked-in default shell state instead of merely
/// toggling enablement.
#[test]
fn reset_module_restores_default_toolbar_state() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut hud_state = HudState::default();
    let rect = HudRect {
        x: 1800.0,
        y: 1200.0,
        w: 10.0,
        h: 10.0,
    };
    hud_state.insert_default_module(HudWidgetKey::InfoBar);
    hud_state.set_module_shell_state(HudWidgetKey::InfoBar, false, rect, rect, 0.0, 0.0);

    hud_state.reset_module(HudWidgetKey::InfoBar);

    assert_eq!(hud_state.module_enabled(HudWidgetKey::InfoBar), Some(true));
    assert_eq!(
        hud_state.module_target_rect(HudWidgetKey::InfoBar),
        Some(crate::hud::HUD_MODULE_DEFINITIONS[0].default_rect)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::InfoBar),
        Some(1.0)
    );
    assert!(hud_state.dirty_layout);
}

/// Verifies that a plain digit key emits the expected module-toggle intent.
#[test]
fn plain_digit_module_shortcut_toggles_module() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(TerminalManager::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit1, Some("1")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::AgentList
        ))]
    );
}

/// Verifies the plain `j` agent-list navigation shortcut emits focus+isolate for the next terminal.
#[test]
fn plain_j_navigates_to_next_agent_and_isolates_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    insert_terminal_manager_resources(&mut world, manager);
    let next_agent = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_two)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyJ, Some("j")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(next_agent)),
            AppCommand::Agent(AppAgentCommand::Inspect(next_agent)),
        ]
    );
}

/// Verifies that the down-arrow shortcut uses the same next-agent focus+isolate behavior as `j`.
#[test]
fn down_arrow_navigates_to_next_agent_and_isolates_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    insert_terminal_manager_resources(&mut world, manager);
    let next_agent = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_two)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::ArrowDown, None));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(next_agent)),
            AppCommand::Agent(AppAgentCommand::Inspect(next_agent)),
        ]
    );
}

/// Verifies the plain `k` agent-list navigation shortcut emits focus+isolate for the previous
/// terminal.
#[test]
fn plain_k_navigates_to_previous_agent_and_isolates_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);
    insert_terminal_manager_resources(&mut world, manager);
    let previous_agent = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_one)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(previous_agent)),
            AppCommand::Agent(AppAgentCommand::Inspect(previous_agent)),
        ]
    );
}

/// Verifies that the up-arrow shortcut uses the same previous-agent focus+isolate behavior as `k`.
#[test]
fn up_arrow_navigates_to_previous_agent_and_isolates_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);
    insert_terminal_manager_resources(&mut world, manager);
    let previous_agent = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_one)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::ArrowUp, None));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(previous_agent)),
            AppCommand::Agent(AppAgentCommand::Inspect(previous_agent)),
        ]
    );
}

/// Verifies that the authoritative app-command path updates focus/visibility and requests redraws.
#[test]
fn focus_and_visibility_requests_request_redraw_immediately() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id)
        .expect("agent should be linked");
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Inspect(agent_id)));
    run_app_commands(&mut world);

    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(id)
    );
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    let redraws_after_focus = world.resource::<Messages<RequestRedraw>>().len();
    assert!(redraws_after_focus >= 1);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::ClearFocus));
    run_app_commands(&mut world);

    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::ShowAll
    );
    assert!(world.resource::<Messages<RequestRedraw>>().len() > redraws_after_focus);
}

/// Verifies that `Alt+Shift+digit` still emits reset intents rather than toggle intents.
#[test]
fn alt_shift_module_shortcut_still_resets_module() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::AltLeft);
    keys.press(KeyCode::ShiftLeft);
    world.insert_resource(keys);
    world.insert_resource(TerminalManager::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit0, Some("0")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Widget(WidgetCommand::Reset(
            HudWidgetKey::InfoBar
        ))]
    );
}

/// Verifies that HUD module shortcuts are ignored while direct terminal input has keyboard capture.
#[test]
fn module_shortcuts_are_suppressed_while_direct_input_is_open() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.open_direct_terminal_input(crate::terminals::TerminalId(1));
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit1, Some("1")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
}

/// Verifies the fixed proportional layout of the message-box modal within the window.
#[test]
fn message_box_rect_is_top_aligned_and_shorter() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };

    let rect = message_box_rect(&window);
    assert!((rect.w - 1176.0).abs() < 0.01);
    assert!((rect.h - 468.0).abs() < 0.01);
    assert!((rect.x - 112.0).abs() < 0.01);
    assert!((rect.y - 8.0).abs() < 0.01);
}

/// Verifies that clicking the task-dialog `Clear done` button emits the clear-done intent but leaves
/// the dialog/editor state open for the subsequent persistence update.
#[test]
fn clicking_task_dialog_clear_done_button_persists_updated_text() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [x] done\n- [ ] keep");

    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let (_, clear_done_rect, _) = task_dialog_action_buttons(&window)[0];
    window.set_cursor_position(Some(Vec2::new(
        clear_done_rect.x + 4.0,
        clear_done_rect.y + 4.0,
    )));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let emitted = world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(
        emitted,
        vec![AppCommand::Task(AppTaskCommand::ClearDone {
            agent_id: crate::agents::AgentId(1),
        })]
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.task_dialog.visible);
    assert_eq!(hud_state.task_dialog.text, "- [x] done\n- [ ] keep");
}

/// Verifies that clearing done tasks through the app-command path refreshes the open task editor
/// from authoritative task state rather than leaving stale local text behind.
#[test]
fn clear_done_task_request_updates_open_dialog_from_persisted_state() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _, _) = super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text("session-a", "- [x] done\n- [ ] keep"));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(notes_state);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "stale local");
    insert_test_hud_state(&mut world, hud_state);
    {
        let mut tasks = world.resource_mut::<crate::conversations::AgentTaskStore>();
        let _ = tasks.set_text(agent_id, "- [x] done\n- [ ] keep");
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Task(AppTaskCommand::ClearDone { agent_id }));

    run_app_commands(&mut world);

    {
        let notes_state = world.resource::<TerminalNotesState>();
        assert_eq!(notes_state.note_text("session-a"), Some("- [ ] keep"));
    }
    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.task_dialog.text, "- [ ] keep");
    assert!(hud_state.task_dialog.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that submitting an empty task editor clears persisted note state instead of storing an
/// empty note blob.
#[test]
fn set_task_text_request_clears_persisted_task_presence_when_empty() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _, _) = super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text("session-a", "- [x] done"));
    assert!(notes_state
        .note_text("session-a")
        .is_some_and(|text| !text.trim().is_empty()));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(notes_state);
    assert!(world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .is_some());
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "");
    insert_test_hud_state(&mut world, hud_state);
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Composer(AppComposerCommand::Submit));

    run_app_commands(&mut world);

    let notes_state = world.resource::<TerminalNotesState>();
    assert_eq!(notes_state.note_text("session-a"), None);
    assert!(notes_state
        .note_text("session-a")
        .is_none_or(|text| text.trim().is_empty()));
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    let hud_state = snapshot_test_hud_state(&world);
    assert!(!hud_state.task_dialog.visible);
}

/// Verifies that consuming the next task through the app-command path sends the task payload to
/// the terminal and marks that task done in persisted notes.
#[test]
fn consume_next_task_request_sends_message_and_marks_task_done() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, input_rx, _) = super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text("session-a", "- [ ] first\n  detail\n- [ ] second"));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(notes_state);
    insert_default_hud_resources(&mut world);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut tasks = world.resource_mut::<crate::conversations::AgentTaskStore>();
        let _ = tasks.set_text(agent_id, "- [ ] first\n  detail\n- [ ] second");
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Task(AppTaskCommand::ConsumeNext { agent_id }));

    run_app_commands(&mut world);

    assert_eq!(
        input_rx.try_recv().unwrap(),
        crate::terminals::TerminalCommand::SendCommand("first\n  detail".into())
    );
    assert_eq!(
        world
            .resource::<TerminalNotesState>()
            .note_text("session-a"),
        Some("- [x] first\n  detail\n- [ ] second")
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that clicking the message-box append-task button turns the current draft into an
/// `AppendTerminalTask` intent and closes the modal.
#[test]
fn clicking_message_box_task_button_emits_append_task_intent() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("follow up");

    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let (_, append_rect, _) = message_box_action_buttons(&window)[0];
    window.set_cursor_position(Some(Vec2::new(append_rect.x + 4.0, append_rect.y + 4.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let emitted = world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(
        emitted,
        vec![AppCommand::Task(AppTaskCommand::Append {
            agent_id: crate::agents::AgentId(1),
            text: "follow up".into(),
        })]
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    assert!(!snapshot_test_hud_state(&world).message_box.visible);
}

/// Verifies that create-agent dialog pointer clicks persist field-focus and completion cleanup even
/// though the modal handler exits early after handling the click.
#[test]
fn create_agent_dialog_pointer_click_persists_focus_cleanup_and_redraw() {
    let mut world = World::default();
    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let name_rect = create_agent_name_field_rect(&window);
    window.set_cursor_position(Some(Vec2::new(name_rect.x + 4.0, name_rect.y + 4.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    init_hud_commands(&mut world);
    insert_default_hud_resources(&mut world);
    world.spawn((window, PrimaryWindow));

    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(AppCreateAgentKind::Agent);
        session.create_agent_dialog.focus = CreateAgentDialogField::Kind;
        session.create_agent_dialog.error = Some("stale error".into());
        session.create_agent_dialog.cwd_field.field.load_text("s");
        assert!(session
            .create_agent_dialog
            .cwd_field
            .start_or_cycle_completion(false));
    }

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let session = world.resource::<AppSessionState>();
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Name
    );
    assert_eq!(session.create_agent_dialog.error, None);
    assert!(session.create_agent_dialog.cwd_field.completion.is_none());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that the direct-input capture branch still persists layout drag cleanup even though it
/// returns before general HUD interaction runs.
#[test]
fn direct_input_pointer_capture_persists_drag_cleanup() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::InfoBar);
    hud_state.drag = Some(HudDragState {
        module_id: HudWidgetKey::InfoBar,
        grab_offset: Vec2::new(7.0, 9.0),
    });
    hud_state.open_direct_terminal_input(crate::terminals::TerminalId(3));

    let window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    init_hud_commands(&mut world);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    world.spawn((window, PrimaryWindow));

    world.run_system_once(handle_hud_pointer_input).unwrap();

    assert!(snapshot_test_hud_state(&world).drag.is_none());
}

/// Verifies that a mouse release with no cursor still clears the transient agent-list drag state
/// before the pointer handler exits.
#[test]
fn releasing_pointer_without_cursor_clears_agent_drag_state() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    hud_state.drag = Some(HudDragState {
        module_id: HudWidgetKey::AgentList,
        grab_offset: Vec2::new(3.0, 4.0),
    });

    let window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    init_hud_commands(&mut world);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    world.spawn((window, PrimaryWindow));

    world.insert_resource(AgentListUiState {
        scroll_offset: 0.0,
        hovered_agent: None,
        drag: AgentListDragState {
            pressed_agent: Some(crate::agents::AgentId(11)),
            press_origin: Some(Vec2::new(10.0, 12.0)),
            dragging_agent: Some(crate::agents::AgentId(11)),
            drag_cursor: Some(Vec2::new(15.0, 20.0)),
            drag_grab_offset_y: 6.0,
            last_reorder_index: Some(2),
        },
    });
    {
        let buttons = &mut world.resource_mut::<ButtonInput<MouseButton>>();
        buttons.press(MouseButton::Left);
        buttons.release(MouseButton::Left);
    }

    world.run_system_once(handle_hud_pointer_input).unwrap();

    let hud_state = snapshot_test_hud_state(&world);
    let agent_list_state = world.resource::<AgentListUiState>();
    assert!(hud_state.drag.is_none());
    assert_eq!(agent_list_state.drag.pressed_agent, None);
    assert_eq!(agent_list_state.drag.press_origin, None);
    assert_eq!(agent_list_state.drag.dragging_agent, None);
    assert_eq!(agent_list_state.drag.drag_cursor, None);
    assert_eq!(agent_list_state.drag.drag_grab_offset_y, 0.0);
    assert_eq!(agent_list_state.drag.last_reorder_index, None);
}

/// Verifies that HUD hit-testing returns the frontmost enabled module when rects overlap.
#[test]
fn hud_state_topmost_enabled_at_prefers_frontmost_module() {
    let mut state = HudState::default();
    state.insert_default_module(HudWidgetKey::InfoBar);
    state.insert_default_module(HudWidgetKey::AgentList);
    state.raise_to_front(HudWidgetKey::AgentList);

    assert_eq!(
        state.topmost_enabled_at(Vec2::new(40.0, 110.0)),
        Some(HudWidgetKey::AgentList)
    );
}

/// Verifies that removing a middle active terminal promotes the previous surviving terminal in
/// creation order to active/isolate state.
#[test]
fn killing_active_terminal_selects_previous_terminal_in_creation_order() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client.sessions.lock().unwrap().extend([
        "neozeus-session-a".to_owned(),
        "neozeus-session-b".to_owned(),
        "neozeus-session-c".to_owned(),
    ]);

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let (bridge_three, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    let id_three = manager.create_terminal_with_session(bridge_three, "neozeus-session-c".into());
    manager.focus_terminal(id_two);

    let mut store = TerminalPresentationStore::default();
    for id in [id_one, id_two, id_three] {
        store.register(
            id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_two),
    });
    world.insert_resource(TerminalViewState::default());
    for id in [id_one, id_two, id_three] {
        let panel_entity = world.spawn((TerminalPanel { id },)).id();
        let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    let focus = manager.clone_focus_state();
    assert_eq!(manager.terminal_ids(), &[id_one, id_three]);
    assert_eq!(focus.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_two)
    );
}

/// Verifies that removing the first active terminal promotes the next surviving terminal to
/// active/isolate state.
#[test]
fn killing_first_active_terminal_selects_next_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client.sessions.lock().unwrap().extend([
        "neozeus-session-a".to_owned(),
        "neozeus-session-b".to_owned(),
    ]);

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_one);

    let mut store = TerminalPresentationStore::default();
    for id in [id_one, id_two] {
        store.register(
            id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_one),
    });
    world.insert_resource(TerminalViewState::default());
    for id in [id_one, id_two] {
        let panel_entity = world.spawn((TerminalPanel { id },)).id();
        let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    let focus = manager.clone_focus_state();
    assert_eq!(manager.terminal_ids(), &[id_two]);
    assert_eq!(focus.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_one)
    );
}

/// Verifies that a successful active-terminal kill removes terminal-manager state, presentation
/// state, labels, spawned panel entities, and resets visibility/persistence bookkeeping.
#[test]
fn killing_active_terminal_removes_runtime_presentation_and_labels() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_some());
    assert!(client.sessions.lock().unwrap().is_empty());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}

/// Verifies that duplicate agent names are rejected before any daemon session is created.
#[test]
fn create_agent_rejects_duplicate_name_without_creating_session() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.init_resource::<Messages<AppCommand>>();
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    let mut catalog = crate::agents::AgentCatalog::default();
    catalog.create_agent(
        Some("oracle".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentCapabilities::terminal_defaults(),
    );
    world.insert_resource(catalog);
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(crate::app::CreateAgentKind::Agent);
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Create {
            label: Some("oracle".into()),
            spawn_shell_only: false,
            working_directory: "~/code".into(),
        }));

    run_app_commands(&mut world);

    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 0);
    assert!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .visible
    );
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .error
            .as_deref(),
        Some("agent `oracle` already exists")
    );
    assert!(client.created_sessions.lock().unwrap().is_empty());
}

/// Verifies that creating a shell agent creates a session without injecting any bootstrap command
/// payload.
#[test]
fn create_shell_agent_request_does_not_send_pi_command() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(crate::app::CreateAgentKind::Shell);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Create {
            label: Some("shell".into()),
            spawn_shell_only: true,
            working_directory: "~/code".into(),
        }));

    run_app_commands(&mut world);

    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
    assert!(
        !world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .visible
    );
    assert!(client.sent_commands.lock().unwrap().is_empty());
    assert_eq!(
        client.created_sessions.lock().unwrap().as_slice(),
        &[("neozeus-session-0".to_owned(), Some("~/code".to_owned()))]
    );
}

/// Verifies the special-case cleanup path for disconnected terminals: local state is removed even if
/// daemon-side kill returns an error.
#[test]
fn killing_disconnected_active_terminal_removes_local_state_even_if_daemon_kill_fails() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager
        .get_mut(id)
        .expect("missing terminal")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_some());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}

/// Verifies that a kill failure for an otherwise live terminal preserves all local state instead of
/// tearing presentation/labels down prematurely.
#[test]
fn killing_active_terminal_preserves_local_state_when_tmux_kill_fails() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    assert_eq!(world.resource::<TerminalManager>().terminal_ids(), &[id]);
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_some());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_none());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 1);
    assert_eq!(frame_count, 1);
}

/// Verifies the enum default for terminal visibility policy is the non-isolating `ShowAll` mode.
#[test]
fn terminal_visibility_policy_defaults_to_show_all() {
    assert_eq!(
        TerminalVisibilityPolicy::default(),
        TerminalVisibilityPolicy::ShowAll
    );
}
