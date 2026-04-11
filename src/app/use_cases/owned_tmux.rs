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

pub(crate) struct OwnedTmuxContext<'a, 'w> {
    pub(crate) app_session: &'a mut AppSessionState,
    pub(crate) selection: &'a mut crate::hud::AgentListSelection,
    pub(crate) agent_catalog: &'a AgentCatalog,
    pub(crate) runtime_index: &'a AgentRuntimeIndex,
    pub(crate) terminal_manager: &'a mut TerminalManager,
    pub(crate) focus_state: &'a mut TerminalFocusState,
    pub(crate) input_capture: &'a mut HudInputCaptureState,
    pub(crate) view_state: &'a mut TerminalViewState,
    pub(crate) visibility_state: &'a mut TerminalVisibilityState,
    pub(crate) runtime_spawner: &'a TerminalRuntimeSpawner,
    pub(crate) owned_tmux_sessions: &'a mut OwnedTmuxSessionStore,
    pub(crate) active_terminal_content: &'a mut ActiveTerminalContentState,
    pub(crate) redraws: &'a mut MessageWriter<'w, RequestRedraw>,
}

/// Selects one owned tmux child as the active terminal content source.
pub(crate) fn select_owned_tmux(session_uid: &str, ctx: &mut OwnedTmuxContext<'_, '_>) {
    if ctx.owned_tmux_sessions.session(session_uid).is_none() {
        let _ = refresh_owned_tmux_sessions_now(ctx.runtime_spawner, ctx.owned_tmux_sessions);
    }
    let mut focus_ctx = super::FocusMutationContext {
        session: ctx.app_session,
        projection: super::FocusProjectionContext {
            agent_catalog: ctx.agent_catalog,
            runtime_index: ctx.runtime_index,
            owned_tmux_sessions: ctx.owned_tmux_sessions,
            selection: ctx.selection,
            active_terminal_content: ctx.active_terminal_content,
            terminal_manager: ctx.terminal_manager,
            focus_state: ctx.focus_state,
            input_capture: ctx.input_capture,
            view_state: ctx.view_state,
            visibility_state: ctx.visibility_state,
        },
        redraws: ctx.redraws,
    };
    focus_owned_tmux_without_persist(session_uid, &mut focus_ctx);
}

/// Kills the currently selected owned tmux child session.
pub(crate) fn kill_selected_owned_tmux(ctx: &mut OwnedTmuxContext<'_, '_>) {
    let Some(session_uid) = (match &ctx.app_session.focus_intent.target {
        crate::app::session::FocusIntentTarget::OwnedTmux(session_uid) => Some(session_uid.clone()),
        crate::app::session::FocusIntentTarget::None
        | crate::app::session::FocusIntentTarget::Agent(_)
        | crate::app::session::FocusIntentTarget::Terminal(_) => ctx
            .active_terminal_content
            .selected_owned_tmux_session_uid()
            .map(str::to_owned),
    }) else {
        return;
    };

    match ctx.runtime_spawner.kill_owned_tmux_session(&session_uid) {
        Ok(()) => {
            ctx.owned_tmux_sessions.record_removed_session(&session_uid);
            let _ = refresh_owned_tmux_sessions_now(ctx.runtime_spawner, ctx.owned_tmux_sessions);
            let mut focus_ctx = super::FocusMutationContext {
                session: ctx.app_session,
                projection: super::FocusProjectionContext {
                    agent_catalog: ctx.agent_catalog,
                    runtime_index: ctx.runtime_index,
                    owned_tmux_sessions: ctx.owned_tmux_sessions,
                    selection: ctx.selection,
                    active_terminal_content: ctx.active_terminal_content,
                    terminal_manager: ctx.terminal_manager,
                    focus_state: ctx.focus_state,
                    input_capture: ctx.input_capture,
                    view_state: ctx.view_state,
                    visibility_state: ctx.visibility_state,
                },
                redraws: ctx.redraws,
            };
            clear_focus_without_persist(VisibilityMode::ShowAll, &mut focus_ctx);
        }
        Err(error) => {
            let _ = refresh_owned_tmux_sessions_now(ctx.runtime_spawner, ctx.owned_tmux_sessions);
            if ctx.owned_tmux_sessions.session(&session_uid).is_some() {
                ctx.active_terminal_content.set_last_error(error);
                ctx.redraws.write(RequestRedraw);
            } else {
                let mut focus_ctx = super::FocusMutationContext {
                    session: ctx.app_session,
                    projection: super::FocusProjectionContext {
                        agent_catalog: ctx.agent_catalog,
                        runtime_index: ctx.runtime_index,
                        owned_tmux_sessions: ctx.owned_tmux_sessions,
                        selection: ctx.selection,
                        active_terminal_content: ctx.active_terminal_content,
                        terminal_manager: ctx.terminal_manager,
                        focus_state: ctx.focus_state,
                        input_capture: ctx.input_capture,
                        view_state: ctx.view_state,
                        visibility_state: ctx.visibility_state,
                    },
                    redraws: ctx.redraws,
                };
                clear_focus_without_persist(VisibilityMode::ShowAll, &mut focus_ctx);
            }
        }
    }
}
