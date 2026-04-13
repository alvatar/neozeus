use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::{load_persisted_app_state_from, AppStatePersistenceState},
    conversations::{AgentTaskStore, ConversationPersistenceState, ConversationStore},
    hud::{HudInputCaptureState, TerminalVisibilityState},
    startup::rehydrate_restored_projection_state,
    terminals::{
        append_debug_log, load_terminal_notes_from, ActiveTerminalContentState,
        OwnedTmuxSessionStore, TerminalFocusState, TerminalManager, TerminalNotesState,
        TerminalPresentationStore, TerminalRuntimeSpawner, TerminalViewState,
    },
};
use bevy::{prelude::Time, window::RequestRedraw};

use super::{clear_composer_and_direct_input, project_focus_intent, restore_app};

pub(crate) struct ResetRuntimeContext<'a, 'w> {
    pub(crate) agent_catalog: &'a mut AgentCatalog,
    pub(crate) runtime_index: &'a mut AgentRuntimeIndex,
    pub(crate) app_session: &'a mut crate::app::AppSessionState,
    pub(crate) selection: &'a mut crate::hud::AgentListSelection,
    pub(crate) terminal_manager: &'a mut TerminalManager,
    pub(crate) focus_state: &'a mut TerminalFocusState,
    pub(crate) owned_tmux_sessions: &'a mut OwnedTmuxSessionStore,
    pub(crate) active_terminal_content: &'a mut ActiveTerminalContentState,
    pub(crate) runtime_spawner: &'a TerminalRuntimeSpawner,
    pub(crate) input_capture: &'a mut HudInputCaptureState,
    pub(crate) app_state_persistence: &'a mut AppStatePersistenceState,
    pub(crate) notes_state: &'a mut TerminalNotesState,
    pub(crate) aegis_policy: &'a mut crate::aegis::AegisPolicyStore,
    pub(crate) aegis_runtime: &'a mut crate::aegis::AegisRuntimeStore,
    pub(crate) visibility_state: &'a mut TerminalVisibilityState,
    pub(crate) view_state: &'a mut TerminalViewState,
    pub(crate) presentation_store: Option<&'a mut TerminalPresentationStore>,
    pub(crate) conversations: &'a mut ConversationStore,
    pub(crate) conversation_persistence: &'a mut ConversationPersistenceState,
    pub(crate) tasks: &'a mut AgentTaskStore,
    pub(crate) time: &'a Time,
    pub(crate) redraws: &'a mut bevy::prelude::MessageWriter<'w, RequestRedraw>,
}

pub(crate) fn reset_runtime_from_snapshot(ctx: &mut ResetRuntimeContext<'_, '_>) {
    let agent_catalog = &mut *ctx.agent_catalog;
    let runtime_index = &mut *ctx.runtime_index;
    let app_session = &mut *ctx.app_session;
    let selection = &mut *ctx.selection;
    let terminal_manager = &mut *ctx.terminal_manager;
    let focus_state = &mut *ctx.focus_state;
    let owned_tmux_sessions = &mut *ctx.owned_tmux_sessions;
    let active_terminal_content = &mut *ctx.active_terminal_content;
    let runtime_spawner = ctx.runtime_spawner;
    let input_capture = &mut *ctx.input_capture;
    let app_state_persistence = &mut *ctx.app_state_persistence;
    let notes_state = &mut *ctx.notes_state;
    let aegis_policy = &mut *ctx.aegis_policy;
    let aegis_runtime = &mut *ctx.aegis_runtime;
    let visibility_state = &mut *ctx.visibility_state;
    let view_state = &mut *ctx.view_state;
    let mut presentation_store = ctx.presentation_store.take();
    let conversations = &mut *ctx.conversations;
    let conversation_persistence = &mut *ctx.conversation_persistence;
    let tasks = &mut *ctx.tasks;
    let time = ctx.time;
    let redraws = &mut *ctx.redraws;
    app_session.reset_dialog.close();
    app_session.recovery_status.show_reset_confirmed();
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
    *conversations = ConversationStore::default();
    conversation_persistence.clear_runtime_state();
    *tasks = AgentTaskStore::default();
    notes_state.clear_runtime_state();
    *aegis_policy = crate::aegis::AegisPolicyStore::default();
    *aegis_runtime = crate::aegis::AegisRuntimeStore::default();
    app_session
        .focus_intent
        .clear(crate::app::VisibilityMode::ShowAll);
    let mut focus_projection = super::FocusProjectionContext {
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
    };
    project_focus_intent(app_session, &mut focus_projection);
    if let Some(presentation_store) = presentation_store.as_deref_mut() {
        *presentation_store = TerminalPresentationStore::default();
    }

    let mut status_details = vec![
        "Reset confirmed".to_owned(),
        "Runtime clear started".to_owned(),
        "Runtime clear completed".to_owned(),
    ];
    let should_rebuild = app_state_persistence
        .path
        .as_ref()
        .map(|path| load_persisted_app_state_from(path))
        .is_some_and(|persisted| !persisted.agents.is_empty());
    if should_rebuild {
        status_details.push("Automatic recovery started from saved snapshot".into());
        let mut restore_ctx = super::RestoreAppContext {
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
            visibility_state,
            view_state,
            presentation_store,
            time,
            redraws,
        };
        let summary = restore_app(&mut restore_ctx);
        if let Some(path) = notes_state.path.as_ref() {
            let notes = load_terminal_notes_from(path);
            notes_state.load(notes);
        }
        rehydrate_restored_projection_state(
            agent_catalog,
            runtime_index,
            notes_state,
            Some(tasks),
            conversation_persistence,
            conversations,
            time,
        );
        let status = crate::app::render_recovery_status_summary(
            "Reset recovery completed",
            &summary,
            status_details,
        );
        app_session
            .recovery_status
            .show(status.tone, status.title, status.details);
    } else {
        status_details.push("No saved snapshot to restore".into());
        app_session.recovery_status.show(
            crate::app::RecoveryStatusTone::Success,
            "Reset completed: runtime cleared; no saved snapshot to restore",
            status_details,
        );
    }
}
