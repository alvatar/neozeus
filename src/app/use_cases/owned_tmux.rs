use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::session::{AppSessionState, VisibilityMode},
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        ActiveTerminalContentState, OwnedTmuxSessionInfo, OwnedTmuxSessionStore,
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
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    app_session: &mut AppSessionState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
    runtime_spawner: &TerminalRuntimeSpawner,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    active_terminal_content: &mut ActiveTerminalContentState,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    let selected_session = owned_tmux_sessions
        .sessions
        .iter()
        .find(|session| session.session_uid == session_uid)
        .cloned()
        .or_else(|| {
            runtime_spawner
                .list_owned_tmux_sessions()
                .ok()?
                .into_iter()
                .find(|session| session.session_uid == session_uid)
        });
    let owner_agent_id = selected_session
        .as_ref()
        .and_then(|session: &OwnedTmuxSessionInfo| {
            agent_catalog.find_by_uid(&session.owner_agent_uid)
        });
    let owner_terminal_id =
        owner_agent_id.and_then(|agent_id| runtime_index.primary_terminal(agent_id));

    active_terminal_content.select_owned_tmux(session_uid.to_owned(), owner_terminal_id);

    if let Some(agent_id) = owner_agent_id {
        app_session.active_agent = Some(agent_id);
    }
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
            owned_tmux_sessions
                .sessions
                .retain(|session| session.session_uid != session_uid);
            active_terminal_content.clear();
        }
        Err(error) => {
            let listed_sessions = runtime_spawner.list_owned_tmux_sessions().ok();
            let session_still_exists = listed_sessions
                .as_ref()
                .map(|sessions| {
                    sessions
                        .iter()
                        .any(|session| session.session_uid == session_uid)
                })
                .unwrap_or_else(|| {
                    owned_tmux_sessions
                        .sessions
                        .iter()
                        .any(|session| session.session_uid == session_uid)
                });
            if let Some(sessions) = listed_sessions {
                owned_tmux_sessions.sessions = sessions;
            } else if !session_still_exists {
                owned_tmux_sessions
                    .sessions
                    .retain(|session| session.session_uid != session_uid);
            }
            if session_still_exists {
                active_terminal_content.set_last_error(error);
            } else {
                active_terminal_content.clear();
            }
        }
    }

    redraws.write(RequestRedraw);
}
