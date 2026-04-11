use crate::{
    aegis::{AegisPolicyStore, AegisRuntimeStore},
    agents::{AgentCatalog, AgentKind, AgentRuntimeIndex},
    app::{
        load_persisted_app_state_from, mark_app_state_dirty, ordered_reconciled_persisted_agents,
        reconcile_persisted_agents, AppStatePersistenceState,
    },
    startup::choose_startup_focus_session_name,
    terminals::{
        append_debug_log, ActiveTerminalContentState, DaemonSessionInfo, OwnedTmuxSessionStore,
        TerminalFocusState, TerminalLifecycle, TerminalManager, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalViewState, PERSISTENT_SESSION_PREFIX,
    },
};

use super::super::session::{AppSessionState, VisibilityMode};
use super::{
    attach_restored_terminal, clear_focus_without_persist, focus_agent_without_persist,
    launch_spec_for_recovery_spec, respawn_recovered_agent_with_launch_spec, spawn_agent_terminal,
};
use bevy::prelude::*;

fn agent_kind_from_daemon_session(session: &DaemonSessionInfo) -> AgentKind {
    match session.metadata.agent_kind {
        Some(crate::shared::daemon_wire::DaemonAgentKind::Pi) => AgentKind::Pi,
        Some(crate::shared::daemon_wire::DaemonAgentKind::Claude) => AgentKind::Claude,
        Some(crate::shared::daemon_wire::DaemonAgentKind::Codex) => AgentKind::Codex,
        Some(crate::shared::daemon_wire::DaemonAgentKind::Terminal) => AgentKind::Terminal,
        Some(crate::shared::daemon_wire::DaemonAgentKind::Verifier) => AgentKind::Verifier,
        None if session
            .session_id
            .starts_with(crate::terminals::VERIFIER_SESSION_PREFIX) =>
        {
            AgentKind::Verifier
        }
        None => AgentKind::Terminal,
    }
}

/// Handles startup focus candidate is interactive.
fn startup_focus_candidate_is_interactive(session: &DaemonSessionInfo) -> bool {
    matches!(session.runtime.lifecycle, TerminalLifecycle::Running)
}

fn persisted_recovery_to_agent_recovery(
    recovery: crate::shared::app_state_file::PersistedAgentRecoverySpec,
) -> Option<crate::agents::AgentRecoverySpec> {
    match recovery {
        crate::shared::app_state_file::PersistedAgentRecoverySpec::Pi {
            session_path,
            cwd,
            is_workdir,
            workdir_slug,
        } => Some(crate::agents::AgentRecoverySpec::Pi {
            cwd: cwd
                .or_else(|| {
                    crate::shared::pi_session_files::read_session_header(&session_path)
                        .ok()
                        .map(|header| header.cwd)
                })
                .unwrap_or_default(),
            session_path,
            is_workdir,
            workdir_slug,
        }),
        crate::shared::app_state_file::PersistedAgentRecoverySpec::Claude {
            session_id,
            cwd,
            model,
            profile,
        } => Some(crate::agents::AgentRecoverySpec::Claude {
            session_id,
            cwd,
            model,
            profile,
        }),
        crate::shared::app_state_file::PersistedAgentRecoverySpec::Codex {
            session_id,
            cwd,
            model,
            profile,
        } => Some(crate::agents::AgentRecoverySpec::Codex {
            session_id,
            cwd,
            model,
            profile,
        }),
    }
}

fn clone_provenance_from_recovery(
    recovery: &Option<crate::agents::AgentRecoverySpec>,
) -> Option<String> {
    match recovery {
        Some(crate::agents::AgentRecoverySpec::Pi { session_path, .. }) => {
            Some(session_path.clone())
        }
        Some(crate::agents::AgentRecoverySpec::Claude { .. })
        | Some(crate::agents::AgentRecoverySpec::Codex { .. })
        | None => None,
    }
}

