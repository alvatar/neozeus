use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::session::{AppSessionState, VisibilityMode},
    hud::{HudInputCaptureState, TerminalVisibilityState},
    terminals::{
        refresh_owned_tmux_sessions_now, ActiveTerminalContentState, OwnedTmuxSessionStore,
        TerminalFocusState, TerminalManager, TerminalRuntimeSpawner, TerminalViewState,
    },
};

use super::apply_focus_intent;
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
    app_session
        .focus_intent
        .focus_owned_tmux(session_uid.to_owned());
    apply_focus_intent(
        app_session,
        agent_catalog,
        runtime_index,
        owned_tmux_sessions,
        selection,
        active_terminal_content,
        terminal_manager,
        focus_state,
        input_capture,
        view_state,
        visibility_state,
    );

    redraws.write(RequestRedraw);
}

#[allow(
    clippy::too_many_arguments,
    reason = "owned tmux kill now re-applies the unified focus/content intent projections"
)]
/// Kills the currently selected owned tmux child session.
pub(crate) fn kill_selected_owned_tmux(
    app_session: &mut AppSessionState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
    runtime_spawner: &TerminalRuntimeSpawner,
    selection: &mut crate::hud::AgentListSelection,
    owned_tmux_sessions: &mut OwnedTmuxSessionStore,
    active_terminal_content: &mut ActiveTerminalContentState,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    let Some(session_uid) = (match &app_session.focus_intent.target {
        crate::app::session::FocusIntentTarget::OwnedTmux(session_uid) => Some(session_uid.clone()),
        crate::app::session::FocusIntentTarget::None
        | crate::app::session::FocusIntentTarget::Agent(_) => active_terminal_content
            .selected_owned_tmux_session_uid()
            .map(str::to_owned),
    }) else {
        return;
    };

    match runtime_spawner.kill_owned_tmux_session(&session_uid) {
        Ok(()) => {
            owned_tmux_sessions.record_removed_session(&session_uid);
            let _ = refresh_owned_tmux_sessions_now(runtime_spawner, owned_tmux_sessions);
            app_session.focus_intent.clear(VisibilityMode::ShowAll);
            apply_focus_intent(
                app_session,
                agent_catalog,
                runtime_index,
                owned_tmux_sessions,
                selection,
                active_terminal_content,
                terminal_manager,
                focus_state,
                input_capture,
                view_state,
                visibility_state,
            );
        }
        Err(error) => {
            let _ = refresh_owned_tmux_sessions_now(runtime_spawner, owned_tmux_sessions);
            if owned_tmux_sessions.session(&session_uid).is_some() {
                active_terminal_content.set_last_error(error);
            } else {
                app_session.focus_intent.clear(VisibilityMode::ShowAll);
                apply_focus_intent(
                    app_session,
                    agent_catalog,
                    runtime_index,
                    owned_tmux_sessions,
                    selection,
                    active_terminal_content,
                    terminal_manager,
                    focus_state,
                    input_capture,
                    view_state,
                    visibility_state,
                );
            }
        }
    }

    redraws.write(RequestRedraw);
}
