use super::{capturing_bridge, pressed_text, test_bridge};
use crate::{
    hud::{HudIntent, TerminalVisibilityState},
    input::{
        ctrl_sequence, focus_terminal_on_panel_click, handle_global_terminal_spawn_shortcut,
        handle_terminal_direct_input_keyboard, handle_terminal_lifecycle_shortcuts,
        handle_terminal_message_box_keyboard, hide_terminal_on_background_click,
        keyboard_input_to_terminal_command, should_exit_application, should_kill_active_terminal,
        should_spawn_terminal_globally,
    },
    terminals::{
        TerminalCommand, TerminalManager, TerminalNotesState, TerminalPanel, TerminalPresentation,
        TerminalSessionPersistenceState,
    },
};
use bevy::{
    app::AppExit,
    ecs::system::RunSystemOnce,
    input::{
        keyboard::{Key, KeyboardInput},
        ButtonInput, ButtonState,
    },
    prelude::{Entity, KeyCode, Messages, MouseButton, Time, Vec2, Visibility, Window, World},
    window::{PrimaryWindow, RequestRedraw},
};

fn pressed_key(key_code: KeyCode, logical_key: Key) -> KeyboardInput {
    KeyboardInput {
        key_code,
        logical_key,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    }
}

fn init_hud_commands(world: &mut World) {
    world.init_resource::<Messages<HudIntent>>();
}

fn drain_hud_commands(world: &mut World) -> Vec<HudIntent> {
    world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<HudIntent>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap()
}

fn dispatch_message_box_key(world: &mut World, event: KeyboardInput) {
    world.insert_resource(Messages::<KeyboardInput>::default());
    world.resource_mut::<Messages<KeyboardInput>>().write(event);
    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
}

fn dispatch_terminal_ui_key(world: &mut World, event: KeyboardInput) {
    world.insert_resource(Messages::<KeyboardInput>::default());
    world.resource_mut::<Messages<KeyboardInput>>().write(event);
    world
        .run_system_once(handle_terminal_direct_input_keyboard)
        .unwrap();
    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
}

fn world_with_active_terminal_and_receiver(
    cursor: Vec2,
    panel_visible: bool,
    panel_position: Vec2,
) -> (
    World,
    crate::terminals::TerminalId,
    std::sync::mpsc::Receiver<TerminalCommand>,
) {
    let (bridge, input_rx, _) = capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut world = World::default();
    let mut window = Window::default();
    window.set_cursor_position(Some(cursor));
    window.focused = true;

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(Time::<()>::default());
    world.insert_resource(manager);
    world.insert_resource(crate::hud::HudState::default());
    world.insert_resource(TerminalNotesState::default());
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id: terminal_id },
        TerminalPresentation {
            home_position: panel_position,
            current_position: panel_position,
            target_position: panel_position,
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        if panel_visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        },
    ));

    (world, terminal_id, input_rx)
}

fn world_with_active_terminal(
    cursor: Vec2,
    panel_visible: bool,
    panel_position: Vec2,
) -> (World, crate::terminals::TerminalId) {
    let (world, terminal_id, _input_rx) =
        world_with_active_terminal_and_receiver(cursor, panel_visible, panel_position);
    (world, terminal_id)
}

#[test]
fn ctrl_sequence_maps_common_shortcuts() {
    assert_eq!(ctrl_sequence(KeyCode::KeyC), Some("\u{3}"));
    assert_eq!(ctrl_sequence(KeyCode::KeyL), Some("\u{c}"));
    assert_eq!(ctrl_sequence(KeyCode::Enter), None);
}

#[test]
fn plain_text_uses_text_payload() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyA, Some("a"));
    let command = keyboard_input_to_terminal_command(&event, &keys);
    match command {
        Some(TerminalCommand::InputText(text)) => assert_eq!(text, "a"),
        _ => panic!("expected text input command"),
    }
}

#[test]
fn global_spawn_shortcut_only_uses_plain_z() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyZ, Some("z"));
    assert!(should_spawn_terminal_globally(&event, &keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_spawn_terminal_globally(&event, &ctrl_keys));
}

#[test]
fn global_spawn_shortcut_enqueues_spawn_even_with_active_terminal() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);

    let mut world = World::default();
    let window = Window {
        focused: true,
        ..Default::default()
    };
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(manager);
    world.insert_resource(crate::hud::HudState::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyZ, Some("z")));

    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::SpawnTerminal]
    );
}

