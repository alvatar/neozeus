use crate::{
    agents::{AgentCatalog, AgentId, AgentKind, AgentMetadata, AgentRuntimeIndex},
    app::{mark_app_state_dirty, AppStatePersistenceState},
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    shared::pi_session_files::make_new_session_path,
    startup::StartupLoadingState,
    terminals::{
        append_debug_log, attach_terminal_session, resolve_daemon_socket_path, TerminalFocusState,
        TerminalManager, TerminalRuntimeSpawner, TerminalViewState,
    },
};

use super::super::session::{AppSessionState, VisibilityMode};
use bevy::{prelude::*, window::RequestRedraw};

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentLaunchSpec {
    startup_command: Option<String>,
    metadata: AgentMetadata,
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    if value
        .bytes()
        .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'_' | b'-'))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn build_agent_launch_spec(
    kind: AgentKind,
    working_directory: Option<&str>,
) -> Result<AgentLaunchSpec, String> {
    if kind == AgentKind::Pi {
        let session_path = make_new_session_path(working_directory)?;
        return Ok(AgentLaunchSpec {
            startup_command: Some(format!("pi --session {}", shell_quote(&session_path))),
            metadata: AgentMetadata {
                clone_source_session_path: Some(session_path),
                is_workdir: false,
            },
        });
    }

    Ok(AgentLaunchSpec {
        startup_command: kind.bootstrap_command().map(str::to_owned),
        metadata: AgentMetadata::default(),
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "spawn crosses daemon, agent, session, and presentation state"
)]
/// Spawns agent terminal.
pub(crate) fn spawn_agent_terminal(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
    selection: &mut crate::hud::AgentListSelection,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    input_capture: &mut HudInputCaptureState,
    app_state_persistence: &mut AppStatePersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    startup_loading: Option<&mut StartupLoadingState>,
    time: &Time,
    prefix: &str,
    kind: AgentKind,
    label: Option<String>,
    working_directory: Option<&str>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<AgentId, String> {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let launch = build_agent_launch_spec(kind, working_directory)?;
    let identity = agent_catalog.allocate_identity_with_metadata(
        label.as_deref(),
        kind,
        kind.capabilities(),
        launch.metadata,
    )?;
    let mut env_overrides = vec![
        ("NEOZEUS_AGENT_UID".to_owned(), identity.uid.clone()),
        ("NEOZEUS_AGENT_LABEL".to_owned(), identity.label.clone()),
        (
            "NEOZEUS_AGENT_KIND".to_owned(),
            identity.kind.env_name().to_owned(),
        ),
    ];
    if let Some(socket_path) = resolve_daemon_socket_path() {
        env_overrides.push((
            "NEOZEUS_DAEMON_SOCKET".to_owned(),
            socket_path.to_string_lossy().into_owned(),
        ));
    }
    let session_name = runtime_spawner.create_session_with_cwd_and_env(
        prefix,
        working_directory,
        launch.startup_command.as_deref(),
        &env_overrides,
    )?;
    let (terminal_id, _) = attach_terminal_session(
        terminal_manager,
        focus_state,
        runtime_spawner,
        session_name.clone(),
        true,
    )?;

    let agent_id = agent_catalog.create_agent_from_identity(identity);
    let runtime = terminal_manager
        .get(terminal_id)
        .map(|terminal| &terminal.snapshot.runtime);
    runtime_index.link_terminal(agent_id, terminal_id, session_name.clone(), runtime);
    app_session.active_agent = Some(agent_id);
    *selection = crate::hud::AgentListSelection::Agent(agent_id);
    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
    view_state.focus_terminal(Some(terminal_id));
    app_session.visibility_mode = VisibilityMode::FocusedOnly;
    visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
    mark_app_state_dirty(app_state_persistence, Some(time));
    if let Some(startup_loading) = startup_loading {
        startup_loading.register(terminal_id);
    }
    append_debug_log(format!(
        "spawned agent {} terminal {} session={}",
        agent_id.0, terminal_id.0, session_name
    ));
    redraws.write(RequestRedraw);
    Ok(agent_id)
}

#[allow(
    clippy::too_many_arguments,
    reason = "restore attach crosses daemon, agent, and presentation state"
)]
/// Attaches restored terminal.
pub(crate) fn attach_restored_terminal(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    _app_session: &mut AppSessionState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    startup_loading: Option<&mut StartupLoadingState>,
    session_name: String,
    focus: bool,
    kind: AgentKind,
    label: Option<String>,
    agent_uid: Option<String>,
    clone_source_session_path: Option<String>,
    is_workdir: bool,
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
        is_workdir,
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
    runtime_index.link_terminal(agent_id, terminal_id, session_name, runtime);
    if let Some(startup_loading) = startup_loading {
        startup_loading.register(terminal_id);
    }
    Ok((agent_id, terminal_id))
}
