use super::*;

/// Recognizes the exact `Ctrl+Enter` chord used to toggle direct-input mode.
///
/// The check is intentionally strict: the event must be a key press for `Enter`, Ctrl must be down,
/// and Alt/Super must both be absent so the shortcut cannot collide with terminal input or desktop
/// bindings.
fn is_plain_ctrl_enter(event: &KeyboardInput, ctrl: bool, alt: bool, super_key: bool) -> bool {
    event.state == ButtonState::Pressed
        && event.key_code == KeyCode::Enter
        && ctrl
        && !alt
        && !super_key
}

/// Hides the exact runtime-state predicate used when deciding whether keyboard input may be routed
/// into a terminal.
///
/// Keeping the call behind a local helper makes the intent at call sites clearer and gives this file
/// one place to change if the project's idea of "interactive" ever becomes stricter than the raw
/// runtime state's helper.
pub(super) fn terminal_is_interactive(terminal: &crate::terminals::TerminalRuntimeState) -> bool {
    terminal.is_interactive()
}

/// Routes keyboard input into the active terminal when direct-input mode is toggled on.
///
/// The system has two jobs. First, it watches for the `Ctrl+Enter` toggle that opens or closes
/// direct-input mode. Second, while the mode is active, it converts keyboard events into terminal
/// commands and sends them through the terminal bridge. If the target terminal disappears or stops
/// being interactive, the mode is closed immediately and the HUD is asked to redraw so the visual
/// framing stays in sync.
#[allow(
    clippy::too_many_arguments,
    reason = "direct terminal input needs keyboard, focus, modal capture, terminal state, and redraws together"
)]
pub(crate) fn handle_terminal_direct_input_keyboard(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    mut app_session: ResMut<AppSessionState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    if !primary_window.focused || app_session.modal_input_owner(&input_capture) {
        return;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(&keys);
    let had_direct_input = input_capture.direct_input_terminal.is_some();
    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
    if had_direct_input && input_capture.direct_input_terminal.is_none() {
        redraws.write(RequestRedraw);
    }

    if let Some(target_terminal) = input_capture.direct_input_terminal {
        // Direct-input mode is sticky across frames, but it is not allowed to outlive the terminal
        // it targets. Revalidate the target before forwarding any key events.
        let Some(terminal) = terminal_manager.get(target_terminal) else {
            input_capture.close_direct_terminal_input();
            redraws.write(RequestRedraw);
            return;
        };
        if !terminal_is_interactive(&terminal.snapshot.runtime) {
            input_capture.close_direct_terminal_input();
            redraws.write(RequestRedraw);
            return;
        }
        let mut mode_changed = false;
        for event in messages.read() {
            if is_plain_ctrl_enter(event, ctrl, alt, super_key) {
                let _ = input_capture
                    .toggle_direct_terminal_input(&mut app_session.composer, target_terminal);
                mode_changed = true;
                break;
            }
            if !ctrl && !alt && !super_key {
                match event.key_code {
                    KeyCode::End => {
                        let bottom_delta = terminal
                            .snapshot
                            .surface
                            .as_ref()
                            .and_then(|surface| i32::try_from(surface.display_offset).ok())
                            .map(|offset| -offset)
                            .unwrap_or(0);
                        if bottom_delta != 0 {
                            terminal.bridge.note_key_event(event);
                            terminal
                                .bridge
                                .send(TerminalCommand::ScrollDisplay(bottom_delta));
                        }
                        continue;
                    }
                    KeyCode::PageUp => {
                        terminal.bridge.note_key_event(event);
                        terminal.bridge.send(TerminalCommand::ScrollDisplay(
                            terminal_page_scroll_rows(&terminal_manager, target_terminal),
                        ));
                        continue;
                    }
                    KeyCode::PageDown => {
                        terminal.bridge.note_key_event(event);
                        terminal.bridge.send(TerminalCommand::ScrollDisplay(
                            -terminal_page_scroll_rows(&terminal_manager, target_terminal),
                        ));
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(command) = keyboard_input_to_terminal_command(event, &keys) {
                terminal.bridge.note_key_event(event);
                terminal.bridge.send(command);
            }
        }
        if mode_changed {
            redraws.write(RequestRedraw);
        }
        return;
    }

    let Some(active_id) = focus_state.active_id() else {
        return;
    };
    let Some(active_terminal) = terminal_manager.get(active_id) else {
        return;
    };
    if !terminal_is_interactive(&active_terminal.snapshot.runtime) {
        return;
    }
    for event in messages.read() {
        if !is_plain_ctrl_enter(event, ctrl, alt, super_key) {
            continue;
        }
        let _ = input_capture.toggle_direct_terminal_input(&mut app_session.composer, active_id);
        redraws.write(RequestRedraw);
        break;
    }
}

/// Converts a raw Bevy keyboard event into the terminal command NeoZeus should send to the PTY.
///
/// Control chords are translated first through [`ctrl_sequence`], because terminals expect the ASCII
/// control bytes rather than the printable character. Special navigation keys are then mapped to the
/// conventional escape sequences, and finally ordinary text input is emitted as `InputText`. Any
/// event involving unsupported modifier combinations is dropped.
pub(crate) fn keyboard_input_to_terminal_command(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> Option<TerminalCommand> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if event.state != ButtonState::Pressed {
        return None;
    }

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

/// Maps letter keys to the control-byte sequences terminals conventionally expect.
///
/// This covers the classic ASCII control range (`Ctrl+A` through `Ctrl+Z`) and intentionally returns
/// string slices because the rest of the input pipeline already sends terminal events as strings.
pub(crate) fn ctrl_sequence(key_code: KeyCode) -> Option<&'static str> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    match key_code {
        KeyCode::KeyA => Some("\u{1}"),
        KeyCode::KeyB => Some("\u{2}"),
        KeyCode::KeyC => Some("\u{3}"),
        KeyCode::KeyD => Some("\u{4}"),
        KeyCode::KeyE => Some("\u{5}"),
        KeyCode::KeyF => Some("\u{6}"),
        KeyCode::KeyG => Some("\u{7}"),
        KeyCode::KeyH => Some("\u{8}"),
        KeyCode::KeyI => Some("\u{9}"),
        KeyCode::KeyJ => Some("\n"),
        KeyCode::KeyK => Some("\u{b}"),
        KeyCode::KeyL => Some("\u{c}"),
        KeyCode::KeyM => Some("\r"),
        KeyCode::KeyN => Some("\u{e}"),
        KeyCode::KeyO => Some("\u{f}"),
        KeyCode::KeyP => Some("\u{10}"),
        KeyCode::KeyQ => Some("\u{11}"),
        KeyCode::KeyR => Some("\u{12}"),
        KeyCode::KeyS => Some("\u{13}"),
        KeyCode::KeyT => Some("\u{14}"),
        KeyCode::KeyU => Some("\u{15}"),
        KeyCode::KeyV => Some("\u{16}"),
        KeyCode::KeyW => Some("\u{17}"),
        KeyCode::KeyX => Some("\u{18}"),
        KeyCode::KeyY => Some("\u{19}"),
        KeyCode::KeyZ => Some("\u{1a}"),
        _ => None,
    }
}
