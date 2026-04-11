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
        Some(kind) => AgentKind::from_daemon_kind(kind),
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

pub(crate) struct RestoreAppContext<'a, 'w> {
    pub(crate) agent_catalog: &'a mut AgentCatalog,
    pub(crate) runtime_index: &'a mut AgentRuntimeIndex,
    pub(crate) app_session: &'a mut AppSessionState,
    pub(crate) selection: &'a mut crate::hud::AgentListSelection,
    pub(crate) terminal_manager: &'a mut TerminalManager,
    pub(crate) focus_state: &'a mut TerminalFocusState,
    pub(crate) owned_tmux_sessions: &'a OwnedTmuxSessionStore,
    pub(crate) active_terminal_content: &'a mut ActiveTerminalContentState,
    pub(crate) runtime_spawner: &'a TerminalRuntimeSpawner,
    pub(crate) input_capture: &'a mut crate::hud::HudInputCaptureState,
    pub(crate) app_state_persistence: &'a mut AppStatePersistenceState,
    pub(crate) aegis_policy: &'a mut AegisPolicyStore,
    pub(crate) aegis_runtime: &'a mut AegisRuntimeStore,
    pub(crate) visibility_state: &'a mut crate::hud::TerminalVisibilityState,
    pub(crate) view_state: &'a mut TerminalViewState,
    pub(crate) presentation_store: Option<&'a mut TerminalPresentationStore>,
    pub(crate) time: &'a Time,
    pub(crate) redraws: &'a mut MessageWriter<'w, bevy::window::RequestRedraw>,
}

/// Restores app.
pub(crate) fn restore_app(ctx: &mut RestoreAppContext<'_, '_>) -> RecoveryExecutionSummary {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let agent_catalog = &mut *ctx.agent_catalog;
    let runtime_index = &mut *ctx.runtime_index;
    let app_session = &mut *ctx.app_session;
    let selection = &mut *ctx.selection;
    let terminal_manager = &mut *ctx.terminal_manager;
    let focus_state = &mut *ctx.focus_state;
    let owned_tmux_sessions = ctx.owned_tmux_sessions;
    let active_terminal_content = &mut *ctx.active_terminal_content;
    let runtime_spawner = ctx.runtime_spawner;
    let input_capture = &mut *ctx.input_capture;
    let app_state_persistence = &mut *ctx.app_state_persistence;
    let aegis_policy = &mut *ctx.aegis_policy;
    let _aegis_runtime = &mut *ctx.aegis_runtime;
    let visibility_state = &mut *ctx.visibility_state;
    let view_state = &mut *ctx.view_state;
    let mut presentation_store = ctx.presentation_store.take();
    let time = ctx.time;
    let redraws = &mut *ctx.redraws;
    let persisted = app_state_persistence
        .path
        .as_ref()
        .map(|path| load_persisted_app_state_from(path))
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
        // Daemon metadata contributes only the live-session identity mirror (uid/label/kind).
        // Recovery provenance, clone provenance, Aegis state, and conversations/tasks remain
        // app-owned and are reconstructed only from persisted app state.
        let keep = startup_focus_candidate_is_interactive(session);
        if keep {
            importable.push(crate::shared::app_state_file::PersistedAgentState {
                agent_uid: session.metadata.agent_uid.clone(),
                runtime_session_name: Some(session_name),
                label: session.metadata.agent_label.clone(),
                kind: agent_kind_from_daemon_session(session).persisted_kind(),
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
        // For restore/import attach, the daemon owns session existence/runtime and the app owns
        // stable uid/label/kind plus all recovery metadata; attach reuses the live session and then
        // re-mirrors the app-owned identity back into daemon metadata.
        let attach_result = {
            let mut attach_ctx = super::AttachRestoredTerminalContext {
                agent_catalog,
                runtime_index,
                terminal_manager,
                focus_state,
                runtime_spawner,
                presentation_store: presentation_store_slot,
            };
            attach_restored_terminal(
                &mut attach_ctx,
                super::AttachRestoredTerminalRequest {
                    session_name: runtime_session_name,
                    focus: false,
                    kind: AgentKind::from_persisted_kind(record.kind),
                    label: record.label,
                    agent_uid: record.agent_uid,
                    clone_source_session_path,
                    recovery,
                },
            )
        };
        match attach_result {
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
        let mut spawn_ctx = super::SpawnAgentContext {
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
            presentation_store: presentation_store.as_deref_mut(),
            time,
            redraws,
        };
        match respawn_recovered_agent_with_launch_spec(
            &mut spawn_ctx,
            PERSISTENT_SESSION_PREFIX,
            AgentKind::from_persisted_kind(record.kind),
            agent_uid.clone(),
            record.label.clone(),
            working_directory,
            launch,
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
            focus_agent_without_persist(agent_id, VisibilityMode::FocusedOnly, &mut focus_ctx);
        }
    } else if let Some(agent_id) = respawned_focus_agent {
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
        focus_agent_without_persist(agent_id, VisibilityMode::FocusedOnly, &mut focus_ctx);
    } else if !agent_catalog.order.is_empty() {
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
    } else if !summary.snapshot_found {
        let mut spawn_ctx = super::SpawnAgentContext {
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
            redraws,
        };
        let _ = spawn_agent_terminal(
            &mut spawn_ctx,
            PERSISTENT_SESSION_PREFIX,
            AgentKind::Pi,
            None,
            None,
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
