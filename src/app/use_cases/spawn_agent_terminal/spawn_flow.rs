use super::*;

#[allow(
    clippy::too_many_arguments,
    reason = "spawn crosses daemon, agent, session, and presentation state"
)]
fn spawn_agent_terminal_internal(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
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
    presentation_store: Option<&mut TerminalPresentationStore>,
    time: &Time,
    prefix: &str,
    kind: AgentKind,
    label: Option<String>,
    working_directory: Option<&str>,
    launch: AgentLaunchSpec,
    focus_terminal: bool,
    persist_mutation: bool,
    restored_agent_uid: Option<String>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<AgentId, String> {
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
    let (session_name, terminal_id, _) = spawn_runtime_terminal_session(
        terminal_manager,
        focus_state,
        runtime_spawner,
        prefix,
        working_directory,
        launch.startup_command.as_deref(),
        &env_overrides,
        focus_terminal,
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
        presentation_store.mark_startup_pending(terminal_id);
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

#[allow(
    clippy::too_many_arguments,
    reason = "spawn crosses daemon, agent, session, and presentation state"
)]
pub(crate) fn spawn_agent_terminal_with_launch_spec(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
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
    presentation_store: Option<&mut TerminalPresentationStore>,
    time: &Time,
    prefix: &str,
    kind: AgentKind,
    label: Option<String>,
    working_directory: Option<&str>,
    launch: AgentLaunchSpec,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<AgentId, String> {
    spawn_agent_terminal_internal(
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
        prefix,
        kind,
        label,
        working_directory,
        launch,
        true,
        true,
        None,
        redraws,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "startup recovery respawn crosses daemon, agent, session, and presentation state"
)]
pub(crate) fn respawn_recovered_agent_with_launch_spec(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
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
    presentation_store: Option<&mut TerminalPresentationStore>,
    time: &Time,
    prefix: &str,
    kind: AgentKind,
    agent_uid: String,
    label: Option<String>,
    working_directory: Option<&str>,
    launch: AgentLaunchSpec,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<AgentId, String> {
    spawn_agent_terminal_internal(
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
        prefix,
        kind,
        label,
        working_directory,
        launch,
        false,
        false,
        Some(agent_uid),
        redraws,
    )
}

/// Spawns agent terminal.
#[allow(
    clippy::too_many_arguments,
    reason = "spawn crosses daemon, agent, session, and presentation state"
)]
pub(crate) fn spawn_agent_terminal(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
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
    presentation_store: Option<&mut TerminalPresentationStore>,
    time: &Time,
    prefix: &str,
    kind: AgentKind,
    label: Option<String>,
    working_directory: Option<&str>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<AgentId, String> {
    let launch = build_agent_launch_spec(kind, working_directory)?;
    spawn_agent_terminal_with_launch_spec(
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
        prefix,
        kind,
        label,
        working_directory,
        launch,
        redraws,
    )
}