fn validate_recovery_spec(recovery: &crate::agents::AgentRecoverySpec) -> Result<(), String> {
    match recovery {
        crate::agents::AgentRecoverySpec::Pi { session_path, .. } => {
            if !std::path::Path::new(session_path).exists() {
                return Err(format!("Pi session path missing: {session_path}"));
            }
            Ok(())
        }
        crate::agents::AgentRecoverySpec::Claude {
            session_id, cwd, ..
        } => {
            if session_id.trim().is_empty() {
                return Err("Claude session id missing".into());
            }
            if cwd.trim().is_empty() {
                return Err("Claude cwd missing".into());
            }
            Ok(())
        }
        crate::agents::AgentRecoverySpec::Codex {
            session_id, cwd, ..
        } => {
            if session_id.trim().is_empty() {
                return Err("Codex session id missing".into());
            }
            if cwd.trim().is_empty() {
                return Err("Codex cwd missing".into());
            }
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RecoveryExecutionSummary {
    pub(crate) snapshot_found: bool,
    pub(crate) restored_agents: usize,
    pub(crate) failed_agents: Vec<String>,
    pub(crate) skipped_agents: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecoveryStatusPresentation {
    pub(crate) tone: crate::app::RecoveryStatusTone,
    pub(crate) title: String,
    pub(crate) details: Vec<String>,
}

pub(crate) fn render_recovery_status_summary(
    title_prefix: &str,
    summary: &RecoveryExecutionSummary,
    mut details: Vec<String>,
) -> RecoveryStatusPresentation {
    let title = if summary.skipped_agents.is_empty() {
        format!(
            "{title_prefix}: {} restored, {} failed",
            summary.restored_agents,
            summary.failed_agents.len()
        )
    } else {
        format!(
            "{title_prefix}: {} restored, {} failed, {} skipped",
            summary.restored_agents,
            summary.failed_agents.len(),
            summary.skipped_agents.len()
        )
    };
    details.extend(summary.failed_agents.iter().cloned());
    details.extend(summary.skipped_agents.iter().cloned());
    RecoveryStatusPresentation {
        tone: if summary.failed_agents.is_empty() {
            crate::app::RecoveryStatusTone::Success
        } else {
            crate::app::RecoveryStatusTone::Error
        },
        title,
        details,
    }
}

fn skipped_live_only_restore_message(
    record: &crate::shared::app_state_file::PersistedAgentState,
) -> String {
    format!(
        "startup skipped live-only agent {}: runtime session unavailable",
        record.label.as_deref().unwrap_or("<unlabeled-agent>")
    )
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
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    active_terminal_content: &mut ActiveTerminalContentState,
    runtime_spawner: &TerminalRuntimeSpawner,
    input_capture: &mut crate::hud::HudInputCaptureState,
    app_state_persistence: &mut AppStatePersistenceState,
    aegis_policy: &mut AegisPolicyStore,
    _aegis_runtime: &mut AegisRuntimeStore,
    visibility_state: &mut crate::hud::TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    presentation_store: Option<&mut TerminalPresentationStore>,
    time: &Time,
    redraws: &mut MessageWriter<bevy::window::RequestRedraw>,
) -> RecoveryExecutionSummary {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let mut presentation_store = presentation_store;
    let persisted = app_state_persistence
        .path
        .as_ref()
        .map(load_persisted_app_state_from)
        .unwrap_or_default();
    let mut summary = RecoveryExecutionSummary {
        snapshot_found: !persisted.agents.is_empty(),
        ..RecoveryExecutionSummary::default()
    };
    let live_session_infos = match runtime_spawner.list_session_infos() {
        Ok(sessions) => sessions,
        Err(error) => {
            let message = format!("daemon session discovery failed: {error}");
            append_debug_log(message.clone());
            if summary.snapshot_found {
                summary.failed_agents.push(message);
            }
            return summary;
        }
    };
    let (restore, prune, import_session_names) =
        reconcile_persisted_agents(&persisted, &live_session_infos);
    let persisted_missing_agent_uid = persisted
        .agents
        .iter()
        .any(|record| record.agent_uid.is_none());
    let live_session_lookup = live_session_infos
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect::<std::collections::HashMap<_, _>>();
    let mut importable = Vec::new();
    let mut next_import_order = persisted
        .agents
        .iter()
        .map(|record| record.order_index)
        .max()
        .map(|max| max + 1)
        .unwrap_or(0);
    for session_name in import_session_names {
        let Some(session) = live_session_lookup.get(session_name.as_str()) else {
            continue;
        };
        let keep = startup_focus_candidate_is_interactive(session);
        if keep {
            importable.push(crate::shared::app_state_file::PersistedAgentState {
                agent_uid: session.metadata.agent_uid.clone(),
                runtime_session_name: Some(session_name),
                label: session.metadata.agent_label.clone(),
                kind: match agent_kind_from_daemon_session(session) {
                    AgentKind::Pi => crate::shared::app_state_file::PersistedAgentKind::Pi,
                    AgentKind::Claude => crate::shared::app_state_file::PersistedAgentKind::Claude,
                    AgentKind::Codex => crate::shared::app_state_file::PersistedAgentKind::Codex,
                    AgentKind::Terminal => {
                        crate::shared::app_state_file::PersistedAgentKind::Terminal
                    }
                    AgentKind::Verifier => {
                        crate::shared::app_state_file::PersistedAgentKind::Verifier
                    }
                },
                recovery: None,
                clone_source_session_path: None,
                aegis_enabled: false,
                aegis_prompt_text: None,
                order_index: next_import_order,
                last_focused: false,
            });
            next_import_order += 1;
            continue;
        }
        match runtime_spawner.kill_session(&session_name) {
            Ok(()) => append_debug_log(format!(
                "startup reaped disconnected unpersisted session {}",
                session_name
            )),
            Err(error) => append_debug_log(format!(
                "startup skipped disconnected unpersisted session {} after reap failed: {error}",
                session_name
            )),
        }
    }
    if persisted_missing_agent_uid || !prune.is_empty() || !importable.is_empty() {
        mark_app_state_dirty(app_state_persistence, None);
    }

    let mut respawned_focus_agent = None;
    for record in ordered_reconciled_persisted_agents(&restore, &importable) {
        let Some(runtime_session_name) = record.runtime_session_name.clone() else {
            append_debug_log("startup attach skipped for record missing runtime session name");
            continue;
        };
        let presentation_store_slot = presentation_store.as_deref_mut();
        let should_mark_startup_pending = live_session_lookup
            .get(runtime_session_name.as_str())
            .is_some_and(|session| startup_focus_candidate_is_interactive(session));
        let recovery = record
            .recovery
            .and_then(persisted_recovery_to_agent_recovery);
        let clone_source_session_path = record
            .clone_source_session_path
            .clone()
            .or_else(|| clone_provenance_from_recovery(&recovery));
        match attach_restored_terminal(
            agent_catalog,
            runtime_index,
            app_session,
            terminal_manager,
            focus_state,
            runtime_spawner,
            presentation_store_slot,
            runtime_session_name,
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
            clone_source_session_path,
            recovery,
        ) {
            Ok((agent_id, terminal_id)) => {
                summary.restored_agents += 1;
                if record.aegis_enabled || record.aegis_prompt_text.is_some() {
                    if let Some(agent_uid) = agent_catalog.uid(agent_id) {
                        let prompt_text = record
                            .aegis_prompt_text
                            .clone()
                            .unwrap_or_else(|| crate::aegis::DEFAULT_AEGIS_PROMPT.to_owned());
                        let _ = aegis_policy.restore_policy(
                            agent_uid,
                            record.aegis_enabled,
                            prompt_text,
                        );
                    }
                }
                if should_mark_startup_pending {
                    if let Some(presentation_store) = presentation_store.as_deref_mut() {
                        presentation_store.mark_startup_pending(terminal_id);
                    }
                }
            }
            Err(error) => {
                let message = format!(
                    "startup attach failed for {}: {error}",
                    record
                        .runtime_session_name
                        .as_deref()
                        .unwrap_or("<missing-session>")
                );
                append_debug_log(message.clone());
                summary.failed_agents.push(message);
            }
        }
    }

    for record in prune
        .iter()
        .filter(|record| record.durability() == crate::agents::AgentDurability::LiveOnly)
    {
        let message = skipped_live_only_restore_message(record);
        append_debug_log(message.clone());
        summary.skipped_agents.push(message);
    }

    let respawnable = prune
        .iter()
        .filter(|record| record.durability() == crate::agents::AgentDurability::Recoverable)
        .cloned()
        .collect::<Vec<_>>();
    for record in respawnable {
        let Some(agent_uid) = record.agent_uid.clone() else {
            let message =
                "startup respawn skipped for recoverable record missing agent uid".to_owned();
            append_debug_log(message.clone());
            summary.failed_agents.push(message);
            continue;
        };
        let Some(recovery) = record
            .recovery
            .clone()
            .and_then(persisted_recovery_to_agent_recovery)
        else {
            let message = format!(
                "startup respawn skipped for {} missing valid recovery spec",
                record.label.as_deref().unwrap_or("<unlabeled-agent>")
            );
            append_debug_log(message.clone());
            summary.failed_agents.push(message);
            continue;
        };
        if let Err(error) = validate_recovery_spec(&recovery) {
            let message = format!(
                "startup respawn skipped for {}: {error}",
                record.label.as_deref().unwrap_or("<unlabeled-agent>")
            );
            append_debug_log(message.clone());
            summary.failed_agents.push(message);
            continue;
        }
        let mut launch = launch_spec_for_recovery_spec(&recovery);
        if record.clone_source_session_path.is_some() {
            launch.metadata.clone_source_session_path = record.clone_source_session_path.clone();
        }
        let working_directory = match &recovery {
            crate::agents::AgentRecoverySpec::Pi { cwd, .. }
            | crate::agents::AgentRecoverySpec::Claude { cwd, .. }
            | crate::agents::AgentRecoverySpec::Codex { cwd, .. } => Some(cwd.as_str()),
        };
        match respawn_recovered_agent_with_launch_spec(
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
            visibility_state,
            view_state,
            presentation_store.as_deref_mut(),
            time,
            PERSISTENT_SESSION_PREFIX,
            match record.kind {
                crate::shared::app_state_file::PersistedAgentKind::Pi => AgentKind::Pi,
                crate::shared::app_state_file::PersistedAgentKind::Claude => AgentKind::Claude,
                crate::shared::app_state_file::PersistedAgentKind::Codex => AgentKind::Codex,
                crate::shared::app_state_file::PersistedAgentKind::Terminal => AgentKind::Terminal,
                crate::shared::app_state_file::PersistedAgentKind::Verifier => AgentKind::Verifier,
            },
            agent_uid.clone(),
            record.label.clone(),
            working_directory,
            launch,
            redraws,
        ) {
            Ok(agent_id) => {
                summary.restored_agents += 1;
                if record.aegis_enabled || record.aegis_prompt_text.is_some() {
                    if let Some(agent_uid) = agent_catalog.uid(agent_id) {
                        let prompt_text = record
                            .aegis_prompt_text
                            .clone()
                            .unwrap_or_else(|| crate::aegis::DEFAULT_AEGIS_PROMPT.to_owned());
                        let _ = aegis_policy.restore_policy(
                            agent_uid,
                            record.aegis_enabled,
                            prompt_text,
                        );
                    }
                }
                if record.last_focused {
                    respawned_focus_agent = Some(agent_id);
                }
            }
            Err(error) => {
                let message = format!(
                    "startup respawn failed for {}: {error}",
                    record.label.as_deref().unwrap_or("<unlabeled-agent>")
                );
                append_debug_log(message.clone());
                summary.failed_agents.push(message);
            }
        }
    }

    let restored_focus_session = restore
        .iter()
        .find(|record| {
            record.last_focused
                && record
                    .runtime_session_name
                    .as_deref()
                    .and_then(|session_name| live_session_lookup.get(session_name))
                    .is_some_and(|session| startup_focus_candidate_is_interactive(session))
        })
        .and_then(|record| record.runtime_session_name.as_deref());
    let restored_session_names = restore
        .iter()
        .filter(|record| {
            record
                .runtime_session_name
                .as_deref()
                .and_then(|session_name| live_session_lookup.get(session_name))
                .is_some_and(|session| startup_focus_candidate_is_interactive(session))
        })
        .filter_map(|record| record.runtime_session_name.as_deref())
        .collect::<Vec<_>>();
    let imported_session_names = importable
        .iter()
        .filter_map(|record| record.runtime_session_name.as_deref())
        .collect::<Vec<_>>();

    if let Some(session_name) = choose_startup_focus_session_name(
        restored_focus_session,
        &restored_session_names,
        &imported_session_names,
    ) {
        if let Some(agent_id) = runtime_index.agent_for_session(session_name) {
            focus_agent_without_persist(
                agent_id,
                VisibilityMode::FocusedOnly,
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
                redraws,
            );
        }
    } else if let Some(agent_id) = respawned_focus_agent {
        focus_agent_without_persist(
            agent_id,
            VisibilityMode::FocusedOnly,
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
            redraws,
        );
    } else if !agent_catalog.order.is_empty() {
        clear_focus_without_persist(
            VisibilityMode::ShowAll,
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
            redraws,
        );
    } else if !summary.snapshot_found {
        let _ = spawn_agent_terminal(
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
            visibility_state,
            view_state,
            presentation_store,
            time,
            PERSISTENT_SESSION_PREFIX,
            AgentKind::Pi,
            None,
            None,
            redraws,
        );
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::{
        agent_kind_from_daemon_session, render_recovery_status_summary,
        skipped_live_only_restore_message, RecoveryExecutionSummary,
    };
    use crate::{
        agents::AgentKind,
        shared::{
            app_state_file::{PersistedAgentKind, PersistedAgentState},
            daemon_wire::{DaemonAgentKind, DaemonSessionMetadata},
        },
        terminals::{DaemonSessionInfo, TerminalLifecycle, TerminalRuntimeState},
    };

    fn session_with_kind(agent_kind: Option<DaemonAgentKind>) -> DaemonSessionInfo {
        DaemonSessionInfo {
            session_id: "neozeus-session-1".into(),
            runtime: TerminalRuntimeState {
                status: "running".into(),
                lifecycle: TerminalLifecycle::Running,
                last_error: None,
            },
            revision: 0,
            created_order: 0,
            metadata: DaemonSessionMetadata {
                agent_uid: None,
                agent_label: None,
                agent_kind,
            },
        }
    }

    #[test]
    fn imported_live_session_preserves_claude_kind() {
        assert_eq!(
            agent_kind_from_daemon_session(&session_with_kind(Some(DaemonAgentKind::Claude))),
            AgentKind::Claude
        );
    }

    #[test]
    fn imported_live_session_preserves_codex_kind() {
        assert_eq!(
            agent_kind_from_daemon_session(&session_with_kind(Some(DaemonAgentKind::Codex))),
            AgentKind::Codex
        );
    }

    #[test]
    fn imported_live_session_preserves_terminal_kind() {
        assert_eq!(
            agent_kind_from_daemon_session(&session_with_kind(Some(DaemonAgentKind::Terminal))),
            AgentKind::Terminal
        );
    }

    #[test]
    fn imported_live_session_without_kind_falls_back_to_terminal() {
        assert_eq!(
            agent_kind_from_daemon_session(&session_with_kind(None)),
            AgentKind::Terminal
        );
    }

    #[test]
    fn render_recovery_status_summary_keeps_reset_and_startup_counting_semantics_aligned() {
        let summary = RecoveryExecutionSummary {
            snapshot_found: true,
            restored_agents: 2,
            failed_agents: vec!["failed-a".into()],
            skipped_agents: vec!["skipped-b".into()],
        };

        let startup = render_recovery_status_summary(
            "Automatic recovery completed",
            &summary,
            vec!["Automatic recovery started from saved snapshot".into()],
        );
        let reset = render_recovery_status_summary(
            "Reset recovery completed",
            &summary,
            vec!["Reset confirmed".into()],
        );

        assert_eq!(
            startup.title,
            "Automatic recovery completed: 2 restored, 1 failed, 1 skipped"
        );
        assert_eq!(
            reset.title,
            "Reset recovery completed: 2 restored, 1 failed, 1 skipped"
        );
        assert_eq!(startup.tone, crate::app::RecoveryStatusTone::Error);
        assert_eq!(reset.tone, crate::app::RecoveryStatusTone::Error);
        assert_eq!(startup.details[1..], ["failed-a", "skipped-b"]);
        assert_eq!(reset.details[1..], ["failed-a", "skipped-b"]);
    }

    #[test]
    fn persisted_agent_durability_is_live_only_without_recovery_and_recoverable_with_recovery() {
        let live_only = PersistedAgentState {
            agent_uid: Some("agent-live".into()),
            runtime_session_name: Some("neozeus-session-live".into()),
            label: Some("LIVE".into()),
            kind: PersistedAgentKind::Terminal,
            recovery: None,
            clone_source_session_path: None,
            aegis_enabled: false,
            aegis_prompt_text: None,
            order_index: 0,
            last_focused: false,
        };
        let recoverable = PersistedAgentState {
            agent_uid: Some("agent-pi".into()),
            runtime_session_name: None,
            label: Some("PI".into()),
            kind: PersistedAgentKind::Pi,
            recovery: Some(
                crate::shared::app_state_file::PersistedAgentRecoverySpec::Pi {
                    session_path: "/tmp/pi-session.jsonl".into(),
                    cwd: Some("/tmp/demo".into()),
                    is_workdir: false,
                    workdir_slug: None,
                },
            ),
            clone_source_session_path: Some("/tmp/pi-session.jsonl".into()),
            aegis_enabled: false,
            aegis_prompt_text: None,
            order_index: 1,
            last_focused: false,
        };

        assert_eq!(
            live_only.durability(),
            crate::agents::AgentDurability::LiveOnly
        );
        assert_eq!(
            recoverable.durability(),
            crate::agents::AgentDurability::Recoverable
        );
    }

    #[test]
    fn skipped_live_only_message_uses_agent_label_when_present() {
        let record = PersistedAgentState {
            agent_uid: Some("agent-uid-1".into()),
            runtime_session_name: Some("neozeus-session-a".into()),
            label: Some("ALPHA".into()),
            kind: PersistedAgentKind::Terminal,
            recovery: None,
            clone_source_session_path: None,
            aegis_enabled: false,
            aegis_prompt_text: None,
            order_index: 0,
            last_focused: false,
        };

        assert_eq!(
            skipped_live_only_restore_message(&record),
            "startup skipped live-only agent ALPHA: runtime session unavailable"
        );
    }
}
