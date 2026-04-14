use crate::{app::AppSessionState, hud::HudInputCaptureState};
use bevy::prelude::Window;

/// Canonical keyboard ownership route for one frame of input handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyboardRoute {
    Ignore,
    DirectInput,
    ResetDialog,
    AegisDialog,
    RenameAgentDialog,
    CloneAgentDialog,
    CreateAgentDialog,
    MessageDialog,
    TaskDialog,
    Primary,
}

/// Resolves keyboard precedence once for the current frame.
///
/// The order is architectural: window focus gates all keyboard handling, direct-input mode owns the
/// keyboard before any modal/editor route, reset confirmation outranks every other dialog, and only
/// when no more specific owner is active does the primary non-modal shortcut route run.
pub(crate) fn resolve_keyboard_route(
    app_session: &AppSessionState,
    input_capture: &HudInputCaptureState,
    primary_window: &Window,
) -> KeyboardRoute {
    if !primary_window.focused {
        return KeyboardRoute::Ignore;
    }
    if input_capture.direct_input_terminal.is_some() {
        return KeyboardRoute::DirectInput;
    }
    if app_session.reset_dialog.visible {
        return KeyboardRoute::ResetDialog;
    }
    if app_session.aegis_dialog.visible {
        return KeyboardRoute::AegisDialog;
    }
    if app_session.rename_agent_dialog.visible {
        return KeyboardRoute::RenameAgentDialog;
    }
    if app_session.clone_agent_dialog.visible {
        return KeyboardRoute::CloneAgentDialog;
    }
    if app_session.create_agent_dialog.visible {
        return KeyboardRoute::CreateAgentDialog;
    }
    if app_session.composer.message_editor.visible {
        return KeyboardRoute::MessageDialog;
    }
    if app_session.composer.task_editor.visible {
        return KeyboardRoute::TaskDialog;
    }
    KeyboardRoute::Primary
}

#[cfg(test)]
mod tests {
    use super::{resolve_keyboard_route, KeyboardRoute};
    use crate::{app::AppSessionState, hud::HudInputCaptureState, terminals::TerminalId};
    use bevy::prelude::Window;

    #[test]
    fn unfocused_window_ignores_keyboard_route() {
        let window = Window {
            focused: false,
            ..Default::default()
        };
        assert_eq!(
            resolve_keyboard_route(
                &AppSessionState::default(),
                &HudInputCaptureState::default(),
                &window,
            ),
            KeyboardRoute::Ignore
        );
    }

    #[test]
    fn direct_input_route_outranks_all_dialogs() {
        let window = Window {
            focused: true,
            ..Default::default()
        };
        let mut app_session = AppSessionState::default();
        app_session.reset_dialog.open();
        app_session.aegis_dialog.visible = true;
        app_session.rename_agent_dialog.visible = true;
        let input_capture = HudInputCaptureState {
            direct_input_terminal: Some(TerminalId(7)),
        };
        assert_eq!(
            resolve_keyboard_route(&app_session, &input_capture, &window),
            KeyboardRoute::DirectInput
        );
    }

    #[test]
    fn reset_dialog_outranks_other_modal_routes() {
        let window = Window {
            focused: true,
            ..Default::default()
        };
        let mut app_session = AppSessionState::default();
        app_session.reset_dialog.open();
        app_session.aegis_dialog.visible = true;
        app_session.rename_agent_dialog.visible = true;
        app_session.clone_agent_dialog.visible = true;
        app_session.create_agent_dialog.visible = true;
        app_session.composer.message_editor.visible = true;
        app_session.composer.task_editor.visible = true;
        assert_eq!(
            resolve_keyboard_route(&app_session, &HudInputCaptureState::default(), &window),
            KeyboardRoute::ResetDialog
        );
    }

    #[test]
    fn message_dialog_outranks_task_dialog_and_primary() {
        let window = Window {
            focused: true,
            ..Default::default()
        };
        let mut app_session = AppSessionState::default();
        app_session.composer.message_editor.visible = true;
        app_session.composer.task_editor.visible = true;
        assert_eq!(
            resolve_keyboard_route(&app_session, &HudInputCaptureState::default(), &window),
            KeyboardRoute::MessageDialog
        );
    }

    #[test]
    fn task_dialog_outranks_primary() {
        let window = Window {
            focused: true,
            ..Default::default()
        };
        let mut app_session = AppSessionState::default();
        app_session.composer.task_editor.visible = true;
        assert_eq!(
            resolve_keyboard_route(&app_session, &HudInputCaptureState::default(), &window),
            KeyboardRoute::TaskDialog
        );
    }

    #[test]
    fn primary_route_is_fallback_when_no_other_owner_is_active() {
        let window = Window {
            focused: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_keyboard_route(
                &AppSessionState::default(),
                &HudInputCaptureState::default(),
                &window,
            ),
            KeyboardRoute::Primary
        );
    }
}
