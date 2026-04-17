use crate::{
    agents::{
        AgentCatalog, AgentId, AgentKind, AgentMetadata, AgentRecoverySpec, AgentRuntimeIndex,
    },
    app::{mark_app_state_dirty, AppStatePersistenceState},
    hud::{HudInputCaptureState, TerminalVisibilityState},
    shared::{pi_session_files::make_new_session_path, shell::shell_quote},
    terminals::{
        append_debug_log, attach_terminal_session, resolve_daemon_socket_path,
        ActiveTerminalContentState, OwnedTmuxSessionStore, TerminalBridge, TerminalFocusState,
        TerminalId, TerminalManager, TerminalPresentationStore, TerminalRuntimeSpawner,
        TerminalViewState,
    },
};

use super::{
    super::session::{AppSessionState, VisibilityMode},
    focus_agent_without_persist, sync_agent_metadata_to_daemon,
};
use bevy::{prelude::*, window::RequestRedraw};
mod provider_metadata;
mod spawn_flow;
mod spawn_launch_specs;
mod spawn_runtime_sessions;

use provider_metadata::{apply_provider_metadata_capture, prepare_provider_metadata_capture};
pub(crate) use spawn_flow::{
    respawn_recovered_agent_with_launch_spec, spawn_agent_terminal,
    spawn_agent_terminal_with_launch_spec,
};
pub(crate) struct SpawnAgentContext<'a, 'w> {
    pub(crate) agent_catalog: &'a mut AgentCatalog,
    pub(crate) runtime_index: &'a mut AgentRuntimeIndex,
    pub(crate) app_session: &'a mut AppSessionState,
    pub(crate) selection: &'a mut crate::hud::AgentListSelection,
    pub(crate) terminal_manager: &'a mut TerminalManager,
    pub(crate) focus_state: &'a mut TerminalFocusState,
    pub(crate) owned_tmux_sessions: &'a OwnedTmuxSessionStore,
    pub(crate) active_terminal_content: &'a mut ActiveTerminalContentState,
    pub(crate) runtime_spawner: &'a TerminalRuntimeSpawner,
    pub(crate) input_capture: &'a mut HudInputCaptureState,
    pub(crate) app_state_persistence: &'a mut AppStatePersistenceState,
    pub(crate) visibility_state: &'a mut TerminalVisibilityState,
    pub(crate) view_state: &'a mut TerminalViewState,
    pub(crate) presentation_store: Option<&'a mut TerminalPresentationStore>,
    pub(crate) time: &'a Time,
    pub(crate) redraws: &'a mut MessageWriter<'w, RequestRedraw>,
}
use spawn_launch_specs::build_agent_launch_spec;
pub(crate) use spawn_launch_specs::{
    claude_fork_launch_spec, codex_fork_launch_spec, generate_provider_session_id,
    launch_spec_for_recovery_spec, pi_launch_spec_for_session_path, AgentLaunchSpec,
};
pub(crate) use spawn_runtime_sessions::{attach_restored_terminal, spawn_runtime_terminal_session};

pub(crate) struct SpawnAgentRequest<'a> {
    pub(crate) prefix: &'a str,
    pub(crate) kind: AgentKind,
    pub(crate) label: Option<String>,
    pub(crate) working_directory: Option<&'a str>,
    pub(crate) launch: AgentLaunchSpec,
    pub(crate) focus_terminal: bool,
    pub(crate) persist_mutation: bool,
    pub(crate) restored_agent_uid: Option<String>,
}

pub(crate) struct SpawnRuntimeTerminalSessionContext<'a> {
    pub(crate) terminal_manager: &'a mut TerminalManager,
    pub(crate) focus_state: &'a mut TerminalFocusState,
    pub(crate) runtime_spawner: &'a TerminalRuntimeSpawner,
}

pub(crate) struct SpawnRuntimeTerminalSessionRequest<'a> {
    pub(crate) prefix: &'a str,
    pub(crate) working_directory: Option<&'a str>,
    pub(crate) startup_command: Option<&'a str>,
    pub(crate) env_overrides: &'a [(String, String)],
    pub(crate) focus: bool,
}

pub(crate) struct AttachRestoredTerminalContext<'a> {
    pub(crate) agent_catalog: &'a mut AgentCatalog,
    pub(crate) runtime_index: &'a mut AgentRuntimeIndex,
    pub(crate) terminal_manager: &'a mut TerminalManager,
    pub(crate) focus_state: &'a mut TerminalFocusState,
    pub(crate) runtime_spawner: &'a TerminalRuntimeSpawner,
    pub(crate) presentation_store: Option<&'a mut TerminalPresentationStore>,
}

pub(crate) struct AttachRestoredTerminalRequest {
    pub(crate) session_name: String,
    pub(crate) focus: bool,
    pub(crate) kind: AgentKind,
    pub(crate) label: Option<String>,
    pub(crate) agent_uid: Option<String>,
    pub(crate) clone_source_session_path: Option<String>,
    pub(crate) recovery: Option<AgentRecoverySpec>,
}