#[test]
fn kill_active_terminal_shortcut_only_uses_plain_ctrl_k() {
    let event = pressed_text(KeyCode::KeyK, Some("k"));
    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(should_kill_active_terminal(&event, &ctrl_keys));

    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(!should_kill_active_terminal(&event, &plain_keys));

    let mut alt_ctrl_keys = ButtonInput::<KeyCode>::default();
    alt_ctrl_keys.press(KeyCode::ControlLeft);
    alt_ctrl_keys.press(KeyCode::AltLeft);
    assert!(!should_kill_active_terminal(&event, &alt_ctrl_keys));
}

#[test]
fn exit_application_shortcut_only_uses_plain_f10() {
    let event = pressed_text(KeyCode::F10, None);
    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(should_exit_application(&event, &plain_keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_exit_application(&event, &ctrl_keys));

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    assert!(!should_exit_application(&event, &alt_keys));
}

#[test]
fn f10_enqueues_app_exit() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(crate::hud::HudState::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::F10, None));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert_eq!(world.resource::<Messages<AppExit>>().len(), 1);
    assert!(drain_hud_commands(&mut world).is_empty());
}

#[test]
fn background_click_hides_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), true, Vec2::ZERO);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        crate::hud::TerminalVisibilityPolicy::ShowAll
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalViewState>()
            .offset,
        Vec2::ZERO
    );
    assert!(world
        .resource::<TerminalSessionPersistenceState>()
        .dirty_since_secs
        .is_some());
    assert!(manager.get(terminal_id).is_some());
}

#[test]
fn clicking_visible_terminal_does_not_hide_it() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(640.0, 360.0), true, Vec2::ZERO);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.active_id(), Some(terminal_id));
    assert!(world
        .resource::<TerminalSessionPersistenceState>()
        .dirty_since_secs
        .is_none());
}

#[test]
fn clicking_shifted_visible_terminal_does_not_hide_it() {
    let panel_position = Vec2::new(180.0, 120.0);
    let panel_center = Vec2::new(640.0 + panel_position.x, 360.0 - panel_position.y);
    let (mut world, terminal_id) = world_with_active_terminal(panel_center, true, panel_position);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.active_id(), Some(terminal_id));
    assert!(world
        .resource::<TerminalSessionPersistenceState>()
        .dirty_since_secs
        .is_none());
}

#[test]
fn clicking_terminal_panel_enqueues_focus_and_isolate_for_topmost_visible_panel() {
    let mut world = World::default();
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.set_cursor_position(Some(Vec2::new(640.0, 360.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(crate::hud::HudState::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel {
            id: crate::terminals::TerminalId(1),
        },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::new(220.0, 140.0),
            target_size: Vec2::new(220.0, 140.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: -0.1,
            target_z: -0.1,
        },
        Visibility::Visible,
    ));
    world.spawn((
        TerminalPanel {
            id: crate::terminals::TerminalId(2),
        },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::new(220.0, 140.0),
            target_size: Vec2::new(220.0, 140.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.3,
            target_z: 0.3,
        },
        Visibility::Visible,
    ));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world
        .run_system_once(focus_terminal_on_panel_click)
        .unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            HudIntent::FocusTerminal(crate::terminals::TerminalId(2)),
            HudIntent::HideAllButTerminal(crate::terminals::TerminalId(2)),
        ]
    );
}

#[test]
fn enter_opens_message_box_for_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Enter, None));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();

    let hud_state = world.resource::<crate::hud::HudState>();
    assert!(hud_state.message_box.visible);
    assert_eq!(hud_state.message_box.target_terminal, Some(terminal_id));
    assert!(hud_state.message_box.text.is_empty());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn plain_t_opens_task_dialog_for_active_terminal_with_saved_text() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let session_name = world
        .resource::<TerminalManager>()
        .get(terminal_id)
        .unwrap()
        .session_name
        .clone();
    world
        .resource_mut::<TerminalNotesState>()
        .set_note_text(&session_name, "- [ ] first task\n  detail");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyT, Some("t")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();

    let hud_state = world.resource::<crate::hud::HudState>();
    assert!(hud_state.task_dialog.visible);
    assert_eq!(hud_state.task_dialog.target_terminal, Some(terminal_id));
    assert_eq!(hud_state.task_dialog.text, "- [ ] first task\n  detail");
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn plain_n_enqueues_consume_next_task_for_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyN, Some("n")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::ConsumeNextTerminalTask(terminal_id)]
    );
}

