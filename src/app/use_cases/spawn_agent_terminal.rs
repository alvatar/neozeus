use crate::{
    agents::{AgentCatalog, AgentId, AgentKind, AgentMetadata, AgentRuntimeIndex},
    app::{mark_app_state_dirty, AppStatePersistenceState},
    hud::{HudInputCaptureState, TerminalVisibilityState},
    shared::pi_session_files::make_new_session_path,
    terminals::{
        append_debug_log, attach_terminal_session, resolve_daemon_socket_path,
        ActiveTerminalContentState, OwnedTmuxSessionStore, TerminalBridge, TerminalFocusState,
        TerminalId, TerminalManager, TerminalPresentationStore, TerminalRuntimeSpawner,
        TerminalViewState,
    },
};

use super::{
    super::session::{AppSessionState, VisibilityMode},
    apply_focus_intent,
};
use bevy::{prelude::*, window::RequestRedraw};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentLaunchSpec {
    pub(crate) startup_command: Option<String>,
    pub(crate) metadata: AgentMetadata,
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    if value.bytes().all(|byte| {
        matches!(
            byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'_' | b'-'
        )
    }) {
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
        return Ok(pi_launch_spec_for_session_path(session_path, false, None));
    }

    Ok(AgentLaunchSpec {
        startup_command: kind.bootstrap_command().map(str::to_owned),
        metadata: AgentMetadata::default(),
    })
}

pub(crate) fn pi_launch_spec_for_session_path(
    session_path: String,
    is_workdir: bool,
    workdir_slug: Option<String>,
) -> AgentLaunchSpec {
    AgentLaunchSpec {
        startup_command: Some(format!("pi --session {}", shell_quote(&session_path))),
        metadata: AgentMetadata {
            clone_source_session_path: Some(session_path),
            is_workdir,
            workdir_slug,
        },
    }
}

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
        true,
    )?;

    let agent_id = agent_catalog.create_agent_from_identity(identity);
    let runtime = terminal_manager
        .get(terminal_id)
        .map(|terminal| &terminal.snapshot.runtime);
    runtime_index.link_terminal(agent_id, terminal_id, session_name.clone(), runtime);
    app_session
        .focus_intent
        .focus_agent(agent_id, VisibilityMode::FocusedOnly);
    apply_focus_intent(
        app_session,
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
    );
    mark_app_state_dirty(app_state_persistence, Some(time));
    if let Some(presentation_store) = presentation_store {
        presentation_store.mark_startup_pending(terminal_id);
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
    presentation_store: Option<&mut TerminalPresentationStore>,
    session_name: String,
    focus: bool,
    kind: AgentKind,
    label: Option<String>,
    agent_uid: Option<String>,
    clone_source_session_path: Option<String>,
    is_workdir: bool,
    workdir_slug: Option<String>,
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
        workdir_slug,
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
    if let Some(presentation_store) = presentation_store {
        presentation_store.mark_startup_pending(terminal_id);
    }
    Ok((agent_id, terminal_id))
}

#[cfg(test)]
mod tests {
    use super::{
        spawn_agent_terminal_with_launch_spec, spawn_runtime_terminal_session, AgentLaunchSpec,
    };
    use crate::{
        agents::{AgentCatalog, AgentKind, AgentMetadata, AgentRuntimeIndex},
        app::{AppSessionState, AppStatePersistenceState},
        terminals::{
            AttachedDaemonSession, DaemonSessionInfo, OwnedTmuxSessionInfo, TerminalCommand,
            TerminalDaemonClient, TerminalDaemonClientResource, TerminalFocusState,
            TerminalManager, TerminalRuntimeSpawner, TerminalViewState,
        },
    };
    use bevy::{ecs::system::RunSystemOnce, prelude::*, window::RequestRedraw};
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    struct AttachFailDaemonClient {
        created_sessions: Mutex<Vec<String>>,
        killed_sessions: Mutex<Vec<String>>,
    }

    impl AttachFailDaemonClient {
        fn new() -> Self {
            Self {
                created_sessions: Mutex::new(Vec::new()),
                killed_sessions: Mutex::new(Vec::new()),
            }
        }
    }

    impl TerminalDaemonClient for AttachFailDaemonClient {
        fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
            Ok(Vec::new())
        }

        fn update_session_metadata_label(
            &self,
            _session_id: &str,
            _agent_label: Option<&str>,
        ) -> Result<(), String> {
            Ok(())
        }

        fn create_session_with_env(
            &self,
            prefix: &str,
            _cwd: Option<&str>,
            _env_overrides: &[(String, String)],
        ) -> Result<String, String> {
            let session_id = format!("{prefix}1");
            self.created_sessions
                .lock()
                .unwrap()
                .push(session_id.clone());
            Ok(session_id)
        }

        fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String> {
            Ok(Vec::new())
        }

        fn create_owned_tmux_session(
            &self,
            _owner_agent_uid: &str,
            _display_name: &str,
            _cwd: Option<&str>,
            _command: &str,
        ) -> Result<OwnedTmuxSessionInfo, String> {
            Err("unused in test".into())
        }

        fn capture_owned_tmux_session(
            &self,
            _session_uid: &str,
            _lines: usize,
        ) -> Result<String, String> {
            Err("unused in test".into())
        }

