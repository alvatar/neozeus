use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex},
    app::{AppSessionState, VisibilityMode},
    conversations::AgentTaskStore,
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        kill_active_terminal_session_and_remove, TerminalFocusState, TerminalManager,
        TerminalRuntimeSpawner, TerminalSessionPersistenceState, TerminalViewState,
    },
};
use bevy::{prelude::*, window::RequestRedraw};

fn adjacent_agent_in_catalog(catalog: &AgentCatalog, agent_id: AgentId) -> Option<AgentId> {
    let index = catalog
        .order
        .iter()
        .position(|existing| *existing == agent_id)?;
    if index > 0 {
        catalog.order.get(index - 1).copied()
    } else {
        catalog.order.get(index + 1).copied()
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "kill spans daemon, agent, session, and projection state"
)]
pub(crate) fn kill_active_agent(
    time: &Time,
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
    task_store: &mut AgentTaskStore,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    input_capture: &mut HudInputCaptureState,
    session_persistence: &mut TerminalSessionPersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<Option<AgentId>, String> {
    let Some(active_agent) = app_session.active_agent else {
        return Ok(None);
    };
    let replacement_agent = adjacent_agent_in_catalog(agent_catalog, active_agent);
    let removed = kill_active_terminal_session_and_remove(
        time,
        terminal_manager,
        focus_state,
        runtime_spawner,
        session_persistence,
    )?;
    let Some((terminal_id, _session_name)) = removed else {
        return Ok(None);
    };

    let _ = runtime_index.remove_terminal(terminal_id);
    let _ = agent_catalog.remove(active_agent);
    let _ = task_store.remove_agent(active_agent);
    view_state.forget_terminal(terminal_id);
    app_session.composer.unbind_agent(active_agent);
    app_session.active_agent = replacement_agent;

    if let Some(replacement_agent) = replacement_agent {
        if let Some(replacement_terminal) = runtime_index.primary_terminal(replacement_agent) {
            focus_state.focus_terminal(terminal_manager, replacement_terminal);
            #[cfg(test)]
            terminal_manager.replace_test_focus_state(focus_state);
            view_state.focus_terminal(Some(replacement_terminal));
            visibility_state.policy = match app_session.visibility_mode {
                VisibilityMode::ShowAll => TerminalVisibilityPolicy::ShowAll,
                VisibilityMode::FocusedOnly => {
                    TerminalVisibilityPolicy::Isolate(replacement_terminal)
                }
            };
        }
    } else {
        visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
        view_state.focus_terminal(None);
    }
    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
    redraws.write(RequestRedraw);
    Ok(Some(active_agent))
}
