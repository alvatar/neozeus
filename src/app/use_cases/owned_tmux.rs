use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::session::{AppSessionState, VisibilityMode},
    hud::{HudInputCaptureState, TerminalVisibilityState},
    terminals::{
        refresh_owned_tmux_sessions_now, ActiveTerminalContentState, OwnedTmuxSessionStore,
        TerminalFocusState, TerminalManager, TerminalRuntimeSpawner, TerminalViewState,
    },
};

use super::{clear_focus_without_persist, focus_owned_tmux_without_persist};
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
    let mut focus_ctx = super::FocusMutationContext {
        session: app_session,
        projection: super::FocusProjectionContext {
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
        },
        redraws,
    };
    focus_owned_tmux_without_persist(session_uid, &mut focus_ctx);
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
        | crate::app::session::FocusIntentTarget::Agent(_)
        | crate::app::session::FocusIntentTarget::Terminal(_) => active_terminal_content
            .selected_owned_tmux_session_uid()
            .map(str::to_owned),
    }) else {
        return;
    };

    match runtime_spawner.kill_owned_tmux_session(&session_uid) {
        Ok(()) => {
            owned_tmux_sessions.record_removed_session(&session_uid);
            let _ = refresh_owned_tmux_sessions_now(runtime_spawner, owned_tmux_sessions);
            let mut focus_ctx = super::FocusMutationContext {
                session: app_session,
                projection: super::FocusProjectionContext {
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
                },
                redraws,
            };
            clear_focus_without_persist(VisibilityMode::ShowAll, &mut focus_ctx);
        }
        Err(error) => {
            let _ = refresh_owned_tmux_sessions_now(runtime_spawner, owned_tmux_sessions);
            if owned_tmux_sessions.session(&session_uid).is_some() {
                active_terminal_content.set_last_error(error);
                redraws.write(RequestRedraw);
            } else {
                let mut focus_ctx = super::FocusMutationContext {
                    session: app_session,
                    projection: super::FocusProjectionContext {
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
                    },
                    redraws,
                };
                clear_focus_without_persist(VisibilityMode::ShowAll, &mut focus_ctx);
            }
        }
    }
}
