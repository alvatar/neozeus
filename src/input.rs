use crate::{
    hud::{HudCommand, HudDispatcher, HudState},
    terminals::{
        mark_terminal_sessions_dirty, terminal_texture_screen_size, TerminalCommand,
        TerminalDisplayMode, TerminalManager, TerminalPanel, TerminalPointerState,
        TerminalPresentation, TerminalPresentationStore, TerminalSessionPersistenceState,
        TerminalViewState,
    },
};
use bevy::{
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
        ButtonState,
    },
    prelude::*,
    window::PrimaryWindow,
};

fn has_plain_modifiers(keys: &ButtonInput<KeyCode>) -> (bool, bool, bool) {
    (
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight),
    )
}

pub(crate) fn should_spawn_terminal_globally(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
    has_active_terminal: bool,
) -> bool {
    if has_active_terminal || event.state != ButtonState::Pressed || event.key_code != KeyCode::KeyZ
    {
        return false;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    !(ctrl || alt || super_key)
}

pub(crate) fn should_kill_active_terminal(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::KeyK {
        return false;
    }
    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    ctrl && !alt && !super_key
}

pub(crate) fn handle_global_terminal_spawn_shortcut(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    mut dispatcher: ResMut<HudDispatcher>,
) {
    if !primary_window.focused || terminal_manager.active_id().is_some() {
        return;
    }

    for event in messages.read() {
        if should_spawn_terminal_globally(event, &keys, false) {
            dispatcher.commands.push(HudCommand::SpawnTerminal);
            break;
        }
    }
}

pub(crate) fn handle_terminal_lifecycle_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut dispatcher: ResMut<HudDispatcher>,
) {
    for event in messages.read() {
        if should_kill_active_terminal(event, &keys) {
            dispatcher.commands.push(HudCommand::KillActiveTerminal);
        }
    }
}

fn terminal_panel_contains_cursor(
    window: &Window,
    presentation: &TerminalPresentation,
    cursor: Vec2,
) -> bool {
    let min = Vec2::new(
        window.width() * 0.5 + presentation.current_position.x - presentation.current_size.x * 0.5,
        window.height() * 0.5 + presentation.current_position.y - presentation.current_size.y * 0.5,
    );
    let max = min + presentation.current_size;
    cursor.x >= min.x && cursor.x <= max.x && cursor.y >= min.y && cursor.y <= max.y
}

pub(crate) fn hide_terminal_on_background_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    hud_state: Res<HudState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
) {
    if !mouse_buttons.just_pressed(MouseButton::Left) || !primary_window.focused {
        return;
    }
    let Some(_) = terminal_manager.active_id() else {
        return;
    };
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if hud_state.topmost_enabled_at(cursor).is_some() {
        return;
    }
    if panels.iter().any(|(_, presentation, visibility)| {
        *visibility == Visibility::Visible
            && terminal_panel_contains_cursor(&primary_window, presentation, cursor)
    }) {
        return;
    }
    if terminal_manager.clear_active_terminal().is_some() {
        mark_terminal_sessions_dirty(&mut session_persistence, Some(&time));
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "mouse drag needs input, geometry, pointer state, and terminal bridge"
)]
pub(crate) fn drag_terminal_view(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    mut view_state: ResMut<TerminalViewState>,
    mut pointer_state: ResMut<TerminalPointerState>,
) {
    let delta = mouse_motion
        .read()
        .fold(Vec2::ZERO, |acc, event| acc + event.delta);

    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let middle_pressed = mouse_buttons.pressed(MouseButton::Middle);
    if !primary_window.focused || !middle_pressed || delta == Vec2::ZERO {
        pointer_state.scroll_drag_remainder_px = 0.0;
        return;
    }

    if shift {
        pointer_state.scroll_drag_remainder_px = 0.0;
        view_state.offset += Vec2::new(delta.x, -delta.y);
        return;
    }

    let Some(texture_state) = presentation_store.active_texture_state(terminal_manager.active_id())
    else {
        pointer_state.scroll_drag_remainder_px = 0.0;
        return;
    };
    let pixel_perfect = presentation_store.active_display_mode(terminal_manager.active_id())
        == Some(TerminalDisplayMode::PixelPerfect);
    let screen_size =
        terminal_texture_screen_size(texture_state, &view_state, &primary_window, pixel_perfect);
    let screen_cell_height = if texture_state.cell_size.y == 0 || texture_state.texture_size.y == 0
    {
        1.0
    } else {
        screen_size.y * (texture_state.cell_size.y as f32 / texture_state.texture_size.y as f32)
    }
    .max(1.0);

    pointer_state.scroll_drag_remainder_px += delta.y;
    let lines = (-pointer_state.scroll_drag_remainder_px / screen_cell_height).trunc() as i32;
    if lines != 0 {
        pointer_state.scroll_drag_remainder_px += lines as f32 * screen_cell_height;
        if let Some(bridge) = terminal_manager.active_bridge() {
            bridge.send(TerminalCommand::ScrollDisplay(lines));
        }
    }
}

