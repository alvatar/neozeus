use super::*;

pub(crate) fn spawn_runtime_terminal_session(
    ctx: &mut SpawnRuntimeTerminalSessionContext<'_>,
    request: SpawnRuntimeTerminalSessionRequest<'_>,
) -> Result<(String, TerminalId, TerminalBridge), String> {
    let SpawnRuntimeTerminalSessionRequest {
        prefix,
        working_directory,
        startup_command,
        env_overrides,
        focus,
    } = request;
    let session_name = ctx.runtime_spawner.create_session_with_cwd_and_env(
        prefix,
        working_directory,
        startup_command,
        env_overrides,
    )?;
    match attach_terminal_session(
        ctx.terminal_manager,
        ctx.focus_state,
        ctx.runtime_spawner,
        session_name.clone(),
        focus,
    ) {
        Ok((terminal_id, bridge)) => Ok((session_name, terminal_id, bridge)),
        Err(error) => {
            let _ = ctx.runtime_spawner.kill_session(&session_name);
            Err(error)
        }
    }
}

/// Attaches restored terminal.
pub(crate) fn attach_restored_terminal(
    ctx: &mut AttachRestoredTerminalContext<'_>,
    request: AttachRestoredTerminalRequest,
) -> Result<(AgentId, crate::terminals::TerminalId), String> {
    let AttachRestoredTerminalRequest {
        session_name,
        focus,
        kind,
        label,
        agent_uid,
        clone_source_session_path,
        recovery,
    } = request;
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let (terminal_id, _) = attach_terminal_session(
        ctx.terminal_manager,
        ctx.focus_state,
        ctx.runtime_spawner,
        session_name.clone(),
        focus,
    )?;
    let capabilities = kind.capabilities();
    let metadata = AgentMetadata {
        clone_source_session_path,
        recovery,
    };
    let agent_id = match agent_uid {
        Some(agent_uid) => ctx.agent_catalog.create_agent_with_uid_and_metadata(
            agent_uid,
            None,
            kind,
            capabilities,
            metadata,
        ),
        None => ctx
            .agent_catalog
            .create_agent_with_metadata(None, kind, capabilities, metadata),
    };
    if let Some(label) = label {
        match ctx.agent_catalog.validate_rename_label(agent_id, &label) {
            Ok(label) => {
                let _ = ctx.agent_catalog.rename_agent(agent_id, label);
            }
            Err(error) => {
                append_debug_log(format!(
                    "restored agent label conflict for session {}: {error}; using generated fallback",
                    session_name
                ));
            }
        }
    }
    let runtime = ctx
        .terminal_manager
        .get(terminal_id)
        .map(|terminal| &terminal.snapshot.runtime);
    ctx.runtime_index
        .link_terminal(agent_id, terminal_id, session_name.clone(), runtime);
    if let Err(error) = sync_agent_metadata_to_daemon(
        ctx.runtime_spawner,
        ctx.runtime_index,
        ctx.agent_catalog,
        agent_id,
    ) {
        append_debug_log(format!(
            "restored session metadata mirror failed for {}: {error}",
            session_name
        ));
    }
    let _ = ctx.presentation_store;
    Ok((agent_id, terminal_id))
}
