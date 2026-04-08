use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex},
    app::{mark_app_state_dirty, AppStatePersistenceState},
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        ActiveTerminalContentState, OwnedTmuxSessionStore, TerminalFocusState, TerminalManager,
        TerminalViewState,
    },
};

use super::super::session::{AppSessionState, FocusIntentTarget, VisibilityMode};
use bevy::{prelude::Time, window::RequestRedraw};

fn terminal_visibility_policy(
    visibility_mode: VisibilityMode,
    terminal_id: Option<crate::terminals::TerminalId>,
) -> TerminalVisibilityPolicy {
    match (visibility_mode, terminal_id) {
        (VisibilityMode::FocusedOnly, Some(terminal_id)) => {
            TerminalVisibilityPolicy::Isolate(terminal_id)
        }
        (VisibilityMode::ShowAll, _) | (VisibilityMode::FocusedOnly, None) => {
            TerminalVisibilityPolicy::ShowAll
        }
    }
}

fn reconcile_focus_intent(
    session: &mut AppSessionState,
    agent_catalog: &AgentCatalog,
    _runtime_index: &AgentRuntimeIndex,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
) {
    match &session.focus_intent.target {
        FocusIntentTarget::None => {}
        FocusIntentTarget::Agent(agent_id) => {
            if agent_catalog.uid(*agent_id).is_none() {
                session.focus_intent.clear(VisibilityMode::ShowAll);
            }
        }
        FocusIntentTarget::OwnedTmux(session_uid) => {
            if owned_tmux_sessions.session(session_uid).is_none() {
                session.focus_intent.clear(VisibilityMode::ShowAll);
            }
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "focus intent fans out into selection, active terminal content, focus, view, visibility, and input projections"
)]
pub(crate) fn apply_focus_intent(
    session: &mut AppSessionState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    selection: &mut crate::hud::AgentListSelection,
    active_terminal_content: &mut ActiveTerminalContentState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
) {
    reconcile_focus_intent(session, agent_catalog, runtime_index, owned_tmux_sessions);

    match &session.focus_intent.target {
        FocusIntentTarget::None => {
            *selection = crate::hud::AgentListSelection::None;
            active_terminal_content.clear();
            let _ = focus_state.clear_active_terminal();
            #[cfg(test)]
            terminal_manager.replace_test_focus_state(focus_state);
            view_state.focus_terminal(None);
            visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
        }
        FocusIntentTarget::Agent(agent_id) => {
            let terminal_id = runtime_index.primary_terminal(*agent_id);
            *selection = crate::hud::AgentListSelection::Agent(*agent_id);
            active_terminal_content.clear();
            if let Some(terminal_id) = terminal_id {
                focus_state.focus_terminal(terminal_manager, terminal_id);
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                view_state.focus_terminal(Some(terminal_id));
            } else {
                let _ = focus_state.clear_active_terminal();
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                view_state.focus_terminal(None);
            }
            visibility_state.policy =
                terminal_visibility_policy(session.visibility_mode(), terminal_id);
        }
        FocusIntentTarget::OwnedTmux(session_uid) => {
            *selection = crate::hud::AgentListSelection::OwnedTmux(session_uid.clone());
            let owner_terminal_id = owned_tmux_sessions
                .session(session_uid)
                .and_then(|owned_tmux| agent_catalog.find_by_uid(&owned_tmux.owner_agent_uid))
                .and_then(|agent_id| runtime_index.primary_terminal(agent_id));
            active_terminal_content.select_owned_tmux(session_uid.clone(), owner_terminal_id);
            if let Some(terminal_id) = owner_terminal_id {
                focus_state.focus_terminal(terminal_manager, terminal_id);
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                view_state.focus_terminal(Some(terminal_id));
            } else {
                let _ = focus_state.clear_active_terminal();
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                view_state.focus_terminal(None);
            }
            visibility_state.policy =
                terminal_visibility_policy(session.visibility_mode(), owner_terminal_id);
        }
    }

    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
}

#[allow(
    clippy::too_many_arguments,
    reason = "focus agent updates focus intent plus all runtime-facing mirrors"
)]
/// Focuses agent.
pub(crate) fn focus_agent(
    agent_id: AgentId,
    visibility_mode: VisibilityMode,
    session: &mut AppSessionState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    selection: &mut crate::hud::AgentListSelection,
    active_terminal_content: &mut ActiveTerminalContentState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    app_state_persistence: &mut AppStatePersistenceState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
    time: &Time,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    session.focus_intent.focus_agent(agent_id, visibility_mode);
    apply_focus_intent(
        session,
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
    mark_app_state_dirty(app_state_persistence, Some(time));
    redraws.write(RequestRedraw);
}
