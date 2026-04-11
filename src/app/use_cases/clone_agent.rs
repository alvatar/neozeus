use crate::{
    agents::{AgentId, AgentKind, AgentRecoverySpec},
    app::mark_app_state_dirty,
    shared::{
        pi_session_files::{fork_session, read_session_header},
        worktree::{create_worktree, get_current_branch, get_worktree_repo_root, worktree_slug},
    },
    terminals::PERSISTENT_SESSION_PREFIX,
};

use super::{
    claude_fork_launch_spec, codex_fork_launch_spec, generate_provider_session_id,
    pi_launch_spec_for_session_path, spawn_agent_terminal_with_launch_spec,
};

pub(crate) struct CloneAgentContext<'a, 'w> {
    pub(crate) spawn: super::SpawnAgentContext<'a, 'w>,
}

pub(crate) fn clone_agent(
    source_agent_id: AgentId,
    label: &str,
    workdir: bool,
    ctx: &mut CloneAgentContext<'_, '_>,
) -> Result<AgentId, String> {
    let agent_catalog = &mut *ctx.spawn.agent_catalog;
    let time = ctx.spawn.time;

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

    let agent_id = spawn_agent_terminal_with_launch_spec(
        &mut ctx.spawn,
        PERSISTENT_SESSION_PREFIX,
        kind,
        Some(label),
        Some(target_cwd.as_str()),
        launch,
    )?;
    mark_app_state_dirty(ctx.spawn.app_state_persistence, Some(time));
    Ok(agent_id)
}
