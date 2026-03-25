use crate::terminals::{TerminalManager, TerminalPresentationStore, TerminalViewState};
use bevy::{prelude::*, window::RequestRedraw};

pub(crate) fn apply_terminal_view_requests(
    mut requests: MessageReader<crate::hud::TerminalViewRequest>,
    terminal_manager: Res<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    mut view_state: ResMut<TerminalViewState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        match request {
            crate::hud::TerminalViewRequest::ToggleActiveDisplayMode => {
                presentation_store.toggle_active_display_mode(terminal_manager.active_id());
            }
            crate::hud::TerminalViewRequest::ResetActiveView => {
                view_state.distance = 10.0;
                view_state.reset_active_offset(terminal_manager.active_id());
            }
        }
        redraws.write(RequestRedraw);
    }
}
