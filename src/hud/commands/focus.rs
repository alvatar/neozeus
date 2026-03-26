use crate::{
    hud::HudInputCaptureState,
    terminals::{
        mark_terminal_sessions_dirty, TerminalFocusState, TerminalManager,
        TerminalSessionPersistenceState, TerminalViewState,
    },
};
use bevy::{prelude::*, window::RequestRedraw};

#[allow(
    clippy::too_many_arguments,
    unused_mut,
    reason = "focus requests update focus state, input capture, persistence, and redraw together"
)]
/// Applies terminal focus requests.
pub(crate) fn apply_terminal_focus_requests(
    mut requests: MessageReader<crate::hud::TerminalFocusRequest>,
    time: Res<Time>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut focus_state: ResMut<TerminalFocusState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
    mut view_state: ResMut<TerminalViewState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        focus_state.focus_terminal(&terminal_manager, request.terminal_id);
        #[cfg(test)]
        terminal_manager.replace_test_focus_state(&focus_state);
        input_capture.reconcile_direct_terminal_input(focus_state.active_id());
        view_state.focus_terminal(focus_state.active_id());
        mark_terminal_sessions_dirty(&mut session_persistence, Some(&time));
        redraws.write(RequestRedraw);
    }
}
