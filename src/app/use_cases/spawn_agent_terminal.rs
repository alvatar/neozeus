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
    focus_agent_without_persist,
};
use bevy::{prelude::*, window::RequestRedraw};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentLaunchSpec {
    pub(crate) startup_command: Option<String>,
    pub(crate) metadata: AgentMetadata,
}

static NEXT_PROVIDER_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) fn generate_provider_session_id() -> String {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = NEXT_PROVIDER_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let mixed = now_nanos ^ counter;
    let tail = (now_nanos.wrapping_add(counter)) & 0xffff_ffff_ffff;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        ((mixed >> 96) & 0xffff_ffff) as u32,
        ((mixed >> 80) & 0xffff) as u16,
        ((mixed >> 64) & 0xffff) as u16,
        ((mixed >> 48) & 0xffff) as u16,
        tail as u64,
    )
}

fn build_agent_launch_spec(
    kind: AgentKind,
    working_directory: Option<&str>,
) -> Result<AgentLaunchSpec, String> {
    if kind == AgentKind::Pi {
        let session_path = make_new_session_path(working_directory)?;
        return Ok(pi_launch_spec_for_session_path(session_path, false, None));
    }

    if kind == AgentKind::Claude {
        let session_id = generate_provider_session_id();
        let cwd = crate::shared::pi_session_files::resolve_session_cwd(working_directory)?;
        return Ok(AgentLaunchSpec {
            startup_command: Some(format!("claude --session-id {}", shell_quote(&session_id))),
            metadata: AgentMetadata {
                clone_source_session_path: None,
                recovery: Some(AgentRecoverySpec::Claude {
                    session_id,
                    cwd,
                    model: None,
                    profile: None,
                }),
            },
        });
    }

    Ok(AgentLaunchSpec {
        startup_command: kind.bootstrap_command().map(str::to_owned),
        metadata: AgentMetadata::default(),
    })
}

pub(crate) fn claude_fork_launch_spec(
    parent_session_id: &str,
    child_session_id: String,
    cwd: &str,
    model: Option<String>,
    profile: Option<String>,
) -> AgentLaunchSpec {
    let mut command = format!(
        "claude --resume {} --fork-session --session-id {}",
        shell_quote(parent_session_id),
        shell_quote(&child_session_id)
    );
    if let Some(model) = model.as_deref() {
        command.push_str(" --model ");
        command.push_str(&shell_quote(model));
    }
    if let Some(profile) = profile.as_deref() {
        command.push_str(" -p ");
        command.push_str(&shell_quote(profile));
    }
    AgentLaunchSpec {
        startup_command: Some(command),
        metadata: AgentMetadata {
            clone_source_session_path: None,
            recovery: Some(AgentRecoverySpec::Claude {
                session_id: child_session_id,
                cwd: cwd.to_owned(),
                model,
                profile,
            }),
        },
    }
}

pub(crate) fn codex_fork_launch_spec(
    parent_session_id: &str,
    cwd: &str,
    model: Option<String>,
    profile: Option<String>,
) -> AgentLaunchSpec {
    let mut command = format!(
        "codex fork {} -C {}",
        shell_quote(parent_session_id),
        shell_quote(cwd)
    );
    if let Some(model) = model.as_deref() {
        command.push_str(" -m ");
        command.push_str(&shell_quote(model));
    }
    if let Some(profile) = profile.as_deref() {
        command.push_str(" -p ");
        command.push_str(&shell_quote(profile));
    }
    AgentLaunchSpec {
        startup_command: Some(command),
        metadata: AgentMetadata::default(),
    }
}

