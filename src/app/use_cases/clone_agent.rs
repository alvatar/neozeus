use crate::{
    agents::{AgentCatalog, AgentId, AgentKind, AgentRecoverySpec, AgentRuntimeIndex},
    app::{mark_app_state_dirty, AppStatePersistenceState},
    hud::{HudInputCaptureState, TerminalVisibilityState},
    shared::{
        pi_session_files::{fork_session, read_session_header},
        worktree::{create_worktree, get_current_branch, get_worktree_repo_root, worktree_slug},
    },
    terminals::{
        ActiveTerminalContentState, OwnedTmuxSessionStore, TerminalFocusState, TerminalManager,
        TerminalRuntimeSpawner, TerminalViewState, PERSISTENT_SESSION_PREFIX,
    },
};
use bevy::{prelude::Time, window::RequestRedraw};

use super::{
    claude_fork_launch_spec, codex_fork_launch_spec, generate_provider_session_id,
    pi_launch_spec_for_session_path, spawn_agent_terminal_with_launch_spec,
};

#[allow(
    clippy::too_many_arguments,
    reason = "clone spans provenance, worktree setup, terminal spawn, and selection side effects"
)]
pub(crate) fn clone_agent(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut crate::app::AppSessionState,
    selection: &mut crate::hud::AgentListSelection,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    active_terminal_content: &mut ActiveTerminalContentState,
    runtime_spawner: &TerminalRuntimeSpawner,
    input_capture: &mut HudInputCaptureState,
    app_state_persistence: &mut AppStatePersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    time: &Time,
    source_agent_id: AgentId,
    label: &str,
    workdir: bool,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) -> Result<AgentId, String> {
    let label = agent_catalog
        .validate_new_label(Some(label))?
        .ok_or_else(|| "agent name is required".to_owned())?;
    let Some(kind) = agent_catalog.kind(source_agent_id) else {
        return Err(format!("unknown agent {}", source_agent_id.0));
    };

    let (kind, target_cwd, launch) = match agent_catalog.recovery_spec(source_agent_id) {
        Some(AgentRecoverySpec::Claude {
            session_id,
            cwd,
            model,
            profile,
        }) if kind == AgentKind::Claude => {
            let child_session_id = generate_provider_session_id();
            (
                AgentKind::Claude,
                cwd.clone(),
                claude_fork_launch_spec(
                    session_id,
                    child_session_id,
                    cwd,
                    model.clone(),
                    profile.clone(),
                ),
            )
        }
        Some(AgentRecoverySpec::Codex {
            session_id,
            cwd,
            model,
            profile,
        }) if kind == AgentKind::Codex => (
            AgentKind::Codex,
            cwd.clone(),
            codex_fork_launch_spec(session_id, cwd, model.clone(), profile.clone()),
        ),
        _ if kind == AgentKind::Pi => {
            let source_session_path = agent_catalog
                .clone_source_session_path(source_agent_id)
                .ok_or_else(|| "source Pi agent is missing clone provenance".to_owned())?
                .to_owned();
            let source_header = read_session_header(&source_session_path)?;
            let workdir_slug = workdir.then(|| worktree_slug(&label)).transpose()?;
            let target_cwd = if workdir {
                let repo_root = get_worktree_repo_root(&source_header.cwd)
                    .map_err(|_| format!("Not a git repo: {}", source_header.cwd))?;
                let parent_branch = get_current_branch(&repo_root)
                    .map_err(|error| format!("Cannot determine current branch: {error}"))?;
                create_worktree(
                    &repo_root,
                    workdir_slug
                        .as_deref()
                        .ok_or_else(|| "missing workdir slug".to_owned())?,
                    Some(&parent_branch),
                )?
            } else {
                source_header.cwd
            };
            let clone_session_path = fork_session(&source_session_path, Some(&target_cwd))?;
            (
                AgentKind::Pi,
                target_cwd.clone(),
                pi_launch_spec_for_session_path(clone_session_path, workdir, workdir_slug),
            )
        }
        _ => {
            return Err("only Pi, Claude, and Codex agents can be cloned".to_owned());
        }
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
        presentation_store: None,
        time,
        redraws,
    };
    let agent_id = spawn_agent_terminal_with_launch_spec(
        &mut spawn_ctx,
        PERSISTENT_SESSION_PREFIX,
        kind,
        Some(label),
        Some(target_cwd.as_str()),
        launch,
    )?;
    mark_app_state_dirty(app_state_persistence, Some(time));
    Ok(agent_id)
}
