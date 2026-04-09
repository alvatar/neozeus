use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::{load_persisted_app_state_from, AppStatePersistenceState},
    conversations::{AgentTaskStore, ConversationPersistenceState, ConversationStore},
    hud::{HudInputCaptureState, TerminalVisibilityState},
    terminals::{
        append_debug_log, ActiveTerminalContentState, OwnedTmuxSessionStore, TerminalFocusState,
        TerminalManager, TerminalPresentationStore, TerminalRuntimeSpawner, TerminalViewState,
    },
};
use bevy::{prelude::Time, window::RequestRedraw};

use super::{clear_composer_and_direct_input, restore_app};

#[allow(
    clippy::too_many_arguments,
    reason = "reset spans daemon teardown, local runtime cleanup, and snapshot-driven rebuild"
)]
pub(crate) fn reset_runtime_from_snapshot(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut crate::app::AppSessionState,
    selection: &mut crate::hud::AgentListSelection,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    owned_tmux_sessions: &mut OwnedTmuxSessionStore,
    active_terminal_content: &mut ActiveTerminalContentState,
    runtime_spawner: &TerminalRuntimeSpawner,
    input_capture: &mut HudInputCaptureState,
    app_state_persistence: &mut AppStatePersistenceState,
    aegis_policy: &mut crate::aegis::AegisPolicyStore,
    aegis_runtime: &mut crate::aegis::AegisRuntimeStore,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    presentation_store: Option<&mut TerminalPresentationStore>,
    conversations: &mut ConversationStore,
    conversation_persistence: &mut ConversationPersistenceState,
    tasks: &mut AgentTaskStore,
    time: &Time,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    app_session.reset_dialog.close();
    clear_composer_and_direct_input(app_session, input_capture, redraws);
    app_session.create_agent_dialog.close();
    app_session.clone_agent_dialog.close();
    app_session.rename_agent_dialog.close();
    app_session.aegis_dialog.close();

    if let Ok(tmux_sessions) = runtime_spawner.list_owned_tmux_sessions() {
        for session in tmux_sessions {
            if let Err(error) = runtime_spawner.kill_owned_tmux_session(&session.session_uid) {
                append_debug_log(format!(
                    "reset failed to kill owned tmux {}: {error}",
                    session.session_uid
                ));
            }
        }
    }
    if let Ok(live_sessions) = runtime_spawner.list_session_infos() {
        for session in live_sessions {
            if let Err(error) = runtime_spawner.kill_session(&session.session_id) {
                append_debug_log(format!(
                    "reset failed to kill session {}: {error}",
                    session.session_id
                ));
            }
        }
    }

    *terminal_manager = TerminalManager::default();
    *focus_state = TerminalFocusState::default();
    *runtime_index = AgentRuntimeIndex::default();
    *agent_catalog = AgentCatalog::default();
    *owned_tmux_sessions = OwnedTmuxSessionStore::default();
    *active_terminal_content = ActiveTerminalContentState::default();
    *view_state = TerminalViewState::default();
    *visibility_state = TerminalVisibilityState::default();
    *selection = crate::hud::AgentListSelection::None;
    *conversations = ConversationStore::default();
    *conversation_persistence = ConversationPersistenceState::default();
    *tasks = AgentTaskStore::default();
    *aegis_policy = crate::aegis::AegisPolicyStore::default();
    *aegis_runtime = crate::aegis::AegisRuntimeStore::default();
    app_session
        .focus_intent
        .clear(crate::app::VisibilityMode::ShowAll);
    let mut presentation_store = presentation_store;
    if let Some(presentation_store) = presentation_store.as_deref_mut() {
        *presentation_store = TerminalPresentationStore::default();
    }

    let should_rebuild = app_state_persistence
        .path
        .as_ref()
        .map(load_persisted_app_state_from)
        .is_some_and(|persisted| !persisted.agents.is_empty());
    if should_rebuild {
        let summary = restore_app(
            agent_catalog,
            runtime_index,
            app_session,
            selection,
            terminal_manager,
            focus_state,
            owned_tmux_sessions,
            active_terminal_content,
            runtime_spawner,
            input_capture,
            app_state_persistence,
            aegis_policy,
            aegis_runtime,
            visibility_state,
            view_state,
            presentation_store,
            time,
            redraws,
        );
        let title = format!(
            "Reset recovery completed: {} restored, {} failed",
            summary.restored_agents,
            summary.failed_agents.len()
        );
        let tone = if summary.failed_agents.is_empty() {
            crate::app::RecoveryStatusTone::Success
        } else {
            crate::app::RecoveryStatusTone::Error
        };
        app_session
            .recovery_status
            .show(tone, title, summary.failed_agents);
    } else {
        app_session.recovery_status.show(
            crate::app::RecoveryStatusTone::Success,
            "Reset completed: runtime cleared; no saved snapshot to restore",
            Vec::new(),
        );
    }
}
