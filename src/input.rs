use crate::{
    hud::{
        HudInputCaptureState, HudIntent, HudLayoutState, HudModalState, TerminalVisibilityPolicy,
        TerminalVisibilityState,
    },
    terminals::{
        mark_terminal_sessions_dirty, terminal_texture_screen_size, TerminalCommand,
        TerminalDisplayMode, TerminalFocusState, TerminalManager, TerminalNotesState,
        TerminalPanel, TerminalPointerState, TerminalPresentation, TerminalPresentationStore,
        TerminalSessionPersistenceState, TerminalViewState,
    },
};
use bevy::{
    app::AppExit,
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
        ButtonState,
    },
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};

fn has_plain_modifiers(keys: &ButtonInput<KeyCode>) -> (bool, bool, bool) {
    (
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight),
    )
}

fn is_plain_ctrl_enter(event: &KeyboardInput, ctrl: bool, alt: bool, super_key: bool) -> bool {
    event.state == ButtonState::Pressed
        && event.key_code == KeyCode::Enter
        && ctrl
        && !alt
        && !super_key
}

fn terminal_is_interactive(terminal: &crate::terminals::TerminalRuntimeState) -> bool {
    terminal.is_interactive()
}

pub(crate) fn should_spawn_terminal_globally(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::KeyZ {
        return false;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    !(ctrl || alt || super_key)
}

pub(crate) fn should_spawn_shell_terminal_globally(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::KeyZ {
        return false;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    ctrl && alt && !super_key
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

pub(crate) fn should_exit_application(event: &KeyboardInput, keys: &ButtonInput<KeyCode>) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::F10 {
        return false;
    }
    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    !(ctrl || alt || super_key)
}

pub(crate) fn handle_global_terminal_spawn_shortcut(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    modal_state: Res<HudModalState>,
    input_capture: Res<HudInputCaptureState>,
    mut hud_commands: MessageWriter<HudIntent>,
) {
    if modal_state.keyboard_capture_active(&input_capture) || !primary_window.focused {
        return;
    }

    for event in messages.read() {
        if should_spawn_shell_terminal_globally(event, &keys) {
            hud_commands.write(HudIntent::SpawnShellTerminal);
            break;
        }
        if should_spawn_terminal_globally(event, &keys) {
            hud_commands.write(HudIntent::SpawnTerminal);
            break;
        }
    }
}

pub(crate) fn handle_terminal_lifecycle_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    modal_state: Res<HudModalState>,
    input_capture: Res<HudInputCaptureState>,
    mut hud_commands: MessageWriter<HudIntent>,
    mut app_exits: MessageWriter<AppExit>,
) {
    if modal_state.keyboard_capture_active(&input_capture) {
        return;
    }

    for event in messages.read() {
        if should_exit_application(event, &keys) {
            app_exits.write(AppExit::Success);
            break;
        }
        if should_kill_active_terminal(event, &keys) {
            hud_commands.write(HudIntent::KillActiveTerminal);
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
        window.height() * 0.5 - presentation.current_position.y - presentation.current_size.y * 0.5,
    );
    let max = min + presentation.current_size;
    cursor.x >= min.x && cursor.x <= max.x && cursor.y >= min.y && cursor.y <= max.y
}

fn topmost_terminal_panel_at_cursor(
    window: &Window,
    panels: &Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    cursor: Vec2,
) -> Option<TerminalPanel> {
    panels
        .iter()
        .filter(|(_, _, visibility)| **visibility == Visibility::Visible)
        .filter(|(_, presentation, _)| terminal_panel_contains_cursor(window, presentation, cursor))
        .max_by(|(_, left, _), (_, right, _)| left.current_z.total_cmp(&right.current_z))
        .map(|(panel, _, _)| *panel)
}

pub(crate) fn focus_terminal_on_panel_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    modal_state: Res<HudModalState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    mut hud_commands: MessageWriter<HudIntent>,
) {
    if modal_state.message_box.visible
        || modal_state.task_dialog.visible
        || !mouse_buttons.just_pressed(MouseButton::Left)
        || !primary_window.focused
    {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return;
    }
    let Some(panel) = topmost_terminal_panel_at_cursor(&primary_window, &panels, cursor) else {
        return;
    };
    hud_commands.write(HudIntent::FocusTerminal(panel.id));
    hud_commands.write(HudIntent::HideAllButTerminal(panel.id));
}

#[allow(
    clippy::too_many_arguments,
    reason = "background-click clear needs input, focus, visibility, view, and persistence resources together"
)]
pub(crate) fn hide_terminal_on_background_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    modal_state: Res<HudModalState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    mut _terminal_manager: ResMut<TerminalManager>,
    mut focus_state: ResMut<TerminalFocusState>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut view_state: ResMut<TerminalViewState>,
) {
    if modal_state.message_box.visible
        || modal_state.task_dialog.visible
        || !mouse_buttons.just_pressed(MouseButton::Left)
        || !primary_window.focused
    {
        return;
    }
    let Some(_) = focus_state.active_id() else {
        return;
    };
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return;
    }
    if topmost_terminal_panel_at_cursor(&primary_window, &panels, cursor).is_some() {
        return;
    }
    if focus_state.clear_active_terminal().is_some() {
        #[cfg(test)]
        _terminal_manager.replace_test_focus_state(&focus_state);
        visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
        view_state.focus_terminal(None);
        input_capture.reconcile_direct_terminal_input(focus_state.active_id());
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
    focus_state: Res<TerminalFocusState>,
    presentation_store: Res<TerminalPresentationStore>,
    layout_state: Res<HudLayoutState>,
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
        view_state.apply_offset_delta(focus_state.active_id(), Vec2::new(delta.x, -delta.y));
        return;
    }

    let Some(texture_state) = presentation_store.active_texture_state(focus_state.active_id())
    else {
        pointer_state.scroll_drag_remainder_px = 0.0;
        return;
    };
    let pixel_perfect = presentation_store.active_display_mode(focus_state.active_id())
        == Some(TerminalDisplayMode::PixelPerfect);
    let screen_size = terminal_texture_screen_size(
        texture_state,
        &view_state,
        &primary_window,
        &layout_state,
        pixel_perfect,
    );
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
        if let Some(bridge) = focus_state.active_bridge(&terminal_manager) {
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
    mut modal_state: ResMut<HudModalState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    if !primary_window.focused || modal_state.message_box.visible || modal_state.task_dialog.visible
    {
        return;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(&keys);
    let had_direct_input = input_capture.direct_input_terminal.is_some();
    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
    if had_direct_input && input_capture.direct_input_terminal.is_none() {
        redraws.write(RequestRedraw);
    }

    if let Some(target_terminal) = input_capture.direct_input_terminal {
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
                let _ =
                    input_capture.toggle_direct_terminal_input(&mut modal_state, target_terminal);
                mode_changed = true;
                break;
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
        let _ = input_capture.toggle_direct_terminal_input(&mut modal_state, active_id);
        redraws.write(RequestRedraw);
        break;
    }
}

fn message_box_event_text(event: &KeyboardInput) -> Option<String> {
    event
        .text
        .as_ref()
        .filter(|text| !text.is_empty())
        .map(|text| text.to_string())
        .or_else(|| match &event.logical_key {
            Key::Character(text) if !text.is_empty() => Some(text.to_string()),
            Key::Space => Some(" ".to_owned()),
            _ => None,
        })
}

fn close_task_dialog_intent(modal_state: &mut HudModalState) -> Option<HudIntent> {
    let target_terminal = modal_state.task_dialog.target_terminal?;
    let payload = modal_state.task_dialog.text.clone();
    modal_state.close_task_dialog();
    Some(HudIntent::SetTerminalTaskText(target_terminal, payload))
}

fn handle_text_editor_event(
    editor: &mut crate::hud::HudMessageBoxState,
    event: &KeyboardInput,
    ctrl: bool,
    alt: bool,
    super_key: bool,
) -> bool {
    if ctrl && !alt && !super_key {
        match event.key_code {
            KeyCode::Space => editor.set_mark(),
            KeyCode::KeyA => editor.move_line_start(),
            KeyCode::KeyB => editor.move_left(),
            KeyCode::KeyD => editor.delete_forward_char(),
            KeyCode::KeyE => editor.move_line_end(),
            KeyCode::KeyF => editor.move_right(),
            KeyCode::KeyH => editor.delete_backward_char(),
            KeyCode::KeyJ => editor.newline_and_indent(),
            KeyCode::KeyK => editor.kill_to_end_of_line(),
            KeyCode::KeyN => editor.move_down(),
            KeyCode::KeyO => editor.open_line(),
            KeyCode::KeyP => editor.move_up(),
            KeyCode::KeyW => editor.kill_region(),
            KeyCode::KeyY => editor.yank(),
            _ => false,
        }
    } else if alt && !ctrl && !super_key {
        match event.key_code {
            KeyCode::Backspace => editor.kill_word_backward(),
            KeyCode::KeyB => editor.move_word_backward(),
            KeyCode::KeyD => editor.kill_word_forward(),
            KeyCode::KeyF => editor.move_word_forward(),
            KeyCode::KeyW => editor.copy_region(),
            KeyCode::KeyY => editor.yank_pop(),
            _ => false,
        }
    } else if !(ctrl || alt || super_key) {
        match event.key_code {
            KeyCode::Enter => editor.insert_newline(),
            KeyCode::Backspace => editor.delete_backward_char(),
            KeyCode::Delete => editor.delete_forward_char(),
            KeyCode::ArrowLeft => editor.move_left(),
            KeyCode::ArrowRight => editor.move_right(),
            KeyCode::ArrowUp => editor.move_up(),
            KeyCode::ArrowDown => editor.move_down(),
            KeyCode::Home => editor.move_line_start(),
            KeyCode::End => editor.move_line_end(),
            KeyCode::Tab => editor.insert_text("\t"),
            _ => message_box_event_text(event).is_some_and(|text| editor.insert_text(&text)),
        }
    } else {
        false
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "dialog keyboard handling needs input, focus, notes, terminal state, HUD state, commands, and redraws together"
)]
pub(crate) fn handle_terminal_message_box_keyboard(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    notes_state: Res<TerminalNotesState>,
    mut modal_state: ResMut<HudModalState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut hud_commands: MessageWriter<HudIntent>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    if !primary_window.focused {
        return;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(&keys);

    if input_capture.direct_input_terminal.is_some() {
        return;
    }

    if modal_state.message_box.visible {
        let mut needs_redraw = false;
        for event in messages.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }

            if ctrl && !alt && !super_key && event.key_code == KeyCode::KeyS {
                if let Some(target_terminal) = modal_state.message_box.target_terminal {
                    let payload = modal_state.message_box.text.clone();
                    if !payload.is_empty() {
                        hud_commands
                            .write(HudIntent::SendTerminalCommand(target_terminal, payload));
                    }
                }
                modal_state.close_message_box_and_discard_draft();
                needs_redraw = true;
                break;
            }

            if event.key_code == KeyCode::Escape {
                modal_state.close_message_box();
                needs_redraw = true;
                break;
            }

            needs_redraw |=
                handle_text_editor_event(&mut modal_state.message_box, event, ctrl, alt, super_key);
        }

        if needs_redraw {
            redraws.write(RequestRedraw);
        }
        return;
    }

    if modal_state.task_dialog.visible {
        let mut needs_redraw = false;
        for event in messages.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }

            if ctrl && !alt && !super_key && event.key_code == KeyCode::KeyT {
                if let Some(target_terminal) = modal_state.task_dialog.target_terminal {
                    hud_commands.write(HudIntent::ClearDoneTerminalTasks(target_terminal));
                }
                needs_redraw = true;
                continue;
            }

            if event.key_code == KeyCode::Escape {
                if let Some(intent) = close_task_dialog_intent(&mut modal_state) {
                    hud_commands.write(intent);
                }
                needs_redraw = true;
                break;
            }

            needs_redraw |=
                handle_text_editor_event(&mut modal_state.task_dialog, event, ctrl, alt, super_key);
        }

        if needs_redraw {
            redraws.write(RequestRedraw);
        }
        return;
    }

    let Some(active_id) = focus_state.active_id() else {
        return;
    };
    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        if ctrl && !alt && !super_key && event.key_code == KeyCode::KeyT {
            hud_commands.write(HudIntent::ClearDoneTerminalTasks(active_id));
            break;
        }

        if ctrl || alt || super_key {
            continue;
        }

        match event.key_code {
            KeyCode::Enter => {
                modal_state.open_message_box(&mut input_capture, active_id);
                redraws.write(RequestRedraw);
                break;
            }
            KeyCode::KeyT => {
                let note_text = terminal_manager
                    .get(active_id)
                    .and_then(|terminal| notes_state.note_text(&terminal.session_name))
                    .unwrap_or_default()
                    .to_owned();
                modal_state.open_task_dialog(&mut input_capture, active_id, &note_text);
                redraws.write(RequestRedraw);
                break;
            }
            KeyCode::KeyN => {
                hud_commands.write(HudIntent::ConsumeNextTerminalTask(active_id));
                break;
            }
            _ => {}
        }
    }
}

pub(crate) fn keyboard_input_to_terminal_command(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> Option<TerminalCommand> {
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

pub(crate) fn ctrl_sequence(key_code: KeyCode) -> Option<&'static str> {
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
