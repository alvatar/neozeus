use crate::terminals::TerminalManager;

/// Handles send terminal command.
pub(crate) fn send_terminal_command(
    terminal_id: crate::terminals::TerminalId,
    command: &str,
    terminal_manager: &TerminalManager,
) {
    if let Some(terminal) = terminal_manager.get(terminal_id) {
        terminal
            .bridge
            .send(crate::terminals::TerminalCommand::SendCommand(
                command.to_owned(),
            ));
    }
}
