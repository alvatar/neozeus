use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex},
    app::AppStatePersistenceState,
    conversations::{
        mark_conversations_dirty, AgentTaskStore, ConversationPersistenceState, ConversationStore,
    },
    hud::{HudInputCaptureState, TerminalVisibilityState},
    terminals::{
        kill_terminal_session_and_remove, mark_terminal_notes_dirty, ActiveTerminalContentState,
        OwnedTmuxSessionStore, TerminalFocusState, TerminalManager, TerminalNotesState,
        TerminalRuntimeSpawner, TerminalViewState,
    },
};

use super::{
    super::session::{AppSessionState, VisibilityMode},
    apply_focus_intent,
};
use bevy::{prelude::*, window::RequestRedraw};

/// Handles adjacent agent in catalog.
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
/// Deletes the selected agent row and updates the remaining selection/focus state.
pub(crate) fn kill_selected_agent(
    selected_agent: AgentId,
    time: &Time,
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
    selection: &mut crate::hud::AgentListSelection,
    task_store: &mut AgentTaskStore,
    conversations: &mut ConversationStore,
    conversation_persistence: &mut ConversationPersistenceState,
    notes_state: &mut TerminalNotesState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    active_terminal_content: &mut ActiveTerminalContentState,
    input_capture: &mut HudInputCaptureState,
    app_state_persistence: &mut AppStatePersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<Option<AgentId>, String> {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let replacement_agent = adjacent_agent_in_catalog(agent_catalog, selected_agent);
    let owner_agent_uid = agent_catalog
        .uid(selected_agent)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing stable uid for agent {}", selected_agent.0))?;
    if let Err(error) = runtime_spawner.kill_owned_tmux_sessions_for_agent(&owner_agent_uid) {
        let owner_tmux_still_exists = runtime_spawner
            .list_owned_tmux_sessions()
            .map(|sessions| {
                sessions
                    .iter()
                    .any(|session| session.owner_agent_uid == owner_agent_uid)
            })
            .unwrap_or(true);
        if owner_tmux_still_exists {
            return Err(error);
        }
    }
    let Some(terminal_id) = runtime_index.primary_terminal(selected_agent) else {
        return Ok(None);
    };
    let Some(session_name) = runtime_index
        .session_name(selected_agent)
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    let removed = kill_terminal_session_and_remove(
        time,
        terminal_manager,
        focus_state,
        runtime_spawner,
        app_state_persistence,
        terminal_id,
        &session_name,
    )?;
    let Some((terminal_id, _session_name)) = removed else {
        return Ok(None);
    };

    let _ = runtime_index.remove_terminal(terminal_id);
    let removed_agent_uid = agent_catalog.uid(selected_agent).map(str::to_owned);
    let _ = agent_catalog.remove(selected_agent);
    let removed_tasks = task_store.remove_agent(selected_agent);
    if conversations.remove_agent(selected_agent) {
        mark_conversations_dirty(conversation_persistence, Some(time));
    }
    if let Some(agent_uid) = removed_agent_uid.as_deref() {
        if notes_state.remove_note_text_by_agent_uid(agent_uid) {
            mark_terminal_notes_dirty(notes_state, Some(time));
        }
    } else if removed_tasks {
        mark_terminal_notes_dirty(notes_state, Some(time));
    }
    view_state.forget_terminal(terminal_id);
    app_session.composer.unbind_agent(selected_agent);
    if let Some(replacement_agent) = replacement_agent {
        app_session
            .focus_intent
            .focus_agent(replacement_agent, app_session.visibility_mode());
    } else {
        app_session.focus_intent.clear(VisibilityMode::ShowAll);
    }
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
    Ok(Some(selected_agent))
}