pub(crate) fn zoom_terminal_view(
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut view_state: ResMut<TerminalViewState>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !primary_window.focused || !shift {
        return;
    }

    let zoom_delta = mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 24.0,
        }
    });

    if zoom_delta == 0.0 {
        return;
    }

    view_state.distance = (view_state.distance - zoom_delta * 0.8).clamp(2.0, 40.0);
}

pub(crate) fn forward_keyboard_input(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    terminal_manager: Res<TerminalManager>,
    _primary_window: Single<&Window, With<PrimaryWindow>>,
) {
    let Some(bridge) = terminal_manager.active_bridge() else {
        return;
    };

    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        bridge.note_key_event(event);
        if let Some(command) = keyboard_input_to_terminal_command(event, &keys) {
            bridge.send(command);
        }
    }
}

pub(crate) fn keyboard_input_to_terminal_command(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> Option<TerminalCommand> {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let super_key = keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight);

    if ctrl && !alt && !super_key {
        if let Some(control) = ctrl_sequence(event.key_code) {
            return Some(TerminalCommand::InputEvent(control.to_string()));
        }
    }

    match event.key_code {
        KeyCode::Enter => Some(TerminalCommand::InputEvent("\r".into())),
        KeyCode::Backspace => Some(TerminalCommand::InputEvent("\u{7f}".into())),
        KeyCode::Tab => Some(TerminalCommand::InputEvent("\t".into())),
        KeyCode::Escape => Some(TerminalCommand::InputEvent("\u{1b}".into())),
        KeyCode::ArrowUp => Some(TerminalCommand::InputEvent("\u{1b}[A".into())),
        KeyCode::ArrowDown => Some(TerminalCommand::InputEvent("\u{1b}[B".into())),
        KeyCode::ArrowRight => Some(TerminalCommand::InputEvent("\u{1b}[C".into())),
        KeyCode::ArrowLeft => Some(TerminalCommand::InputEvent("\u{1b}[D".into())),
        KeyCode::Home => Some(TerminalCommand::InputEvent("\u{1b}[H".into())),
        KeyCode::End => Some(TerminalCommand::InputEvent("\u{1b}[F".into())),
        KeyCode::PageUp => Some(TerminalCommand::InputEvent("\u{1b}[5~".into())),
        KeyCode::PageDown => Some(TerminalCommand::InputEvent("\u{1b}[6~".into())),
        KeyCode::Delete => Some(TerminalCommand::InputEvent("\u{1b}[3~".into())),
        KeyCode::Insert => Some(TerminalCommand::InputEvent("\u{1b}[2~".into())),
        _ if ctrl || alt || super_key => None,
        _ => event
            .text
            .as_ref()
            .filter(|text| !text.is_empty())
            .map(|text| TerminalCommand::InputText(text.to_string()))
            .or_else(|| match &event.logical_key {
                Key::Character(text) if !text.is_empty() => {
                    Some(TerminalCommand::InputText(text.to_string()))
                }
                Key::Space => Some(TerminalCommand::InputText(" ".into())),
                _ => None,
            }),
    }
}

pub(crate) fn ctrl_sequence(key_code: KeyCode) -> Option<&'static str> {
    match key_code {
        KeyCode::KeyA => Some("\u{1}"),
        KeyCode::KeyC => Some("\u{3}"),
        KeyCode::KeyD => Some("\u{4}"),
        KeyCode::KeyE => Some("\u{5}"),
        KeyCode::KeyL => Some("\u{c}"),
        KeyCode::KeyU => Some("\u{15}"),
        KeyCode::KeyZ => Some("\u{1a}"),
        _ => None,
    }
}