pub(crate) fn launch_spec_for_recovery_spec(recovery: &AgentRecoverySpec) -> AgentLaunchSpec {
    match recovery {
        AgentRecoverySpec::Pi {
            session_path,
            is_workdir,
            workdir_slug,
            ..
        } => {
            pi_launch_spec_for_session_path(session_path.clone(), *is_workdir, workdir_slug.clone())
        }
        AgentRecoverySpec::Claude {
            session_id,
            cwd: _,
            model,
            profile,
        } => {
            let mut command = format!("claude --resume {}", shell_quote(session_id));
            if let Some(model) = model {
                command.push_str(" --model ");
                command.push_str(&shell_quote(model));
            }
            if let Some(profile) = profile {
                command.push_str(" -p ");
                command.push_str(&shell_quote(profile));
            }
            AgentLaunchSpec {
                startup_command: Some(command),
                metadata: AgentMetadata {
                    clone_source_session_path: None,
                    recovery: Some(recovery.clone()),
                },
            }
        }
        AgentRecoverySpec::Codex {
            session_id,
            cwd,
            model,
            profile,
        } => {
            let mut command = format!("codex resume {}", shell_quote(session_id));
            if let Some(model) = model {
                command.push_str(" -m ");
                command.push_str(&shell_quote(model));
            }
            if let Some(profile) = profile {
                command.push_str(" -p ");
                command.push_str(&shell_quote(profile));
            }
            command.push_str(" -C ");
            command.push_str(&shell_quote(cwd));
            AgentLaunchSpec {
                startup_command: Some(command),
                metadata: AgentMetadata {
                    clone_source_session_path: None,
                    recovery: Some(recovery.clone()),
                },
            }
        }
    }
}

pub(crate) fn pi_launch_spec_for_session_path(
    session_path: String,
    is_workdir: bool,
    workdir_slug: Option<String>,
) -> AgentLaunchSpec {
    let cwd = crate::shared::pi_session_files::read_session_header(&session_path)
        .map(|header| header.cwd)
        .unwrap_or_default();
    AgentLaunchSpec {
        startup_command: Some(format!("pi --session {}", shell_quote(&session_path))),
        metadata: AgentMetadata {
            clone_source_session_path: Some(session_path.clone()),
            recovery: Some(AgentRecoverySpec::Pi {
                session_path,
                cwd,
                is_workdir,
                workdir_slug,
            }),
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
    let codex_capture = if kind == AgentKind::Codex && launch.metadata.recovery.is_none() {
        crate::shared::pi_session_files::resolve_session_cwd(working_directory)
            .ok()
            .map(|cwd| {
                (
                    cwd,
                    crate::shared::codex_state::codex_thread_ids().unwrap_or_default(),
                )
            })
    } else {
        None
    };

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
    if focus_terminal {
        focus_agent_without_persist(
            agent_id,
            VisibilityMode::FocusedOnly,
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
            redraws,
        );
    }
    if let Some((cwd, known_thread_ids)) = codex_capture {
        match crate::shared::codex_state::wait_for_new_codex_thread_id(
            &cwd,
            &known_thread_ids,
            Duration::from_secs(3),
        ) {
            Ok(Some(session_id)) => {
                let _ = agent_catalog.set_recovery_spec(
                    agent_id,
                    Some(AgentRecoverySpec::Codex {
                        session_id,
                        cwd,
                        model: None,
                        profile: None,
                    }),
                );
            }
            Ok(None) => match crate::shared::codex_state::latest_codex_thread_id_for_cwd(&cwd) {
                Ok(Some(session_id)) => {
                    let _ = agent_catalog.set_recovery_spec(
                        agent_id,
                        Some(AgentRecoverySpec::Codex {
                            session_id,
                            cwd,
                            model: None,
                            profile: None,
                        }),
                    );
                }
                Ok(None) => append_debug_log(format!(
                    "codex recovery capture timed out for agent {}",
                    agent_id.0
                )),
                Err(error) => append_debug_log(format!(
                    "codex recovery fallback failed for agent {}: {error}",
                    agent_id.0
                )),
            },
            Err(error) => append_debug_log(format!(
                "codex recovery capture failed for agent {}: {error}",
                agent_id.0
            )),
        }
    }
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
    runtime_index.link_terminal(agent_id, terminal_id, session_name, runtime);
    let _ = presentation_store;
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
