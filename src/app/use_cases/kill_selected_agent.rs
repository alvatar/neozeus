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
    clear_focus_without_persist, focus_agent_without_persist,
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

pub(crate) struct KillSelectedAgentContext<'a, 'w> {
    pub(crate) agent_catalog: &'a mut AgentCatalog,
    pub(crate) runtime_index: &'a mut AgentRuntimeIndex,
    pub(crate) app_session: &'a mut AppSessionState,
    pub(crate) selection: &'a mut crate::hud::AgentListSelection,
    pub(crate) task_store: &'a mut AgentTaskStore,
    pub(crate) conversations: &'a mut ConversationStore,
    pub(crate) conversation_persistence: &'a mut ConversationPersistenceState,
    pub(crate) notes_state: &'a mut TerminalNotesState,
    pub(crate) terminal_manager: &'a mut TerminalManager,
    pub(crate) focus_state: &'a mut TerminalFocusState,
    pub(crate) runtime_spawner: &'a TerminalRuntimeSpawner,
    pub(crate) owned_tmux_sessions: &'a OwnedTmuxSessionStore,
    pub(crate) active_terminal_content: &'a mut ActiveTerminalContentState,
    pub(crate) input_capture: &'a mut HudInputCaptureState,
    pub(crate) app_state_persistence: &'a mut AppStatePersistenceState,
    pub(crate) aegis_policy: &'a mut crate::aegis::AegisPolicyStore,
    pub(crate) aegis_runtime: &'a mut crate::aegis::AegisRuntimeStore,
    pub(crate) visibility_state: &'a mut TerminalVisibilityState,
    pub(crate) view_state: &'a mut TerminalViewState,
    pub(crate) redraws: &'a mut MessageWriter<'w, RequestRedraw>,
}

/// Deletes the selected agent row and updates the remaining selection/focus state.
pub(crate) fn kill_selected_agent(
    selected_agent: AgentId,
    time: &Time,
    ctx: &mut KillSelectedAgentContext<'_, '_>,
) -> Result<Option<AgentId>, String> {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let replacement_agent = adjacent_agent_in_catalog(ctx.agent_catalog, selected_agent);
    let owner_agent_uid = ctx
        .agent_catalog
        .uid(selected_agent)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing stable uid for agent {}", selected_agent.0))?;
    if let Err(error) = ctx
        .runtime_spawner
        .kill_owned_tmux_sessions_for_agent(&owner_agent_uid)
    {
        let owner_tmux_still_exists = ctx
            .runtime_spawner
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
    let Some(terminal_id) = ctx.runtime_index.primary_terminal(selected_agent) else {
        return Ok(None);
    };
    let Some(session_name) = ctx
        .runtime_index
        .session_name(selected_agent)
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    let removed = kill_terminal_session_and_remove(
        time,
        ctx.terminal_manager,
        ctx.focus_state,
        ctx.runtime_spawner,
        ctx.app_state_persistence,
        terminal_id,
        &session_name,
    )?;
    let Some((terminal_id, _session_name)) = removed else {
        return Ok(None);
    };

    let _ = ctx.runtime_index.remove_terminal(terminal_id);
    let removed_agent_uid = ctx.agent_catalog.uid(selected_agent).map(str::to_owned);
    let _ = ctx.agent_catalog.remove(selected_agent);
    let removed_tasks = ctx.task_store.remove_agent(selected_agent);
    if ctx.conversations.remove_agent(selected_agent) {
        mark_conversations_dirty(ctx.conversation_persistence, Some(time));
    }
    if let Some(agent_uid) = removed_agent_uid.as_deref() {
        if ctx.notes_state.remove_note_text_by_agent_uid(agent_uid) {
            mark_terminal_notes_dirty(ctx.notes_state, Some(time));
        }
        let _ = ctx.aegis_policy.remove(agent_uid);
    } else if removed_tasks {
        mark_terminal_notes_dirty(ctx.notes_state, Some(time));
    }
    let _ = ctx.aegis_runtime.clear(selected_agent);
    ctx.view_state.forget_terminal(terminal_id);
    ctx.app_session.composer.unbind_agent(selected_agent);
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
    if let Some(replacement_agent) = replacement_agent {
        focus_agent_without_persist(
            replacement_agent,
            focus_ctx.session.visibility_mode(),
            &mut focus_ctx,
        );
    } else {
        clear_focus_without_persist(VisibilityMode::ShowAll, &mut focus_ctx);
    }
    Ok(Some(selected_agent))
}
