use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::session::{AppSessionState, VisibilityMode},
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        refresh_owned_tmux_sessions_now, ActiveTerminalContentState, OwnedTmuxSessionStore,
        TerminalFocusState, TerminalManager, TerminalRuntimeSpawner, TerminalViewState,
    },
};
use bevy::{prelude::MessageWriter, window::RequestRedraw};

#[allow(
    clippy::too_many_arguments,
    reason = "selecting tmux spans agent focus, visibility, input capture, runtime lookup, and terminal content override state"
)]
/// Selects one owned tmux child as the active terminal content source.
pub(crate) fn select_owned_tmux(
    session_uid: &str,
    selection: &mut crate::hud::AgentListSelection,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    app_session: &mut AppSessionState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
    runtime_spawner: &TerminalRuntimeSpawner,
    owned_tmux_sessions: &mut OwnedTmuxSessionStore,
    active_terminal_content: &mut ActiveTerminalContentState,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    if owned_tmux_sessions.session(session_uid).is_none() {
        let _ = refresh_owned_tmux_sessions_now(runtime_spawner, owned_tmux_sessions);
    }
    let owner_agent_id = owned_tmux_sessions
        .session(session_uid)
        .and_then(|session| agent_catalog.find_by_uid(&session.owner_agent_uid));
    let owner_terminal_id =
        owner_agent_id.and_then(|agent_id| runtime_index.primary_terminal(agent_id));

    active_terminal_content.select_owned_tmux(session_uid.to_owned(), owner_terminal_id);
    *selection = crate::hud::AgentListSelection::OwnedTmux(session_uid.to_owned());

    if let Some(terminal_id) = owner_terminal_id {
        focus_state.focus_terminal(terminal_manager, terminal_id);
        #[cfg(test)]
        terminal_manager.replace_test_focus_state(focus_state);
        view_state.focus_terminal(Some(terminal_id));
        visibility_state.policy = match app_session.visibility_mode {
            VisibilityMode::ShowAll => TerminalVisibilityPolicy::ShowAll,
            VisibilityMode::FocusedOnly => TerminalVisibilityPolicy::Isolate(terminal_id),
        };
        input_capture.reconcile_direct_terminal_input(focus_state.active_id());
    }

    redraws.write(RequestRedraw);
}

/// Kills the currently selected owned tmux child session.
pub(crate) fn kill_selected_owned_tmux(
    runtime_spawner: &TerminalRuntimeSpawner,
    selection: &mut crate::hud::AgentListSelection,
    owned_tmux_sessions: &mut OwnedTmuxSessionStore,
    active_terminal_content: &mut ActiveTerminalContentState,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    let Some(session_uid) = active_terminal_content
        .selected_owned_tmux_session_uid()
        .map(str::to_owned)
    else {
        return;
    };

    match runtime_spawner.kill_owned_tmux_session(&session_uid) {
        Ok(()) => {
            owned_tmux_sessions.record_removed_session(&session_uid);
            let _ = refresh_owned_tmux_sessions_now(runtime_spawner, owned_tmux_sessions);
            active_terminal_content.clear();
            *selection = crate::hud::AgentListSelection::None;
        }
        Err(error) => {
            let _ = refresh_owned_tmux_sessions_now(runtime_spawner, owned_tmux_sessions);
            if owned_tmux_sessions.session(&session_uid).is_some() {
                active_terminal_content.set_last_error(error);
            } else {
                active_terminal_content.clear();
                *selection = crate::hud::AgentListSelection::None;
            }
        }
    }

    redraws.write(RequestRedraw);
}