#[test]
fn ctrl_enter_toggles_direct_input_mode_for_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<RequestRedraw>>();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);

    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let hud_state = world.resource::<crate::hud::HudState>();
    assert_eq!(hud_state.direct_input_terminal, Some(terminal_id));
    assert!(!hud_state.message_box.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);

    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let hud_state = world.resource::<crate::hud::HudState>();
    assert_eq!(hud_state.direct_input_terminal, None);
    assert!(!hud_state.message_box.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 2);
}

#[test]
fn direct_input_mode_sends_keys_to_terminal_without_opening_message_box() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));
    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::InputText("a".into())
    );
    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::InputEvent("\r".into())
    );
    assert!(!world.resource::<crate::hud::HudState>().message_box.visible);
}

#[test]
fn closing_message_box_preserves_draft_for_reopen() {
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = crate::hud::HudState::default();

    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("draft payload");
    hud_state.close_message_box();

    assert!(!hud_state.message_box.visible);
    assert_eq!(hud_state.message_box.target_terminal, None);

    hud_state.open_message_box(terminal_id);
    assert!(hud_state.message_box.visible);
    assert_eq!(hud_state.message_box.target_terminal, Some(terminal_id));
    assert_eq!(hud_state.message_box.text, "draft payload");
}

#[test]
fn message_box_keeps_separate_drafts_per_terminal() {
    let terminal_one = crate::terminals::TerminalId(7);
    let terminal_two = crate::terminals::TerminalId(9);
    let mut hud_state = crate::hud::HudState::default();

    hud_state.open_message_box(terminal_one);
    hud_state.message_box.insert_text("first draft");
    hud_state.close_message_box();

    hud_state.open_message_box(terminal_two);
    hud_state.message_box.insert_text("second draft");
    hud_state.close_message_box();

    hud_state.open_message_box(terminal_one);
    assert_eq!(hud_state.message_box.text, "first draft");

    hud_state.open_message_box(terminal_two);
    assert_eq!(hud_state.message_box.text, "second draft");
}

#[test]
fn message_box_supports_multiline_typing_and_ctrl_s_send() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyB, Some("b")));

    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "a\nb"
    );

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyS, Some("s")));

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::SendTerminalCommand(terminal_id, "a\nb".into())]
    );
    {
        let hud_state = world.resource::<crate::hud::HudState>();
        assert!(!hud_state.message_box.visible);
        assert!(hud_state.message_box.text.is_empty());
    }

    world.insert_resource(ButtonInput::<KeyCode>::default());
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Enter, None));
    let hud_state = world.resource::<crate::hud::HudState>();
    assert!(hud_state.message_box.visible);
    assert!(hud_state.message_box.text.is_empty());
}

#[test]
fn ctrl_t_clears_done_tasks_for_active_terminal_when_dialog_is_closed() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyT, Some("t")));

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::ClearDoneTerminalTasks(terminal_id)]
    );
}

#[test]
fn task_dialog_ctrl_t_emits_clear_done_request() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [x] done\n  detail\n- [ ] keep");
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyT, Some("t")));

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::ClearDoneTerminalTasks(terminal_id)]
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().task_dialog.text,
        "- [x] done\n  detail\n- [ ] keep"
    );
    assert!(world.resource::<crate::hud::HudState>().task_dialog.visible);
}

#[test]
fn reopening_task_dialog_uses_persisted_text_not_stale_editor_state() {
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = crate::hud::HudState::default();

    hud_state.open_task_dialog(terminal_id, "persisted one");
    hud_state.task_dialog.insert_text("\nunsaved");
    hud_state.close_task_dialog();

    hud_state.open_task_dialog(terminal_id, "persisted two");
    assert!(hud_state.task_dialog.visible);
    assert_eq!(hud_state.task_dialog.text, "persisted two");
}

#[test]
fn task_dialog_escape_persists_tasks_and_closes() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [ ] old");
    hud_state.task_dialog.insert_text("\n- [ ] new");
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Escape, Key::Escape));

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::SetTerminalTaskText(
            terminal_id,
            "- [ ] old\n- [ ] new".into()
        )]
    );
    let hud_state = world.resource::<crate::hud::HudState>();
    assert!(!hud_state.task_dialog.visible);
}

