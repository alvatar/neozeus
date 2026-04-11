use super::*;

/// Reads the three modifier families that matter to NeoZeus shortcut handling.
///
/// The return value is `(ctrl, alt, super)` and deliberately merges left/right variants so the rest
/// of the input code can reason about logical modifiers instead of physical keys.
pub(super) fn has_plain_modifiers(keys: &ButtonInput<KeyCode>) -> (bool, bool, bool) {
    (
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight),
    )
}

/// Decides whether a keyboard event means "spawn a normal terminal".
///
/// The binding is intentionally plain `z` on key press with no Ctrl/Alt/Super modifiers. The helper
/// does not emit commands itself; it just encapsulates the binding policy so systems and tests can
/// share the same rule.
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

/// Decides whether a keyboard event means "open the clone-agent dialog".
pub(crate) fn should_open_clone_agent_dialog(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::KeyC {
        return false;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    !(ctrl || alt || super_key)
}

/// Decides whether a keyboard event should kill the currently active terminal session.
///
/// The shortcut is a plain `Ctrl+k` press. Like the other `should_*` helpers, this function only
/// classifies the event; lifecycle side effects happen in the higher-level system.
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

/// Decides whether a keyboard event should exit the whole application.
///
/// NeoZeus uses plain `F10` with no modifiers for this so the exit path stays orthogonal to terminal
/// key handling and to the modal editor shortcuts.
pub(crate) fn should_exit_application(event: &KeyboardInput, keys: &ButtonInput<KeyCode>) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::F10 {
        return false;
    }
    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    !(ctrl || alt || super_key)
}

#[allow(
    clippy::too_many_arguments,
    reason = "global shortcut handling needs window focus, selection, catalog, session, and redraw output together"
)]
/// Watches unfocused-by-modal keyboard input for the global create-agent shortcut.
///
/// The system exits early whenever the primary window is unfocused or a HUD modal currently owns the
/// keyboard. Otherwise it scans the frame's keyboard events and opens the create-agent dialog on the
/// plain global spawn binding.
pub(crate) fn handle_global_terminal_spawn_shortcut(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    agent_catalog: Res<AgentCatalog>,
    selection: Option<Res<crate::hud::AgentListSelection>>,
    mut app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    if app_session.keyboard_capture_active(&input_capture) || !primary_window.focused {
        return;
    }

    let default_selection = crate::hud::AgentListSelection::None;
    let selection = selection.as_deref().unwrap_or(&default_selection);

    for event in messages.read() {
        if should_spawn_terminal_globally(event, &keys) {
            app_session.create_agent_dialog.open(CreateAgentKind::Pi);
            redraws.write(RequestRedraw);
            break;
        }
        if should_open_clone_agent_dialog(event, &keys) {
            let crate::hud::AgentListSelection::Agent(agent_id) = *selection else {
                continue;
            };
            let Some(kind) = agent_catalog.kind(agent_id) else {
                continue;
            };
            let supports_clone = match kind {
                crate::agents::AgentKind::Pi => {
                    agent_catalog.clone_source_session_path(agent_id).is_some()
                }
                crate::agents::AgentKind::Claude | crate::agents::AgentKind::Codex => {
                    agent_catalog.recovery_spec(agent_id).is_some()
                }
                crate::agents::AgentKind::Terminal | crate::agents::AgentKind::Verifier => false,
            };
            if !supports_clone {
                continue;
            }
            let current_label = agent_catalog.label(agent_id).unwrap_or("AGENT");
            app_session
                .clone_agent_dialog
                .open(agent_id, kind, current_label);
            redraws.write(RequestRedraw);
            break;
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "lifecycle shortcut handling now owns reset-dialog open plus existing selection/exit routing"
)]
pub(crate) fn handle_terminal_lifecycle_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    selection: Option<Res<crate::hud::AgentListSelection>>,
    mut app_commands: MessageWriter<AppCommand>,
    mut app_exits: MessageWriter<AppExit>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    if app_session.keyboard_capture_active(&input_capture) {
        return;
    }

    let default_selection = crate::hud::AgentListSelection::None;
    let selection = selection.as_deref().unwrap_or(&default_selection);

    for event in messages.read() {
        if should_exit_application(event, &keys) {
            app_exits.write(AppExit::Success);
            break;
        }
        let (ctrl, alt, super_key) = has_plain_modifiers(&keys);
        if ctrl
            && alt
            && !super_key
            && event.state == ButtonState::Pressed
            && event.key_code == KeyCode::KeyR
        {
            app_session.reset_dialog.open();
            app_session.recovery_status.show_reset_requested();
            redraws.write(RequestRedraw);
            break;
        }
        if should_kill_active_terminal(event, &keys) {
            match *selection {
                crate::hud::AgentListSelection::OwnedTmux(_) => {
                    app_commands.write(AppCommand::OwnedTmux(OwnedTmuxCommand::KillSelected));
                }
                crate::hud::AgentListSelection::Agent(_) => {
                    app_commands.write(AppCommand::Agent(AppAgentCommand::KillSelected));
                }
                crate::hud::AgentListSelection::None => {}
            }
        }
    }
}
