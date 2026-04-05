use crate::{
    agents::{AgentCatalog, AgentKind, AgentRuntimeIndex},
    app::{
        load_persisted_app_state_from, mark_app_state_dirty, ordered_reconciled_persisted_agents,
        reconcile_persisted_agents, AppStatePersistenceState,
    },
    startup::{
        choose_startup_focus_session_name, startup_visibility_policy_for_focus, StartupLoadingState,
    },
    terminals::{
        append_debug_log, DaemonSessionInfo, TerminalFocusState, TerminalLifecycle,
        TerminalManager, TerminalRuntimeSpawner, TerminalViewState, PERSISTENT_SESSION_PREFIX,
    },
};

use super::super::session::{AppSessionState, VisibilityMode};
use super::{attach_restored_terminal, spawn_agent_terminal};
use bevy::prelude::*;

/// Handles startup focus candidate is interactive.
fn startup_focus_candidate_is_interactive(session: &DaemonSessionInfo) -> bool {
    matches!(session.runtime.lifecycle, TerminalLifecycle::Running)
}

#[allow(
    clippy::too_many_arguments,
    reason = "restore spans persistence, daemon discovery, agent state, and presentation state"
)]
/// Restores app.
pub(crate) fn restore_app(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
    selection: &mut crate::hud::AgentListSelection,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    input_capture: &mut crate::hud::HudInputCaptureState,
    app_state_persistence: &mut AppStatePersistenceState,
    visibility_state: &mut crate::hud::TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    startup_loading: Option<&mut StartupLoadingState>,
    time: &Time,
    redraws: &mut MessageWriter<bevy::window::RequestRedraw>,
) {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let mut startup_loading = startup_loading;
    let persisted = app_state_persistence
        .path
        .as_ref()
        .map(load_persisted_app_state_from)
        .unwrap_or_default();
    let live_session_infos = match runtime_spawner.list_session_infos() {
        Ok(sessions) => sessions,
        Err(error) => {
            append_debug_log(format!("daemon session discovery failed: {error}"));
            let startup_loading_slot = startup_loading.as_deref_mut();
            let _ = spawn_agent_terminal(
                agent_catalog,
                runtime_index,
                app_session,
                selection,
                terminal_manager,
                focus_state,
                runtime_spawner,
                input_capture,
                app_state_persistence,
                visibility_state,
                view_state,
                startup_loading_slot,
                time,
                PERSISTENT_SESSION_PREFIX,
                AgentKind::Pi,
                None,
                None,
                redraws,
            );
            return;
        }
    };
    let live_sessions = live_session_infos
        .iter()
        .map(|session| session.session_id.clone())
        .collect::<Vec<_>>();
    let (restore, import, prune) = reconcile_persisted_agents(&persisted, &live_sessions);
    let persisted_missing_agent_uid = persisted
        .agents
        .iter()
        .any(|record| record.agent_uid.is_none());
    let live_session_lookup = live_session_infos
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect::<std::collections::HashMap<_, _>>();
    let mut importable = Vec::new();
    for record in import {
        let keep = live_session_lookup
            .get(record.session_name.as_str())
            .is_some_and(|session| startup_focus_candidate_is_interactive(session));
        if keep {
            importable.push(record);
            continue;
        }
        match runtime_spawner.kill_session(&record.session_name) {
            Ok(()) => append_debug_log(format!(
                "startup reaped disconnected unpersisted session {}",
                record.session_name
            )),
            Err(error) => append_debug_log(format!(
                "startup skipped disconnected unpersisted session {} after reap failed: {error}",
                record.session_name
            )),
        }
    }
    if persisted_missing_agent_uid || !prune.is_empty() || !importable.is_empty() {
        mark_app_state_dirty(app_state_persistence, None);
    }

    for record in ordered_reconciled_persisted_agents(&restore, &importable) {
        let startup_loading_slot = startup_loading.as_deref_mut();
        if let Err(error) = attach_restored_terminal(
            agent_catalog,
            runtime_index,
            app_session,
            terminal_manager,
            focus_state,
            runtime_spawner,
            startup_loading_slot,
            record.session_name.clone(),
            false,
            match record.kind {
                crate::shared::app_state_file::PersistedAgentKind::Pi => AgentKind::Pi,
                crate::shared::app_state_file::PersistedAgentKind::Claude => AgentKind::Claude,
                crate::shared::app_state_file::PersistedAgentKind::Codex => AgentKind::Codex,
                crate::shared::app_state_file::PersistedAgentKind::Terminal => AgentKind::Terminal,
                crate::shared::app_state_file::PersistedAgentKind::Verifier => AgentKind::Verifier,
            },
            record.label,
            record.agent_uid,
            record.clone_source_session_path,
            record.is_workdir,
        ) {
            append_debug_log(format!(
                "startup attach failed for {}: {error}",
                record.session_name
            ));
        }
    }

    let restored_focus_session = restore
        .iter()
        .find(|record| {
            record.last_focused
                && live_session_lookup
                    .get(record.session_name.as_str())
                    .is_some_and(|session| startup_focus_candidate_is_interactive(session))
        })
        .map(|record| record.session_name.as_str());
    let restored_session_names = restore
        .iter()
        .filter(|record| {
            live_session_lookup
                .get(record.session_name.as_str())
                .is_some_and(|session| startup_focus_candidate_is_interactive(session))
        })
        .map(|record| record.session_name.as_str())
        .collect::<Vec<_>>();
    let imported_session_names = importable
        .iter()
        .map(|record| record.session_name.as_str())
        .collect::<Vec<_>>();

    if let Some(session_name) = choose_startup_focus_session_name(
        restored_focus_session,
        &restored_session_names,
        &imported_session_names,
    ) {
        if let Some(agent_id) = runtime_index.agent_for_session(session_name) {
            *selection = crate::hud::AgentListSelection::Agent(agent_id);
            app_session.visibility_mode = VisibilityMode::FocusedOnly;
            if let Some(terminal_id) = runtime_index.primary_terminal(agent_id) {
                focus_state.focus_terminal(terminal_manager, terminal_id);
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                visibility_state.policy = startup_visibility_policy_for_focus(Some(terminal_id));
                view_state.focus_terminal(Some(terminal_id));
            }
        }
    } else if !agent_catalog.order.is_empty() {
        *selection = crate::hud::AgentListSelection::None;
        app_session.visibility_mode = VisibilityMode::ShowAll;
        focus_state.clear_active_terminal();
        #[cfg(test)]
        terminal_manager.replace_test_focus_state(focus_state);
        visibility_state.policy = crate::hud::TerminalVisibilityPolicy::ShowAll;
        view_state.focus_terminal(None);
    } else {
        let _ = spawn_agent_terminal(
            agent_catalog,
            runtime_index,
            app_session,
            selection,
            terminal_manager,
            focus_state,
            runtime_spawner,
            input_capture,
            app_state_persistence,
            visibility_state,
            view_state,
            startup_loading,
            time,
            PERSISTENT_SESSION_PREFIX,
            AgentKind::Pi,
            None,
            None,
            redraws,
        );
    }
}
