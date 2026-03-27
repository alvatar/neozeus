use crate::terminals::{TerminalCommand, TerminalFocusState, TerminalManager};
use bevy::prelude::*;

/// Delivers queued command-send requests to either the active terminal or a specific terminal.
///
/// The system does no command interpretation itself. It simply resolves the correct terminal bridge—
/// from the current focus state or by direct id lookup—and forwards the string payload as a
/// `TerminalCommand::SendCommand` when a target terminal exists.
pub(crate) fn apply_terminal_send_requests(
    mut requests: MessageReader<crate::hud::TerminalSendRequest>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
) {
    for request in requests.read() {
        match request {
            crate::hud::TerminalSendRequest::Active(command) => {
                if let Some(bridge) = focus_state.active_bridge(&terminal_manager) {
                    bridge.send(TerminalCommand::SendCommand(command.clone()));
                }
            }
            crate::hud::TerminalSendRequest::Target {
                terminal_id,
                command,
            } => {
                if let Some(terminal) = terminal_manager.get(*terminal_id) {
                    terminal
                        .bridge
                        .send(TerminalCommand::SendCommand(command.clone()));
                }
            }
        }
    }
}
