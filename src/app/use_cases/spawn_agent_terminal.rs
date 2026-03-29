use crate::{
    agents::{AgentCapabilities, AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex},
    app::{mark_app_state_dirty, AppStatePersistenceState},
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    startup::StartupLoadingState,
    terminals::{
        append_debug_log, attach_terminal_session, TerminalFocusState, TerminalManager,
        TerminalRuntimeSpawner, TerminalViewState,
    },
};

use super::super::session::{AppSessionState, VisibilityMode};
use bevy::{prelude::*, window::RequestRedraw};

#[allow(
    clippy::too_many_arguments,
    reason = "spawn crosses daemon, agent, session, and presentation state"
)]
/// Spawns agent terminal.
pub(crate) fn spawn_agent_terminal(
    agent_catalog: &mut AgentCatalog,
    runtime_index: &mut AgentRuntimeIndex,
    app_session: &mut AppSessionState,
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
    spawn_shell_only: bool,
    kind: AgentKind,
    label: Option<String>,
    working_directory: Option<&str>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> Result<AgentId, String> {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let label = agent_catalog.validate_new_label(label.as_deref())?;
    let session_name = if spawn_shell_only {
        runtime_spawner.create_shell_session_with_cwd(prefix, working_directory)
    } else {
        runtime_spawner.create_session_with_cwd(prefix, working_directory)
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
    app_session.active_agent = Some(agent_id);
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
) -> Result<(AgentId, crate::terminals::TerminalId), String> {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
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
    let agent_id = agent_catalog.create_agent(None, kind, capabilities);
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
