use crate::{
    hud::{TerminalVisibilityPolicy, TerminalVisibilityRequest, TerminalVisibilityState},
    terminals::{append_debug_log, TerminalManager},
};
use bevy::{prelude::*, window::RequestRedraw};

/// Applies show-all and isolate visibility requests to the terminal presentation policy.
///
/// Isolate requests are validated against the current terminal registry so the app does not retain an
/// impossible isolate target after a terminal disappears. Every change is logged and followed by a
/// redraw request because visibility policy directly affects which panels are presented.
pub(crate) fn apply_visibility_requests(
    mut requests: MessageReader<TerminalVisibilityRequest>,
    terminal_manager: Res<TerminalManager>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        match request {
            TerminalVisibilityRequest::Isolate(terminal_id) => {
                visibility_state.policy = if terminal_manager.get(*terminal_id).is_some() {
                    TerminalVisibilityPolicy::Isolate(*terminal_id)
                } else {
                    TerminalVisibilityPolicy::ShowAll
                };
                append_debug_log(format!("hud visibility {:?}", visibility_state.policy));
            }
            TerminalVisibilityRequest::ShowAll => {
                visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
                append_debug_log("hud visibility show-all");
            }
        }
        redraws.write(RequestRedraw);
    }
}
