use super::*;

#[allow(
    clippy::too_many_arguments,
    reason = "spawn crosses daemon, agent, session, and presentation state"
)]
pub(crate) fn spawn_runtime_terminal_session(
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    prefix: &str,
    working_directory: Option<&str>,
    startup_command: Option<&str>,
    env_overrides: &[(String, String)],
    focus: bool,
) -> Result<(String, TerminalId, TerminalBridge), String> {
    let session_name = runtime_spawner.create_session_with_cwd_and_env(
        prefix,
        working_directory,
        startup_command,
        env_overrides,
    )?;
    match attach_terminal_session(
        terminal_manager,
        focus_state,
        runtime_spawner,
        session_name.clone(),
        focus,
    ) {
        Ok((terminal_id, bridge)) => Ok((session_name, terminal_id, bridge)),
        Err(error) => {
            let _ = runtime_spawner.kill_session(&session_name);
            Err(error)
        }
    }
}

/// Attaches restored terminal.
#[allow(
    clippy::too_many_arguments,
    reason = "restore attach crosses daemon, agent, and presentation state"
)]
pub(crate) fn attach_restored_terminal(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    _app_session: &mut AppSessionState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    presentation_store: Option<&mut TerminalPresentationStore>,
    session_name: String,
    focus: bool,
    kind: AgentKind,
    label: Option<String>,
    agent_uid: Option<String>,
    clone_source_session_path: Option<String>,
    recovery: Option<AgentRecoverySpec>,
) -> Result<(AgentId, crate::terminals::TerminalId), String> {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let (terminal_id, _) = attach_terminal_session(
        terminal_manager,
        focus_state,
        runtime_spawner,
        session_name.clone(),
        focus,
    )?;
    let capabilities = kind.capabilities();
    let metadata = AgentMetadata {
        clone_source_session_path,
        recovery,
    };
    let agent_id = match agent_uid {
        Some(agent_uid) => agent_catalog.create_agent_with_uid_and_metadata(
            agent_uid,
            None,
            kind,
            capabilities,
            metadata,
        ),
        None => agent_catalog.create_agent_with_metadata(None, kind, capabilities, metadata),
    };
    if let Some(label) = label {
        match agent_catalog.validate_rename_label(agent_id, &label) {
            Ok(label) => {
                let _ = agent_catalog.rename_agent(agent_id, label);
            }
            Err(error) => {
                append_debug_log(format!(
                    "restored agent label conflict for session {}: {error}; using generated fallback",
                    session_name
                ));
            }
        }
    }
    let runtime = terminal_manager
        .get(terminal_id)
        .map(|terminal| &terminal.snapshot.runtime);
    runtime_index.link_terminal(agent_id, terminal_id, session_name.clone(), runtime);
    if let Err(error) =
        sync_agent_metadata_to_daemon(runtime_spawner, runtime_index, agent_catalog, agent_id)
    {
        append_debug_log(format!(
            "restored session metadata mirror failed for {}: {error}",
            session_name
        ));
    }
    let _ = presentation_store;
    Ok((agent_id, terminal_id))
}
