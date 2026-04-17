use super::*;

fn spawn_agent_terminal_internal(
    ctx: &mut SpawnAgentContext<'_, '_>,
    request: SpawnAgentRequest<'_>,
) -> Result<AgentId, String> {
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
    let visibility_state = &mut *ctx.visibility_state;
    let view_state = &mut *ctx.view_state;
    let presentation_store = ctx.presentation_store.as_deref_mut();
    let time = ctx.time;
    let redraws = &mut *ctx.redraws;
    let SpawnAgentRequest {
        prefix,
        kind,
        label,
        working_directory,
        launch,
        focus_terminal,
        persist_mutation,
        restored_agent_uid,
    } = request;
    let capabilities = kind.capabilities();
    let pending_identity = match restored_agent_uid {
        Some(agent_uid) => {
            let label = label
                .as_deref()
                .and_then(|value| agent_catalog.validate_new_label(Some(value)).ok().flatten())
                .or(label)
                .unwrap_or_else(|| format!("RESTORED-{}", agent_catalog.order.len() + 1));
            crate::agents::PendingAgentIdentity {
                uid: agent_uid,
                label,
                kind,
                capabilities,
                metadata: launch.metadata.clone(),
            }
        }
        None => agent_catalog.allocate_identity_with_metadata(
            label.as_deref(),
            kind,
            capabilities,
            launch.metadata.clone(),
        )?,
    };
    let agent_uid = pending_identity.uid.clone();
    let agent_label = pending_identity.label.clone();
    let provider_capture = prepare_provider_metadata_capture(kind, &launch, working_directory);

    let mut env_overrides = vec![
        ("NEOZEUS_AGENT_UID".to_owned(), agent_uid),
        ("NEOZEUS_AGENT_LABEL".to_owned(), agent_label),
        ("NEOZEUS_AGENT_KIND".to_owned(), kind.env_name().to_owned()),
    ];
    if let Some(socket_path) = resolve_daemon_socket_path() {
        env_overrides.extend(crate::shared::daemon_socket::daemon_socket_env_pairs(
            &socket_path,
        ));
    }
    let mut runtime_ctx = SpawnRuntimeTerminalSessionContext {
        terminal_manager,
        focus_state,
        runtime_spawner,
    };
    let (session_name, terminal_id, _) = spawn_runtime_terminal_session(
        &mut runtime_ctx,
        SpawnRuntimeTerminalSessionRequest {
            prefix,
            working_directory,
            startup_command: launch.startup_command.as_deref(),
            env_overrides: &env_overrides,
            focus: focus_terminal,
        },
    )?;

    let agent_id = agent_catalog.create_agent_from_identity(pending_identity);
    let runtime = terminal_manager
        .get(terminal_id)
        .map(|terminal| &terminal.snapshot.runtime);
    runtime_index.link_terminal(agent_id, terminal_id, session_name.clone(), runtime);
    if let Err(error) =
        sync_agent_metadata_to_daemon(runtime_spawner, runtime_index, agent_catalog, agent_id)
    {
        append_debug_log(format!(
            "failed to mirror agent metadata to daemon for session {session_name}: {error}"
        ));
    }
    if focus_terminal {
        let mut focus_ctx = super::super::FocusMutationContext {
            session: app_session,
            projection: super::super::FocusProjectionContext {
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
    apply_provider_metadata_capture(agent_catalog, agent_id, provider_capture);
    if persist_mutation {
        mark_app_state_dirty(app_state_persistence, Some(time));
    }
    if let Some(presentation_store) = presentation_store {
        presentation_store.mark_awaiting_first_frame(terminal_id);
    }
    append_debug_log(format!(
        "spawned agent {} terminal {} session={}",
        agent_id.0, terminal_id.0, session_name
    ));
    if !focus_terminal {
        redraws.write(RequestRedraw);
    }
    Ok(agent_id)
}

pub(crate) fn spawn_agent_terminal_with_launch_spec(
    ctx: &mut SpawnAgentContext<'_, '_>,
    prefix: &str,
    kind: AgentKind,
    label: Option<String>,
    working_directory: Option<&str>,
    launch: AgentLaunchSpec,
) -> Result<AgentId, String> {
    spawn_agent_terminal_internal(
        ctx,
        SpawnAgentRequest {
            prefix,
            kind,
            label,
            working_directory,
            launch,
            focus_terminal: true,
            persist_mutation: true,
            restored_agent_uid: None,
        },
    )
}

pub(crate) fn respawn_recovered_agent_with_launch_spec(
    ctx: &mut SpawnAgentContext<'_, '_>,
    prefix: &str,
    kind: AgentKind,
    agent_uid: String,
    label: Option<String>,
    working_directory: Option<&str>,
    launch: AgentLaunchSpec,
) -> Result<AgentId, String> {
    spawn_agent_terminal_internal(
        ctx,
        SpawnAgentRequest {
            prefix,
            kind,
            label,
            working_directory,
            launch,
            focus_terminal: false,
            persist_mutation: false,
            restored_agent_uid: Some(agent_uid),
        },
    )
}

/// Spawns agent terminal.
pub(crate) fn spawn_agent_terminal(
    ctx: &mut SpawnAgentContext<'_, '_>,
    prefix: &str,
    kind: AgentKind,
    label: Option<String>,
    working_directory: Option<&str>,
) -> Result<AgentId, String> {
    let launch = build_agent_launch_spec(kind, working_directory)?;
    spawn_agent_terminal_with_launch_spec(ctx, prefix, kind, label, working_directory, launch)
}
