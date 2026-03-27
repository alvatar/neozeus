use crate::terminals::{TerminalFocusState, TerminalPresentationStore, TerminalViewState};
use bevy::{prelude::*, window::RequestRedraw};

/// Applies HUD-originated view changes to the active terminal presentation.
///
/// The current request surface is intentionally small: toggle pixel-perfect mode or reset the shared
/// view state back to its default distance and zero offset for the active terminal. Every successful
/// request forces a redraw because both operations change visible geometry immediately.
pub(crate) fn apply_terminal_view_requests(
    mut requests: MessageReader<crate::hud::TerminalViewRequest>,
    focus_state: Res<TerminalFocusState>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    mut view_state: ResMut<TerminalViewState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        match request {
            crate::hud::TerminalViewRequest::ToggleActiveDisplayMode => {
                presentation_store.toggle_active_display_mode(focus_state.active_id());
            }
            crate::hud::TerminalViewRequest::ResetActiveView => {
                view_state.distance = 10.0;
                view_state.reset_active_offset(focus_state.active_id());
            }
        }
        redraws.write(RequestRedraw);
    }
}
