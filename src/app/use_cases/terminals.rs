use crate::terminals::{
    TerminalFocusState, TerminalManager, TerminalPresentationStore, TerminalViewState,
};
use bevy::{prelude::MessageWriter, window::RequestRedraw};

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

pub(crate) fn toggle_active_display_mode(
    focus_state: &TerminalFocusState,
    presentation_store: &mut TerminalPresentationStore,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    presentation_store.toggle_active_display_mode(focus_state.active_id());
    redraws.write(RequestRedraw);
}

pub(crate) fn reset_active_view(
    focus_state: &TerminalFocusState,
    view_state: &mut TerminalViewState,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    view_state.distance = 10.0;
    view_state.reset_active_offset(focus_state.active_id());
    redraws.write(RequestRedraw);
}
