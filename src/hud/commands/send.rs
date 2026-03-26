use crate::terminals::{TerminalCommand, TerminalFocusState, TerminalManager};
use bevy::prelude::*;

// Applies terminal send requests.
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
