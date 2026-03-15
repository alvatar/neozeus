use super::pressed_text;
use crate::{
    input::{ctrl_sequence, keyboard_input_to_terminal_command, should_spawn_bootstrap_terminal},
    terminals::TerminalCommand,
};
use bevy::input::ButtonInput;
use bevy::prelude::KeyCode;

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
fn bootstrap_terminal_shortcut_only_uses_plain_z_when_no_terminals_exist() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyZ, Some("z"));
    assert!(should_spawn_bootstrap_terminal(&event, &keys, false));
    assert!(!should_spawn_bootstrap_terminal(&event, &keys, true));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_spawn_bootstrap_terminal(&event, &ctrl_keys, false));
}