use std::time::Duration;

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
            TerminalManager, TerminalPresentationStore, TerminalRuntimeSpawner, TerminalViewState,
        },
    };
    use bevy::{ecs::system::RunSystemOnce, prelude::*, window::RequestRedraw};
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    struct AttachSuccessDaemonClient {
        created_sessions: Mutex<Vec<String>>,
    }

    impl AttachSuccessDaemonClient {
        fn new() -> Self {
            Self {
                created_sessions: Mutex::new(Vec::new()),
            }
        }
    }

    impl TerminalDaemonClient for AttachSuccessDaemonClient {
        fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
            Ok(Vec::new())
        }

        fn update_session_metadata(
            &self,
            _session_id: &str,
            _metadata: &crate::shared::daemon_wire::DaemonSessionMetadata,
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
            let (_tx, rx) = std::sync::mpsc::channel();
            Ok(AttachedDaemonSession {
                snapshot: Default::default(),
                updates: rx,
            })
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

        fn kill_session(&self, _session_id: &str) -> Result<(), String> {
            Ok(())
        }
    }

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

        fn update_session_metadata(
            &self,
            _session_id: &str,
            _metadata: &crate::shared::daemon_wire::DaemonSessionMetadata,
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
            &mut super::SpawnRuntimeTerminalSessionContext {
                terminal_manager: &mut terminal_manager,
                focus_state: &mut focus_state,
                runtime_spawner: &runtime_spawner,
            },
            super::SpawnRuntimeTerminalSessionRequest {
                prefix: "neozeus-session-",
                working_directory: None,
                startup_command: None,
                env_overrides: &[],
                focus: true,
            },
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
    fn spawn_agent_terminal_marks_awaiting_first_frame_without_startup_bootstrap_pending() {
        let daemon = Arc::new(AttachSuccessDaemonClient::new());
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
        world.insert_resource(TerminalPresentationStore::default());
        world.init_resource::<Messages<RequestRedraw>>();

        let agent_id = world
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
                 mut presentation_store: ResMut<TerminalPresentationStore>,
                 mut redraws: MessageWriter<RequestRedraw>| {
                    let mut spawn_ctx = super::SpawnAgentContext {
                        agent_catalog: &mut agent_catalog,
                        runtime_index: &mut runtime_index,
                        app_session: &mut app_session,
                        selection: &mut selection,
                        terminal_manager: &mut terminal_manager,
                        focus_state: &mut focus_state,
                        owned_tmux_sessions: &owned_tmux_sessions,
                        active_terminal_content: &mut active_terminal_content,
                        runtime_spawner: &runtime_spawner,
                        input_capture: &mut input_capture,
                        app_state_persistence: &mut app_state_persistence,
                        visibility_state: &mut visibility_state,
                        view_state: &mut view_state,
                        presentation_store: Some(&mut presentation_store),
                        time: &time,
                        redraws: &mut redraws,
                    };
                    spawn_agent_terminal_with_launch_spec(
                        &mut spawn_ctx,
                        "neozeus-session-",
                        AgentKind::Terminal,
                        Some("alpha".into()),
                        None,
                        AgentLaunchSpec {
                            startup_command: None,
                            metadata: AgentMetadata::default(),
                        },
                    )
                },
            )
            .unwrap()
            .expect("spawn should succeed");

        let terminal_id = world
            .resource::<AgentRuntimeIndex>()
            .primary_terminal(agent_id)
            .expect("spawned agent should be linked to a terminal");
        let presentation_store = world.resource::<TerminalPresentationStore>();
        assert!(presentation_store.is_awaiting_first_frame(terminal_id));
        assert!(!presentation_store.is_startup_bootstrap_pending(terminal_id));
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
                    let mut spawn_ctx = super::SpawnAgentContext {
                        agent_catalog: &mut agent_catalog,
                        runtime_index: &mut runtime_index,
                        app_session: &mut app_session,
                        selection: &mut selection,
                        terminal_manager: &mut terminal_manager,
                        focus_state: &mut focus_state,
                        owned_tmux_sessions: &owned_tmux_sessions,
                        active_terminal_content: &mut active_terminal_content,
                        runtime_spawner: &runtime_spawner,
                        input_capture: &mut input_capture,
                        app_state_persistence: &mut app_state_persistence,
                        visibility_state: &mut visibility_state,
                        view_state: &mut view_state,
                        presentation_store: None,
                        time: &time,
                        redraws: &mut redraws,
                    };
                    spawn_agent_terminal_with_launch_spec(
                        &mut spawn_ctx,
                        "neozeus-session-",
                        AgentKind::Terminal,
                        Some("alpha".into()),
                        None,
                        AgentLaunchSpec {
                            startup_command: None,
                            metadata: AgentMetadata::default(),
                        },
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
