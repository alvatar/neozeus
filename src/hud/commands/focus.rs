use crate::{
    hud::HudState,
    terminals::{
        mark_terminal_sessions_dirty, TerminalManager, TerminalSessionPersistenceState,
        TerminalViewState,
    },
};
use bevy::{prelude::*, window::RequestRedraw};

pub(crate) fn apply_terminal_focus_requests(
    mut requests: MessageReader<crate::hud::TerminalFocusRequest>,
    time: Res<Time>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut hud_state: ResMut<HudState>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
    mut view_state: ResMut<TerminalViewState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        terminal_manager.focus_terminal(request.terminal_id);
        hud_state.reconcile_direct_terminal_input(terminal_manager.active_id());
        view_state.focus_terminal(terminal_manager.active_id());
        mark_terminal_sessions_dirty(&mut session_persistence, Some(&time));
        redraws.write(RequestRedraw);
    }
}