        fn kill_owned_tmux_session(&self, _session_uid: &str) -> Result<(), String> {
            Err("unused in test".into())
        }

        fn kill_owned_tmux_sessions_for_agent(&self, _owner_agent_uid: &str) -> Result<(), String> {
            Err("unused in test".into())
        }

        fn attach_session(&self, _session_id: &str) -> Result<AttachedDaemonSession, String> {
            Err("attach failed".into())
        }

        fn send_command(&self, _session_id: &str, _command: TerminalCommand) -> Result<(), String> {
            Ok(())
        }

        fn resize_session(
            &self,
            _session_id: &str,
            _cols: usize,
            _rows: usize,
        ) -> Result<(), String> {
            Ok(())
        }

        fn kill_session(&self, session_id: &str) -> Result<(), String> {
            self.killed_sessions
                .lock()
                .unwrap()
                .push(session_id.to_owned());
            Ok(())
        }
    }

    #[test]
    fn spawn_runtime_terminal_session_rolls_back_created_session_when_attach_fails() {
        let daemon = Arc::new(AttachFailDaemonClient::new());
        let runtime_spawner = TerminalRuntimeSpawner::for_tests(
            TerminalDaemonClientResource::from_client(daemon.clone()),
        );
        let mut terminal_manager = TerminalManager::default();
        let mut focus_state = TerminalFocusState::default();

        let error = spawn_runtime_terminal_session(
            &mut terminal_manager,
            &mut focus_state,
            &runtime_spawner,
            "neozeus-session-",
            None,
            None,
            &[],
            true,
        )
        .err()
        .expect("attach failure should bubble up");

        assert_eq!(error, "attach failed");
        assert_eq!(
            daemon.created_sessions.lock().unwrap().as_slice(),
            ["neozeus-session-1"]
        );
        assert_eq!(
            daemon.killed_sessions.lock().unwrap().as_slice(),
            ["neozeus-session-1"]
        );
        assert!(terminal_manager.terminal_ids().is_empty());
        assert_eq!(focus_state.active_id(), None);
    }

    #[test]
    fn spawn_agent_terminal_rolls_back_created_session_when_attach_fails() {
        let daemon = Arc::new(AttachFailDaemonClient::new());
        let runtime_spawner = TerminalRuntimeSpawner::for_tests(
            TerminalDaemonClientResource::from_client(daemon.clone()),
        );
        let mut world = World::default();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        world.insert_resource(time);
        world.insert_resource(AgentCatalog::default());
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(crate::hud::AgentListSelection::default());
        world.insert_resource(TerminalManager::default());
        world.insert_resource(TerminalFocusState::default());
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        world.insert_resource(runtime_spawner);
        world.insert_resource(crate::hud::HudInputCaptureState::default());
        world.insert_resource(AppStatePersistenceState::default());
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(TerminalViewState::default());
        world.init_resource::<Messages<RequestRedraw>>();

        let error = world
            .run_system_once(
                |time: Res<Time>,
                 mut agent_catalog: ResMut<AgentCatalog>,
                 mut runtime_index: ResMut<AgentRuntimeIndex>,
                 mut app_session: ResMut<AppSessionState>,
                 mut selection: ResMut<crate::hud::AgentListSelection>,
                 mut terminal_manager: ResMut<TerminalManager>,
                 mut focus_state: ResMut<TerminalFocusState>,
                 owned_tmux_sessions: Res<crate::terminals::OwnedTmuxSessionStore>,
                 mut active_terminal_content: ResMut<
                    crate::terminals::ActiveTerminalContentState,
                >,
                 runtime_spawner: Res<TerminalRuntimeSpawner>,
                 mut input_capture: ResMut<crate::hud::HudInputCaptureState>,
                 mut app_state_persistence: ResMut<AppStatePersistenceState>,
                 mut visibility_state: ResMut<crate::hud::TerminalVisibilityState>,
                 mut view_state: ResMut<TerminalViewState>,
                 mut redraws: MessageWriter<RequestRedraw>| {
                    spawn_agent_terminal_with_launch_spec(
                        &mut agent_catalog,
                        &mut runtime_index,
                        &mut app_session,
                        &mut selection,
                        &mut terminal_manager,
                        &mut focus_state,
                        &owned_tmux_sessions,
                        &mut active_terminal_content,
                        &runtime_spawner,
                        &mut input_capture,
                        &mut app_state_persistence,
                        &mut visibility_state,
                        &mut view_state,
                        None,
                        &time,
                        "neozeus-session-",
                        AgentKind::Terminal,
                        Some("alpha".into()),
                        None,
                        AgentLaunchSpec {
                            startup_command: None,
                            metadata: AgentMetadata::default(),
                        },
                        &mut redraws,
                    )
                },
            )
            .unwrap()
            .expect_err("attach failure should bubble up");

        assert_eq!(error, "attach failed");
        assert_eq!(
            daemon.created_sessions.lock().unwrap().as_slice(),
            ["neozeus-session-1"]
        );
        assert_eq!(
            daemon.killed_sessions.lock().unwrap().as_slice(),
            ["neozeus-session-1"]
        );
        assert!(world.resource::<AgentCatalog>().order.is_empty());
        assert!(world
            .resource::<AgentRuntimeIndex>()
            .session_to_agent
            .is_empty());
    }
}
