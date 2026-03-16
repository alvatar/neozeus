use super::{pressed_text, test_bridge};
use crate::{
    hud::{HudCommand, HudDispatcher},
    input::{
        ctrl_sequence, handle_global_terminal_spawn_shortcut, hide_terminal_on_background_click,
        keyboard_input_to_terminal_command, should_kill_active_terminal,
        should_spawn_terminal_globally,
    },
    terminals::{
        TerminalCommand, TerminalManager, TerminalPanel, TerminalPresentation,
        TerminalSessionPersistenceState,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{keyboard::KeyboardInput, ButtonInput},
    prelude::{KeyCode, Messages, MouseButton, Time, Vec2, Visibility, Window, World},
    window::PrimaryWindow,
};

fn world_with_active_terminal(
    cursor: Vec2,
    panel_visible: bool,
) -> (World, crate::terminals::TerminalId) {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut world = World::default();
    let mut window = Window::default();
    window.set_cursor_position(Some(cursor));
    window.focused = true;

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Time::<()>::default());
    world.insert_resource(manager);
    world.insert_resource(crate::hud::HudState::default());
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id: terminal_id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
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
fn global_spawn_shortcut_only_uses_plain_z_when_no_terminal_is_active() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyZ, Some("z"));
    assert!(should_spawn_terminal_globally(&event, &keys, false));
    assert!(!should_spawn_terminal_globally(&event, &keys, true));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_spawn_terminal_globally(&event, &ctrl_keys, false));
}

#[test]
fn global_spawn_shortcut_enqueues_spawn_even_when_hidden_terminals_exist() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let _ = manager.clear_active_terminal();

    let mut world = World::default();
    let window = Window {
        focused: true,
        ..Default::default()
    };
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(manager);
    world.insert_resource(HudDispatcher::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyZ, Some("z")));

    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();

    assert_eq!(
        world.resource::<HudDispatcher>().commands,
        vec![HudCommand::SpawnTerminal]
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
fn background_click_hides_active_terminal() {
    let (mut world, terminal_id) = world_with_active_terminal(Vec2::new(10.0, 10.0), true);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.active_id(), None);
    assert!(world
        .resource::<TerminalSessionPersistenceState>()
        .dirty_since_secs
        .is_some());
    assert!(manager.get(terminal_id).is_some());
}

#[test]
fn clicking_visible_terminal_does_not_hide_it() {
    let (mut world, terminal_id) = world_with_active_terminal(Vec2::new(640.0, 360.0), true);
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
fn clicking_hud_does_not_hide_active_terminal() {
    let (mut world, terminal_id) = world_with_active_terminal(Vec2::new(10.0, 10.0), false);
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