#[test]
fn message_box_ctrl_t_does_not_enqueue_task_shortcuts() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("follow up\n  details");
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyT, Some("t")));

    assert!(drain_hud_commands(&mut world).is_empty());
    let hud_state = world.resource::<crate::hud::HudState>();
    assert!(hud_state.message_box.visible);
    assert_eq!(hud_state.message_box.text, "follow up\n  details");
}

#[test]
fn message_box_ctrl_bindings_edit_multiline_text() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("alpha\nbeta");
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyA, Key::Character("a".into())),
    );
    assert_eq!(
        world
            .resource::<crate::hud::HudState>()
            .message_box
            .cursor_line_and_column(),
        (1, 0)
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyK, Key::Character("k".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "alpha\n"
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "alpha\nbeta"
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyP, Key::Character("p".into())),
    );
    assert_eq!(
        world
            .resource::<crate::hud::HudState>()
            .message_box
            .cursor_line_and_column(),
        (0, 4)
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyE, Key::Character("e".into())),
    );
    assert_eq!(
        world
            .resource::<crate::hud::HudState>()
            .message_box
            .cursor_line_and_column(),
        (0, 5)
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyN, Key::Character("n".into())),
    );
    assert_eq!(
        world
            .resource::<crate::hud::HudState>()
            .message_box
            .cursor_line_and_column(),
        (1, 4)
    );
}

#[test]
fn message_box_mark_region_ctrl_w_and_ctrl_y_work() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("alpha beta gamma");
    hud_state.message_box.cursor = 6;
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.mark,
        Some(6)
    );

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    world.insert_resource(alt_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyF, Key::Character("f".into())),
    );
    assert_eq!(
        world
            .resource::<crate::hud::HudState>()
            .message_box
            .region_bounds(),
        Some((6, 10))
    );

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyW, Key::Character("w".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "alpha  gamma"
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.mark,
        None
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "alpha beta gamma"
    );
}

#[test]
fn message_box_meta_copy_kill_ring_history_and_backward_kill_word_work() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("one two three");
    hud_state.message_box.cursor = 4;
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    world.insert_resource(alt_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyF, Key::Character("f".into())),
    );
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyW, Key::Character("w".into())),
    );

    world
        .resource_mut::<crate::hud::HudState>()
        .message_box
        .cursor = 8;
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyD, Key::Character("d".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "one two "
    );

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "one two three"
    );

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    world.insert_resource(alt_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "one two two"
    );

    world
        .resource_mut::<crate::hud::HudState>()
        .message_box
        .cursor = world
        .resource::<crate::hud::HudState>()
        .message_box
        .text
        .len();
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Backspace, None));
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "one two "
    );
}

#[test]
fn message_box_ctrl_o_and_ctrl_j_work() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("ab");
    hud_state.message_box.cursor = 1;
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyO, Key::Character("o".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "a\nb"
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.cursor,
        1
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyJ, Key::Character("j".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "a\n\nb"
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.cursor,
        2
    );
}

#[test]
fn message_box_alt_word_motion_and_ctrl_d_work() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("one two");
    world.insert_resource(hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    world.insert_resource(alt_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyB, Key::Character("b".into())),
    );
    assert_eq!(
        world
            .resource::<crate::hud::HudState>()
            .message_box
            .cursor_line_and_column(),
        (0, 4)
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyF, Key::Character("f".into())),
    );
    assert_eq!(
        world
            .resource::<crate::hud::HudState>()
            .message_box
            .cursor_line_and_column(),
        (0, 7)
    );

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyB, Key::Character("b".into())),
    );
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyD, Key::Character("d".into())),
    );
    assert_eq!(
        world.resource::<crate::hud::HudState>().message_box.text,
        "one tw"
    );
}

#[test]
fn lifecycle_shortcuts_are_suppressed_while_message_box_is_open() {
    let mut world = World::default();
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(crate::terminals::TerminalId(1));
    world.insert_resource(hud_state);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}

#[test]
fn lifecycle_shortcuts_are_suppressed_while_direct_input_is_open() {
    let mut world = World::default();
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(crate::terminals::TerminalId(1));
    world.insert_resource(hud_state);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}

#[test]
fn clicking_hud_does_not_hide_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    let mut module =
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]);
    module.shell.current_rect = crate::hud::HudRect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    module.shell.target_rect = module.shell.current_rect;
    hud_state.insert(crate::hud::HudModuleId::DebugToolbar, module);
    world.insert_resource(hud_state);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.active_id(), Some(terminal_id));
}
