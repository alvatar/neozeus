use crate::terminals::{TerminalCommand, TerminalManager};
use bevy::prelude::*;

pub(crate) fn apply_terminal_send_requests(
    mut requests: MessageReader<crate::hud::TerminalSendRequest>,
    terminal_manager: Res<TerminalManager>,
) {
    for request in requests.read() {
        match request {
            crate::hud::TerminalSendRequest::Active(command) => {
                if let Some(bridge) = terminal_manager.active_bridge() {
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
