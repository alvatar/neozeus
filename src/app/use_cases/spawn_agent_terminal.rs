use crate::{
    agents::{AgentCapabilities, AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex},
    app::{AppSessionState, VisibilityMode},
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    startup::StartupLoadingState,
    terminals::{
        append_debug_log, attach_terminal_session, mark_terminal_sessions_dirty,
        TerminalFocusState, TerminalManager, TerminalRuntimeSpawner,
        TerminalSessionPersistenceState, TerminalViewState,
    },
};
use bevy::{prelude::*, window::RequestRedraw};

#[allow(
    clippy::too_many_arguments,
    reason = "spawn crosses daemon, agent, session, and presentation state"
)]
pub(crate) fn spawn_agent_terminal(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    input_capture: &mut HudInputCaptureState,
    session_persistence: &mut TerminalSessionPersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    startup_loading: Option<&mut StartupLoadingState>,
    time: &Time,
    prefix: &str,
    spawn_shell_only: bool,
    kind: AgentKind,
    label: Option<String>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<AgentId, String> {
    let session_name = if spawn_shell_only {
        runtime_spawner.create_shell_session(prefix)
    } else {
        runtime_spawner.create_session(prefix)
    }?;
    let (terminal_id, _) = attach_terminal_session(
        terminal_manager,
        focus_state,
        runtime_spawner,
        session_name.clone(),
        true,
    )?;

    let capabilities = match kind {
        AgentKind::Terminal => AgentCapabilities::terminal_defaults(),
        AgentKind::Verifier => AgentCapabilities::verifier_defaults(),
    };
    let agent_id = agent_catalog.create_agent(label, kind, capabilities);
    let runtime = terminal_manager
        .get(terminal_id)
        .map(|terminal| &terminal.snapshot.runtime);
    runtime_index.link_terminal(agent_id, terminal_id, session_name.clone(), runtime);
    app_session
        .composer
        .bind_agent_terminal(agent_id, terminal_id);
    app_session.active_agent = Some(agent_id);
    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
    view_state.focus_terminal(Some(terminal_id));
    app_session.visibility_mode = VisibilityMode::FocusedOnly;
    visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
    mark_terminal_sessions_dirty(session_persistence, Some(time));
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
pub(crate) fn attach_restored_terminal(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    startup_loading: Option<&mut StartupLoadingState>,
    session_name: String,
    focus: bool,
    kind: AgentKind,
    label: Option<String>,
) -> Result<(AgentId, crate::terminals::TerminalId), String> {
    let (terminal_id, _) = attach_terminal_session(
        terminal_manager,
        focus_state,
        runtime_spawner,
        session_name.clone(),
        focus,
    )?;
    let capabilities = match kind {
        AgentKind::Terminal => AgentCapabilities::terminal_defaults(),
        AgentKind::Verifier => AgentCapabilities::verifier_defaults(),
    };
    let agent_id = agent_catalog.create_agent(label, kind, capabilities);
    let runtime = terminal_manager
        .get(terminal_id)
        .map(|terminal| &terminal.snapshot.runtime);
    runtime_index.link_terminal(agent_id, terminal_id, session_name, runtime);
    app_session
        .composer
        .bind_agent_terminal(agent_id, terminal_id);
    if let Some(startup_loading) = startup_loading {
        startup_loading.register(terminal_id);
    }
    Ok((agent_id, terminal_id))
}
