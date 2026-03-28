use crate::{
    agents::{AgentId, AgentRuntimeIndex},
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        mark_terminal_sessions_dirty, TerminalFocusState, TerminalManager,
        TerminalSessionPersistenceState, TerminalViewState,
    },
};

use super::super::session::{AppSessionState, VisibilityMode};
use bevy::{prelude::Time, window::RequestRedraw};

#[allow(
    clippy::too_many_arguments,
    reason = "focus fans out to runtime-facing mirrors"
)]
/// Focuses agent.
pub(crate) fn focus_agent(
    agent_id: AgentId,
    session: &mut AppSessionState,
    runtime_index: &AgentRuntimeIndex,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    session_persistence: &mut TerminalSessionPersistenceState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
    time: &Time,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let Some(terminal_id) = runtime_index.primary_terminal(agent_id) else {
        return;
    };
    session.active_agent = Some(agent_id);
    focus_state.focus_terminal(terminal_manager, terminal_id);
    #[cfg(test)]
    terminal_manager.replace_test_focus_state(focus_state);
    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
    view_state.focus_terminal(Some(terminal_id));
    visibility_state.policy = match session.visibility_mode {
        VisibilityMode::ShowAll => TerminalVisibilityPolicy::ShowAll,
        VisibilityMode::FocusedOnly => TerminalVisibilityPolicy::Isolate(terminal_id),
    };
    mark_terminal_sessions_dirty(session_persistence, Some(time));
    redraws.write(RequestRedraw);
}
