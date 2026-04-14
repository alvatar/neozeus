use crate::{
    aegis::AegisPolicyStore,
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

#[derive(Clone, Debug, Default)]
struct RestoreSnapshot {
    persisted: crate::shared::app_state_file::PersistedAppState,
    snapshot_found: bool,
    persisted_missing_agent_uid: bool,
    next_import_order: u64,
}

#[derive(Clone, Debug, Default)]
struct LiveSessionInventory {
    sessions: Vec<DaemonSessionInfo>,
    lookup: std::collections::HashMap<String, DaemonSessionInfo>,
}

#[derive(Clone, Debug, Default)]
struct RestorePlan {
    restore: Vec<crate::shared::app_state_file::PersistedAgentState>,
    prune: Vec<crate::shared::app_state_file::PersistedAgentState>,
    importable: Vec<crate::shared::app_state_file::PersistedAgentState>,
    reapable_session_names: Vec<String>,
    should_mark_app_state_dirty: bool,
}

#[derive(Clone, Debug)]
struct AttachIntent {
    record: crate::shared::app_state_file::PersistedAgentState,
    runtime_session_name: String,
    should_mark_startup_pending: bool,
    recovery: Option<crate::agents::AgentRecoverySpec>,
    clone_source_session_path: Option<String>,
}

#[derive(Clone, Debug)]
struct RespawnIntent {
    record: crate::shared::app_state_file::PersistedAgentState,
    agent_uid: String,
    launch: super::spawn_agent_terminal::AgentLaunchSpec,
    working_directory: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct RestoreFocusCandidates {
    restored_focus_session: Option<String>,
    restored_session_names: Vec<String>,
    imported_session_names: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct RestoreProgress {
    summary: RecoveryExecutionSummary,
    respawned_focus_agent: Option<crate::agents::AgentId>,
}

struct RestoreExecution<'a, 'w> {
    agent_catalog: &'a mut AgentCatalog,
    runtime_index: &'a mut AgentRuntimeIndex,
    app_session: &'a mut AppSessionState,
    selection: &'a mut crate::hud::AgentListSelection,
    terminal_manager: &'a mut TerminalManager,
    focus_state: &'a mut TerminalFocusState,
    owned_tmux_sessions: &'a OwnedTmuxSessionStore,
    active_terminal_content: &'a mut ActiveTerminalContentState,
    runtime_spawner: &'a TerminalRuntimeSpawner,
    input_capture: &'a mut crate::hud::HudInputCaptureState,
    app_state_persistence: &'a mut AppStatePersistenceState,
    aegis_policy: &'a mut AegisPolicyStore,
    visibility_state: &'a mut crate::hud::TerminalVisibilityState,
    view_state: &'a mut TerminalViewState,
    presentation_store: Option<&'a mut TerminalPresentationStore>,
    time: &'a Time,
    redraws: &'a mut MessageWriter<'w, bevy::window::RequestRedraw>,
}

fn load_restore_snapshot(app_state_persistence: &AppStatePersistenceState) -> RestoreSnapshot {
    let persisted = app_state_persistence
        .path
        .as_ref()
        .map(|path| load_persisted_app_state_from(path))
        .unwrap_or_default();
    RestoreSnapshot {
        snapshot_found: !persisted.agents.is_empty(),
        persisted_missing_agent_uid: persisted
            .agents
            .iter()
            .any(|record| record.agent_uid.is_none()),
        next_import_order: persisted
            .agents
            .iter()
            .map(|record| record.order_index)
            .max()
            .map(|max| max + 1)
            .unwrap_or(0),
        persisted,
    }
}

fn build_live_session_inventory(sessions: Vec<DaemonSessionInfo>) -> LiveSessionInventory {
    let lookup = sessions
        .iter()
        .cloned()
        .map(|session| (session.session_id.clone(), session))
        .collect();
    LiveSessionInventory { sessions, lookup }
}

fn discover_live_sessions(
    runtime_spawner: &TerminalRuntimeSpawner,
) -> Result<LiveSessionInventory, String> {
    runtime_spawner
        .list_session_infos()
        .map(build_live_session_inventory)
}

fn imported_live_session_record(
    session_name: String,
    session: &DaemonSessionInfo,
    order_index: u64,
) -> crate::shared::app_state_file::PersistedAgentState {
    crate::shared::app_state_file::PersistedAgentState {
        agent_uid: session.metadata.agent_uid.clone(),
        runtime_session_name: Some(session_name),
        label: session.metadata.agent_label.clone(),
        kind: agent_kind_from_daemon_session(session).persisted_kind(),
        recovery: None,
        clone_source_session_path: None,
        aegis_enabled: false,
        aegis_prompt_text: None,
        paused: false,
        order_index,
        last_focused: false,
    }
}

fn classify_importable_and_reapable_live_sessions(
    import_session_names: Vec<String>,
    inventory: &LiveSessionInventory,
    mut next_import_order: u64,
) -> (
    Vec<crate::shared::app_state_file::PersistedAgentState>,
    Vec<String>,
) {
    let mut importable = Vec::new();
    let mut reapable_session_names = Vec::new();
    for session_name in import_session_names {
        let Some(session) = inventory.lookup.get(&session_name) else {
            continue;
        };
        if startup_focus_candidate_is_interactive(session) {
            importable.push(imported_live_session_record(
                session_name,
                session,
                next_import_order,
            ));
            next_import_order += 1;
        } else {
            reapable_session_names.push(session_name);
        }
    }
    (importable, reapable_session_names)
}

fn plan_restore(snapshot: &RestoreSnapshot, inventory: &LiveSessionInventory) -> RestorePlan {
    let (restore, prune, import_session_names) =
        reconcile_persisted_agents(&snapshot.persisted, &inventory.sessions);
    let (importable, reapable_session_names) = classify_importable_and_reapable_live_sessions(
        import_session_names,
        inventory,
        snapshot.next_import_order,
    );
    RestorePlan {
        should_mark_app_state_dirty: snapshot.persisted_missing_agent_uid
            || !prune.is_empty()
            || !importable.is_empty(),
        restore,
        prune,
        importable,
        reapable_session_names,
    }
}

fn reap_unimportable_live_sessions(
    runtime_spawner: &TerminalRuntimeSpawner,
    session_names: &[String],
) {
    for session_name in session_names {
        match runtime_spawner.kill_session(session_name) {
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
}

fn restore_aegis_for_record(
    agent_catalog: &AgentCatalog,
    aegis_policy: &mut AegisPolicyStore,
    agent_id: crate::agents::AgentId,
    record: &crate::shared::app_state_file::PersistedAgentState,
) {
    if !record.aegis_enabled && record.aegis_prompt_text.is_none() {
        return;
    }
    if let Some(agent_uid) = agent_catalog.uid(agent_id) {
        let prompt_text = record
            .aegis_prompt_text
            .clone()
            .unwrap_or_else(|| crate::aegis::DEFAULT_AEGIS_PROMPT.to_owned());
        let _ = aegis_policy.restore_policy(agent_uid, record.aegis_enabled, prompt_text);
    }
}

fn attach_intent_from_record(
    record: crate::shared::app_state_file::PersistedAgentState,
    inventory: &LiveSessionInventory,
) -> Option<AttachIntent> {
    let runtime_session_name = record.runtime_session_name.clone()?;
    let should_mark_startup_pending = inventory
        .lookup
        .get(&runtime_session_name)
        .is_some_and(startup_focus_candidate_is_interactive);
    let recovery = record
        .recovery
        .clone()
        .and_then(persisted_recovery_to_agent_recovery);
    let clone_source_session_path = record
        .clone_source_session_path
        .clone()
        .or_else(|| clone_provenance_from_recovery(&recovery));
    Some(AttachIntent {
        record,
        runtime_session_name,
        should_mark_startup_pending,
        recovery,
        clone_source_session_path,
    })
}

fn build_attach_intents(plan: &RestorePlan, inventory: &LiveSessionInventory) -> Vec<AttachIntent> {
    ordered_reconciled_persisted_agents(&plan.restore, &plan.importable)
        .into_iter()
        .filter_map(|record| attach_intent_from_record(record, inventory))
        .collect()
}

fn attach_live_agents(
    exec: &mut RestoreExecution<'_, '_>,
    intents: &[AttachIntent],
    progress: &mut RestoreProgress,
) {
    for intent in intents {
        let attach_result = {
            let mut attach_ctx = super::AttachRestoredTerminalContext {
                agent_catalog: exec.agent_catalog,
                runtime_index: exec.runtime_index,
                terminal_manager: exec.terminal_manager,
                focus_state: exec.focus_state,
                runtime_spawner: exec.runtime_spawner,
                presentation_store: exec.presentation_store.as_deref_mut(),
            };
            attach_restored_terminal(
                &mut attach_ctx,
                super::AttachRestoredTerminalRequest {
                    session_name: intent.runtime_session_name.clone(),
                    focus: false,
                    kind: AgentKind::from_persisted_kind(intent.record.kind),
                    label: intent.record.label.clone(),
                    agent_uid: intent.record.agent_uid.clone(),
                    clone_source_session_path: intent.clone_source_session_path.clone(),
                    recovery: intent.recovery.clone(),
                },
            )
        };
        match attach_result {
            Ok((agent_id, terminal_id)) => {
                progress.summary.restored_agents += 1;
                let _ = exec
                    .agent_catalog
                    .set_paused(agent_id, intent.record.paused);
                restore_aegis_for_record(
                    exec.agent_catalog,
                    exec.aegis_policy,
                    agent_id,
                    &intent.record,
                );
                if intent.should_mark_startup_pending {
                    if let Some(presentation_store) = exec.presentation_store.as_deref_mut() {
                        presentation_store.mark_startup_pending(terminal_id);
                    }
                }
            }
            Err(error) => {
                let message = format!(
                    "startup attach failed for {}: {error}",
                    intent.runtime_session_name
                );
                append_debug_log(message.clone());
                progress.summary.failed_agents.push(message);
            }
        }
    }
}

fn build_respawn_intent(
    record: &crate::shared::app_state_file::PersistedAgentState,
) -> Result<RespawnIntent, String> {
    let agent_uid = record.agent_uid.clone().ok_or_else(|| {
        "startup respawn skipped for recoverable record missing agent uid".to_owned()
    })?;
    let recovery = record
        .recovery
        .clone()
        .and_then(persisted_recovery_to_agent_recovery)
        .ok_or_else(|| {
            format!(
                "startup respawn skipped for {} missing valid recovery spec",
                record.label.as_deref().unwrap_or("<unlabeled-agent>")
            )
        })?;
    validate_recovery_spec(&recovery).map_err(|error| {
        format!(
            "startup respawn skipped for {}: {error}",
            record.label.as_deref().unwrap_or("<unlabeled-agent>")
        )
    })?;
    let mut launch = launch_spec_for_recovery_spec(&recovery);
    if record.clone_source_session_path.is_some() {
        launch.metadata.clone_source_session_path = record.clone_source_session_path.clone();
    }
    let working_directory = match recovery {
        crate::agents::AgentRecoverySpec::Pi { ref cwd, .. }
        | crate::agents::AgentRecoverySpec::Claude { ref cwd, .. }
        | crate::agents::AgentRecoverySpec::Codex { ref cwd, .. } => Some(cwd.clone()),
    };
    Ok(RespawnIntent {
        record: record.clone(),
        agent_uid,
        launch,
        working_directory,
    })
}

fn build_respawn_intents(
    prune: &[crate::shared::app_state_file::PersistedAgentState],
) -> (Vec<RespawnIntent>, Vec<String>) {
    let mut intents = Vec::new();
    let mut failures = Vec::new();
    for record in prune
        .iter()
        .filter(|record| record.durability() == crate::agents::AgentDurability::Recoverable)
    {
        match build_respawn_intent(record) {
            Ok(intent) => intents.push(intent),
            Err(message) => {
                append_debug_log(message.clone());
                failures.push(message);
            }
        }
    }
    (intents, failures)
}

fn record_skipped_live_only_agents(plan: &RestorePlan, progress: &mut RestoreProgress) {
    for message in plan
        .prune
        .iter()
        .filter(|record| record.durability() == crate::agents::AgentDurability::LiveOnly)
        .map(skipped_live_only_restore_message)
    {
        append_debug_log(message.clone());
        progress.summary.skipped_agents.push(message);
    }
}

fn respawn_recoverable_agents(
    exec: &mut RestoreExecution<'_, '_>,
    plan: &RestorePlan,
    progress: &mut RestoreProgress,
) {
    let (intents, failures) = build_respawn_intents(&plan.prune);
    progress.summary.failed_agents.extend(failures);
    for intent in intents {
        let working_directory = intent.working_directory.as_deref();
        let mut spawn_ctx = super::SpawnAgentContext {
            agent_catalog: exec.agent_catalog,
            runtime_index: exec.runtime_index,
            app_session: exec.app_session,
            selection: exec.selection,
            terminal_manager: exec.terminal_manager,
            focus_state: exec.focus_state,
            owned_tmux_sessions: exec.owned_tmux_sessions,
            active_terminal_content: exec.active_terminal_content,
            runtime_spawner: exec.runtime_spawner,
            input_capture: exec.input_capture,
            app_state_persistence: exec.app_state_persistence,
            visibility_state: exec.visibility_state,
            view_state: exec.view_state,
            presentation_store: exec.presentation_store.as_deref_mut(),
            time: exec.time,
            redraws: exec.redraws,
        };
        match respawn_recovered_agent_with_launch_spec(
            &mut spawn_ctx,
            PERSISTENT_SESSION_PREFIX,
            AgentKind::from_persisted_kind(intent.record.kind),
            intent.agent_uid.clone(),
            intent.record.label.clone(),
            working_directory,
            intent.launch,
        ) {
            Ok(agent_id) => {
                progress.summary.restored_agents += 1;
                let _ = exec
                    .agent_catalog
                    .set_paused(agent_id, intent.record.paused);
                restore_aegis_for_record(
                    exec.agent_catalog,
                    exec.aegis_policy,
                    agent_id,
                    &intent.record,
                );
                if intent.record.last_focused {
                    progress.respawned_focus_agent = Some(agent_id);
                }
            }
            Err(error) => {
                let message = format!(
                    "startup respawn failed for {}: {error}",
                    intent
                        .record
                        .label
                        .as_deref()
                        .unwrap_or("<unlabeled-agent>")
                );
                append_debug_log(message.clone());
                progress.summary.failed_agents.push(message);
            }
        }
    }
}

fn build_restore_focus_candidates(
    plan: &RestorePlan,
    inventory: &LiveSessionInventory,
) -> RestoreFocusCandidates {
    RestoreFocusCandidates {
        restored_focus_session: plan
            .restore
            .iter()
            .find(|record| {
                record.last_focused
                    && record
                        .runtime_session_name
                        .as_deref()
                        .and_then(|session_name| inventory.lookup.get(session_name))
                        .is_some_and(startup_focus_candidate_is_interactive)
            })
            .and_then(|record| record.runtime_session_name.clone()),
        restored_session_names: plan
            .restore
            .iter()
            .filter(|record| {
                record
                    .runtime_session_name
                    .as_deref()
                    .and_then(|session_name| inventory.lookup.get(session_name))
                    .is_some_and(startup_focus_candidate_is_interactive)
            })
            .filter_map(|record| record.runtime_session_name.clone())
            .collect(),
        imported_session_names: plan
            .importable
            .iter()
            .filter_map(|record| record.runtime_session_name.clone())
            .collect(),
    }
}

fn focus_agent_for_restore(exec: &mut RestoreExecution<'_, '_>, agent_id: crate::agents::AgentId) {
    let mut focus_ctx = super::FocusMutationContext {
        session: exec.app_session,
        projection: super::FocusProjectionContext {
            agent_catalog: exec.agent_catalog,
            runtime_index: exec.runtime_index,
            owned_tmux_sessions: exec.owned_tmux_sessions,
            selection: exec.selection,
            active_terminal_content: exec.active_terminal_content,
            terminal_manager: exec.terminal_manager,
            focus_state: exec.focus_state,
            input_capture: exec.input_capture,
            view_state: exec.view_state,
            visibility_state: exec.visibility_state,
        },
        redraws: exec.redraws,
    };
    focus_agent_without_persist(agent_id, VisibilityMode::FocusedOnly, &mut focus_ctx);
}

fn clear_restore_focus(exec: &mut RestoreExecution<'_, '_>) {
    let mut focus_ctx = super::FocusMutationContext {
        session: exec.app_session,
        projection: super::FocusProjectionContext {
            agent_catalog: exec.agent_catalog,
            runtime_index: exec.runtime_index,
            owned_tmux_sessions: exec.owned_tmux_sessions,
            selection: exec.selection,
            active_terminal_content: exec.active_terminal_content,
            terminal_manager: exec.terminal_manager,
            focus_state: exec.focus_state,
            input_capture: exec.input_capture,
            view_state: exec.view_state,
            visibility_state: exec.visibility_state,
        },
        redraws: exec.redraws,
    };
    clear_focus_without_persist(VisibilityMode::ShowAll, &mut focus_ctx);
}

fn spawn_default_agent_after_empty_restore(exec: &mut RestoreExecution<'_, '_>) {
    let mut spawn_ctx = super::SpawnAgentContext {
        agent_catalog: exec.agent_catalog,
        runtime_index: exec.runtime_index,
        app_session: exec.app_session,
        selection: exec.selection,
        terminal_manager: exec.terminal_manager,
        focus_state: exec.focus_state,
        owned_tmux_sessions: exec.owned_tmux_sessions,
        active_terminal_content: exec.active_terminal_content,
        runtime_spawner: exec.runtime_spawner,
        input_capture: exec.input_capture,
        app_state_persistence: exec.app_state_persistence,
        visibility_state: exec.visibility_state,
        view_state: exec.view_state,
        presentation_store: exec.presentation_store.take(),
        time: exec.time,
        redraws: exec.redraws,
    };
    let _ = spawn_agent_terminal(
        &mut spawn_ctx,
        PERSISTENT_SESSION_PREFIX,
        AgentKind::Pi,
        None,
        None,
    );
}

fn finalize_restore_focus(
    exec: &mut RestoreExecution<'_, '_>,
    snapshot_found: bool,
    plan: &RestorePlan,
    inventory: &LiveSessionInventory,
    progress: &RestoreProgress,
) {
    let candidates = build_restore_focus_candidates(plan, inventory);
    let restored_focus_session = candidates.restored_focus_session.as_deref();
    let restored_session_names = candidates
        .restored_session_names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let imported_session_names = candidates
        .imported_session_names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    if let Some(session_name) = choose_startup_focus_session_name(
        restored_focus_session,
        &restored_session_names,
        &imported_session_names,
    ) {
        if let Some(agent_id) = exec.runtime_index.agent_for_session(session_name) {
            focus_agent_for_restore(exec, agent_id);
        }
    } else if let Some(agent_id) = progress.respawned_focus_agent {
        focus_agent_for_restore(exec, agent_id);
    } else if !exec.agent_catalog.order.is_empty() {
        clear_restore_focus(exec);
    } else if !snapshot_found {
        spawn_default_agent_after_empty_restore(exec);
    }
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
    pub(crate) visibility_state: &'a mut crate::hud::TerminalVisibilityState,
    pub(crate) view_state: &'a mut TerminalViewState,
    pub(crate) presentation_store: Option<&'a mut TerminalPresentationStore>,
    pub(crate) time: &'a Time,
    pub(crate) redraws: &'a mut MessageWriter<'w, bevy::window::RequestRedraw>,
}

/// Restores app.
pub(crate) fn restore_app(ctx: &mut RestoreAppContext<'_, '_>) -> RecoveryExecutionSummary {
    let snapshot = load_restore_snapshot(ctx.app_state_persistence);
    let mut progress = RestoreProgress {
        summary: RecoveryExecutionSummary {
            snapshot_found: snapshot.snapshot_found,
            ..RecoveryExecutionSummary::default()
        },
        ..RestoreProgress::default()
    };
    let inventory = match discover_live_sessions(ctx.runtime_spawner) {
        Ok(inventory) => inventory,
        Err(error) => {
            let message = format!("daemon session discovery failed: {error}");
            append_debug_log(message.clone());
            if progress.summary.snapshot_found {
                progress.summary.failed_agents.push(message);
            }
            return progress.summary;
        }
    };
    let plan = plan_restore(&snapshot, &inventory);
    let attach_intents = build_attach_intents(&plan, &inventory);
    if plan.should_mark_app_state_dirty {
        mark_app_state_dirty(ctx.app_state_persistence, None);
    }
    reap_unimportable_live_sessions(ctx.runtime_spawner, &plan.reapable_session_names);

    let mut exec = RestoreExecution {
        agent_catalog: ctx.agent_catalog,
        runtime_index: ctx.runtime_index,
        app_session: ctx.app_session,
        selection: ctx.selection,
        terminal_manager: ctx.terminal_manager,
        focus_state: ctx.focus_state,
        owned_tmux_sessions: ctx.owned_tmux_sessions,
        active_terminal_content: ctx.active_terminal_content,
        runtime_spawner: ctx.runtime_spawner,
        input_capture: ctx.input_capture,
        app_state_persistence: ctx.app_state_persistence,
        aegis_policy: ctx.aegis_policy,
        visibility_state: ctx.visibility_state,
        view_state: ctx.view_state,
        presentation_store: ctx.presentation_store.take(),
        time: ctx.time,
        redraws: ctx.redraws,
    };
    attach_live_agents(&mut exec, &attach_intents, &mut progress);
    record_skipped_live_only_agents(&plan, &mut progress);
    respawn_recoverable_agents(&mut exec, &plan, &mut progress);
    finalize_restore_focus(
        &mut exec,
        snapshot.snapshot_found,
        &plan,
        &inventory,
        &progress,
    );

    progress.summary
}

#[cfg(test)]
mod tests {
    use super::{
        agent_kind_from_daemon_session, build_attach_intents, build_live_session_inventory,
        build_restore_focus_candidates, plan_restore, render_recovery_status_summary,
        skipped_live_only_restore_message, LiveSessionInventory, RecoveryExecutionSummary,
        RestorePlan, RestoreSnapshot,
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
        session_with_metadata("neozeus-session-1", TerminalLifecycle::Running, agent_kind)
    }

    fn session_with_metadata(
        session_id: &str,
        lifecycle: TerminalLifecycle,
        agent_kind: Option<DaemonAgentKind>,
    ) -> DaemonSessionInfo {
        DaemonSessionInfo {
            session_id: session_id.into(),
            runtime: TerminalRuntimeState {
                status: "running".into(),
                lifecycle,
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

    fn persisted_record(
        runtime_session_name: Option<&str>,
        last_focused: bool,
    ) -> PersistedAgentState {
        PersistedAgentState {
            agent_uid: Some("agent-uid-1".into()),
            runtime_session_name: runtime_session_name.map(str::to_owned),
            label: Some("AGENT".into()),
            kind: PersistedAgentKind::Terminal,
            recovery: None,
            clone_source_session_path: None,
            aegis_enabled: false,
            aegis_prompt_text: None,
            paused: false,
            order_index: 0,
            last_focused,
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
            paused: false,
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
            paused: false,
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
            paused: false,
            order_index: 0,
            last_focused: false,
        };

        assert_eq!(
            skipped_live_only_restore_message(&record),
            "startup skipped live-only agent ALPHA: runtime session unavailable"
        );
    }

    #[test]
    fn plan_restore_reaps_noninteractive_imports_and_marks_state_dirty() {
        let snapshot = RestoreSnapshot::default();
        let inventory = build_live_session_inventory(vec![
            session_with_metadata(
                "neozeus-session-running",
                TerminalLifecycle::Running,
                Some(DaemonAgentKind::Terminal),
            ),
            session_with_metadata(
                "neozeus-session-exited",
                TerminalLifecycle::Exited {
                    code: Some(0),
                    signal: None,
                },
                Some(DaemonAgentKind::Terminal),
            ),
        ]);

        let plan = plan_restore(&snapshot, &inventory);

        assert!(plan.should_mark_app_state_dirty);
        assert_eq!(plan.importable.len(), 1);
        assert_eq!(
            plan.importable[0].runtime_session_name.as_deref(),
            Some("neozeus-session-running")
        );
        assert_eq!(
            plan.reapable_session_names,
            vec!["neozeus-session-exited".to_owned()]
        );
    }

    #[test]
    fn build_restore_focus_candidates_ignores_noninteractive_last_focused_sessions() {
        let plan = RestorePlan {
            restore: vec![
                persisted_record(Some("neozeus-session-exited"), true),
                persisted_record(Some("neozeus-session-running"), false),
            ],
            ..RestorePlan::default()
        };
        let inventory: LiveSessionInventory = build_live_session_inventory(vec![
            session_with_metadata(
                "neozeus-session-exited",
                TerminalLifecycle::Exited {
                    code: Some(0),
                    signal: None,
                },
                Some(DaemonAgentKind::Terminal),
            ),
            session_with_metadata(
                "neozeus-session-running",
                TerminalLifecycle::Running,
                Some(DaemonAgentKind::Terminal),
            ),
        ]);

        let candidates = build_restore_focus_candidates(&plan, &inventory);

        assert_eq!(candidates.restored_focus_session, None);
        assert_eq!(
            candidates.restored_session_names,
            vec!["neozeus-session-running".to_owned()]
        );
    }

    #[test]
    fn build_attach_intents_derives_pi_clone_provenance_from_recovery_when_missing() {
        let plan = RestorePlan {
            restore: vec![PersistedAgentState {
                kind: PersistedAgentKind::Pi,
                recovery: Some(
                    crate::shared::app_state_file::PersistedAgentRecoverySpec::Pi {
                        session_path: "/tmp/pi-session.jsonl".into(),
                        cwd: Some("/tmp/demo".into()),
                        is_workdir: false,
                        workdir_slug: None,
                    },
                ),
                clone_source_session_path: None,
                ..persisted_record(Some("neozeus-session-running"), false)
            }],
            ..RestorePlan::default()
        };
        let inventory = build_live_session_inventory(vec![session_with_metadata(
            "neozeus-session-running",
            TerminalLifecycle::Running,
            Some(DaemonAgentKind::Pi),
        )]);

        let intents = build_attach_intents(&plan, &inventory);

        assert_eq!(intents.len(), 1);
        assert_eq!(
            intents[0].clone_source_session_path.as_deref(),
            Some("/tmp/pi-session.jsonl")
        );
        assert!(intents[0].should_mark_startup_pending);
    }
}
