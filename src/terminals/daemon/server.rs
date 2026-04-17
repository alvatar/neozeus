use super::super::debug::append_debug_log;
use super::{
    owned_tmux::{
        capture_owned_tmux_session, create_owned_tmux_session, kill_owned_tmux_session,
        kill_owned_tmux_sessions_for_agent, owned_tmux_sessions_by_uid, OwnedTmuxSessionInfo,
    },
    protocol::{
        read_client_message, write_server_message, ClientMessage, DaemonRequest, DaemonResponse,
        DaemonSessionInfo, ServerMessage,
    },
    session::{DaemonSession, SubscriberIdAllocator},
};
use std::{
    collections::HashMap,
    fs, io,
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

#[derive(Clone)]
struct DaemonRegistry {
    inner: Arc<Mutex<DaemonRegistryInner>>,
}

struct DaemonRegistryInner {
    next_session_counter: u64,
    sessions: HashMap<String, Arc<DaemonSession>>,
    owned_tmux_sessions: HashMap<String, OwnedTmuxSessionInfo>,
}

impl Default for DaemonRegistry {
    /// Creates an empty daemon registry with session ids starting at 1.
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DaemonRegistryInner {
                next_session_counter: 1,
                sessions: HashMap::new(),
                owned_tmux_sessions: owned_tmux_sessions_by_uid().unwrap_or_default(),
            })),
        }
    }
}

impl DaemonRegistry {
    /// Returns the current daemon sessions sorted by daemon creation order.
    ///
    /// The registry stores sessions in a hash map, so the sort step preserves stable UI-facing order.
    fn list_sessions(&self) -> Vec<DaemonSessionInfo> {
        // Session list order follows daemon creation order, not lexical session ids. Dead sessions
        // remain listed until an explicit kill/reap so the UI can inspect final runtime state.
        let registry = lock(&self.inner);
        let mut sessions = registry
            .sessions
            .values()
            .map(|session| session.info())
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.created_order);
        sessions
    }

    fn refresh_owned_tmux_sessions(&self) -> Result<(), String> {
        let sessions = owned_tmux_sessions_by_uid()?;
        lock(&self.inner).owned_tmux_sessions = sessions;
        Ok(())
    }

    fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String> {
        self.refresh_owned_tmux_sessions()?;
        let mut sessions = lock(&self.inner)
            .owned_tmux_sessions
            .values()
            .cloned()
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| {
            left.created_unix
                .cmp(&right.created_unix)
                .then_with(|| left.tmux_name.cmp(&right.tmux_name))
        });
        Ok(sessions)
    }

    fn create_owned_tmux_session(
        &self,
        owner_agent_uid: &str,
        display_name: &str,
        cwd: Option<&str>,
        command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String> {
        let session = create_owned_tmux_session(owner_agent_uid, display_name, cwd, command)?;
        self.refresh_owned_tmux_sessions()?;
        Ok(session)
    }

    fn capture_owned_tmux_session(
        &self,
        session_uid: &str,
        lines: usize,
    ) -> Result<String, String> {
        self.refresh_owned_tmux_sessions()?;
        capture_owned_tmux_session(session_uid, lines)
    }

    fn kill_owned_tmux_session(&self, session_uid: &str) -> Result<(), String> {
        self.refresh_owned_tmux_sessions()?;
        kill_owned_tmux_session(session_uid)?;
        self.refresh_owned_tmux_sessions()?;
        Ok(())
    }

    fn kill_owned_tmux_sessions_for_agent(&self, owner_agent_uid: &str) -> Result<(), String> {
        self.refresh_owned_tmux_sessions()?;
        kill_owned_tmux_sessions_for_agent(owner_agent_uid)?;
        self.refresh_owned_tmux_sessions()?;
        Ok(())
    }

    /// Allocates a fresh daemon session id, starts the session worker, and registers it.
    ///
    /// Prefix validation happens up front so the daemon never creates empty or whitespace-only session
    /// names.
    fn create_session(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        env_overrides: &[(String, String)],
    ) -> Result<String, String> {
        // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
        if prefix.trim().is_empty() {
            return Err("daemon session prefix must not be empty".to_owned());
        }
        let (session_id, created_order) = {
            let mut registry = lock(&self.inner);
            let created_order = registry.next_session_counter;
            let session_id = format!("{prefix}{created_order}");
            registry.next_session_counter += 1;
            (session_id, created_order)
        };
        let session = DaemonSession::start(session_id.clone(), created_order, cwd, env_overrides)?;
        let mut registry = lock(&self.inner);
        if registry
            .sessions
            .insert(session_id.clone(), session)
            .is_some()
        {
            return Err(format!("daemon session `{session_id}` already existed"));
        }
        Ok(session_id)
    }

    fn update_session_metadata(
        &self,
        session_id: &str,
        metadata: crate::shared::daemon_wire::DaemonSessionMetadata,
    ) -> Result<(), String> {
        self.session(session_id)?.update_metadata(metadata);
        Ok(())
    }

    /// Looks up one registered daemon session by id.
    fn session(&self, session_id: &str) -> Result<Arc<DaemonSession>, String> {
        let registry = lock(&self.inner);
        registry
            .sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| format!("daemon session `{session_id}` not found"))
    }

    /// Hard-kills a daemon session, waits for its worker teardown to finish, and only then removes
    /// it from the registry.
    fn kill_session(&self, session_id: &str) -> Result<(), String> {
        let session = self.session(session_id)?;
        session.kill()?;
        let mut registry = lock(&self.inner);
        registry.sessions.remove(session_id);
        Ok(())
    }
}

/// Runs the production daemon server until process shutdown.
///
/// This is the public entry point used by `neozeus daemon`.
pub(crate) fn run_daemon_server(socket_path: &Path) -> Result<(), String> {
    run_server_loop(socket_path, Arc::new(AtomicBool::new(false)))
}

/// Binds the daemon socket, accepts client connections, and spawns one handler thread per client.
///
/// The shared registry lives for the duration of the loop so sessions survive client reconnects.
fn run_server_loop(socket_path: &Path, stop: Arc<AtomicBool>) -> Result<(), String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let listener = bind_listener(socket_path)?;
    append_debug_log(format!("daemon server listening {}", socket_path.display()));
    let registry = DaemonRegistry::default();
    let subscriber_ids = Arc::new(SubscriberIdAllocator::default());
    let connection_ids = Arc::new(AtomicU64::new(1));

    loop {
        let (stream, _) = listener
            .accept()
            .map_err(|error| format!("daemon accept failed: {error}"))?;
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let registry = registry.clone();
        let subscriber_ids = subscriber_ids.clone();
        let connection_id = connection_ids.fetch_add(1, Ordering::Relaxed);
        thread::spawn(move || {
            if let Err(error) = handle_connection(connection_id, registry, subscriber_ids, stream) {
                append_debug_log(format!("daemon connection closed with error: {error}"));
            }
        });
    }

    let _ = fs::remove_file(socket_path);
    Ok(())
}

/// Binds the daemon's Unix listener socket, cleaning up stale socket files when needed.
///
/// If an existing socket still accepts connections, it is treated as a running daemon and binding is
/// refused. Failed connection attempts are only treated as stale when the error class is one we know
/// is safe to replace; ambiguous errors leave the socket in place and fail startup conservatively.
fn bind_listener(socket_path: &Path) -> Result<UnixListener, String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create daemon socket dir {}: {error}",
                parent.display()
            )
        })?;
    }
    if socket_path.exists() {
        match UnixStream::connect(socket_path) {
            Ok(_) => {
                return Err(format!(
                    "daemon already running at {}",
                    socket_path.display()
                ))
            }
            Err(error) if stale_socket_connect_error(&error) => {
                fs::remove_file(socket_path).map_err(|remove_error| {
                    format!(
                        "failed to remove stale daemon socket {} after {:?}: {remove_error}",
                        socket_path.display(),
                        error.kind()
                    )
                })?;
            }
            Err(error) => {
                return Err(format!(
                    "existing daemon socket {} is not safely replaceable: {error}",
                    socket_path.display()
                ));
            }
        }
    }
    UnixListener::bind(socket_path).map_err(|error| {
        format!(
            "failed to bind daemon socket {}: {error}",
            socket_path.display()
        )
    })
}

fn stale_socket_connect_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
    )
}

/// Services one daemon client connection until the socket closes.
///
/// Requests are handled synchronously on the read thread, while outgoing server messages are written by
/// a dedicated writer thread so event producers never write directly to the socket.
fn handle_connection(
    connection_id: u64,
    registry: DaemonRegistry,
    subscriber_ids: Arc<SubscriberIdAllocator>,
    stream: UnixStream,
) -> Result<(), String> {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    append_debug_log(format!("daemon client connected {connection_id}"));
    let mut reader = stream
        .try_clone()
        .map_err(|error| format!("failed to clone daemon client stream: {error}"))?;
    let mut writer = stream;
    let (server_tx, server_rx) = mpsc::channel::<ServerMessage>();
    let writer_thread = thread::spawn(move || {
        while let Ok(message) = server_rx.recv() {
            if write_server_message(&mut writer, &message).is_err() {
                break;
            }
        }
    });

    let mut subscriptions = Vec::<(Arc<DaemonSession>, u64)>::new();
    while let Ok(message) = read_client_message(&mut reader) {
        match message {
            ClientMessage::Request {
                request_id,
                request,
            } => {
                let response = handle_request(
                    &registry,
                    &subscriber_ids,
                    &server_tx,
                    &mut subscriptions,
                    request,
                );
                let _ = server_tx.send(ServerMessage::Response {
                    request_id,
                    response,
                });
            }
        }
    }

    for (session, subscriber_id) in subscriptions {
        session.unsubscribe(subscriber_id);
    }
    drop(server_tx);
    let _ = writer_thread.join();
    append_debug_log(format!("daemon client disconnected {connection_id}"));
    Ok(())
}

/// Executes one decoded daemon request against the registry/session state.
///
/// Attach requests also install a subscriber so future session updates stream back over the same
/// connection.
fn handle_request(
    registry: &DaemonRegistry,
    subscriber_ids: &SubscriberIdAllocator,
    server_tx: &mpsc::Sender<ServerMessage>,
    subscriptions: &mut Vec<(Arc<DaemonSession>, u64)>,
    request: DaemonRequest,
) -> Result<DaemonResponse, String> {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    match request {
        DaemonRequest::ListSessions => Ok(DaemonResponse::SessionList {
            sessions: registry.list_sessions(),
        }),
        DaemonRequest::ListSessionsDetailed => Ok(DaemonResponse::SessionListDetailed {
            sessions: registry.list_sessions(),
        }),
        DaemonRequest::CreateSession {
            prefix,
            cwd,
            env_overrides,
        } => Ok(DaemonResponse::SessionCreated {
            session_id: registry.create_session(&prefix, cwd.as_deref(), &env_overrides)?,
        }),
        DaemonRequest::ListOwnedTmuxSessions => Ok(DaemonResponse::OwnedTmuxSessionList {
            sessions: registry.list_owned_tmux_sessions()?,
        }),
        DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid,
            display_name,
            cwd,
            command,
        } => Ok(DaemonResponse::OwnedTmuxSessionCreated {
            session: registry.create_owned_tmux_session(
                &owner_agent_uid,
                &display_name,
                cwd.as_deref(),
                &command,
            )?,
        }),
        DaemonRequest::CaptureOwnedTmuxSession { session_uid, lines } => {
            Ok(DaemonResponse::OwnedTmuxSessionCapture {
                session_uid: session_uid.clone(),
                text: registry.capture_owned_tmux_session(&session_uid, lines)?,
            })
        }
        DaemonRequest::AttachSession { session_id } => {
            let session = registry.session(&session_id)?;
            let subscriber_id = subscriber_ids.next();
            let attached = session.subscribe(subscriber_id, server_tx.clone());
            subscriptions.push((session, attached.subscriber_id));
            Ok(DaemonResponse::SessionAttached {
                session_id,
                snapshot: attached.snapshot,
                revision: attached.revision,
            })
        }
        DaemonRequest::SendCommand {
            session_id,
            command,
        } => {
            registry.session(&session_id)?.send_command(command)?;
            Ok(DaemonResponse::Ack)
        }
        DaemonRequest::ResizeSession {
            session_id,
            cols,
            rows,
        } => {
            registry.session(&session_id)?.resize(cols, rows)?;
            Ok(DaemonResponse::Ack)
        }
        DaemonRequest::KillSession { session_id } => {
            registry.kill_session(&session_id)?;
            subscriptions.retain(|(session, subscriber_id)| {
                if session.session_id() == session_id {
                    session.unsubscribe(*subscriber_id);
                    false
                } else {
                    true
                }
            });
            Ok(DaemonResponse::Ack)
        }
        DaemonRequest::KillOwnedTmuxSession { session_uid } => {
            registry.kill_owned_tmux_session(&session_uid)?;
            Ok(DaemonResponse::Ack)
        }
        DaemonRequest::KillOwnedTmuxSessionsForAgent { owner_agent_uid } => {
            registry.kill_owned_tmux_sessions_for_agent(&owner_agent_uid)?;
            Ok(DaemonResponse::Ack)
        }
        DaemonRequest::UpdateSessionMetadata {
            session_id,
            metadata,
        } => {
            registry.update_session_metadata(&session_id, metadata)?;
            Ok(DaemonResponse::Ack)
        }
    }
}

/// Locks this value.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
pub(crate) use tests::DaemonServerHandle;


#[cfg(test)]
mod tests {
    use super::*;
    use std::{io, path::PathBuf, time::Duration};

    pub(crate) struct DaemonServerHandle {
        stop: Arc<AtomicBool>,
        join: Option<thread::JoinHandle<()>>,
        socket_path: PathBuf,
    }

    impl DaemonServerHandle {
        /// Starts a test daemon server thread and waits until its socket becomes reachable.
        pub(crate) fn start(socket_path: PathBuf) -> Result<Self, String> {
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let worker_path = socket_path.clone();
            let join = thread::spawn(move || {
                if let Err(error) = run_server_loop(&worker_path, worker_stop) {
                    append_debug_log(format!("daemon server stopped with error: {error}"));
                }
            });
            wait_for_socket(&socket_path, Duration::from_secs(2))?;
            Ok(Self {
                stop,
                join: Some(join),
                socket_path,
            })
        }
    }

    impl Drop for DaemonServerHandle {
        /// Stops the test daemon thread, nudges `accept()` awake, joins it, and removes the socket file.
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = UnixStream::connect(&self.socket_path);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
            let _ = fs::remove_file(&self.socket_path);
        }
    }

    /// Polls until the test daemon socket both exists and accepts connections.
    fn wait_for_socket(socket_path: &Path, timeout: Duration) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if socket_path.exists() && UnixStream::connect(socket_path).is_ok() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "timed out waiting for daemon socket {}",
                    socket_path.display()
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn stale_socket_classifier_accepts_only_safe_replacement_errors() {
        assert!(stale_socket_connect_error(&io::Error::from(
            io::ErrorKind::ConnectionRefused
        )));
        assert!(stale_socket_connect_error(&io::Error::from(
            io::ErrorKind::NotFound
        )));
        assert!(!stale_socket_connect_error(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
    }

    #[test]
    fn bind_listener_refuses_to_replace_active_socket() {
        let dir = crate::tests::temp_dir("neozeus-daemon-bind-active");
        let socket_path = dir.join("daemon.sock");
        let _listener = bind_listener(&socket_path).expect("initial bind should succeed");

        let error = bind_listener(&socket_path).expect_err("active socket should be refused");
        assert!(error.contains("daemon already running at"));
        assert!(socket_path.exists());
        let _client = UnixStream::connect(&socket_path).expect("active socket should remain usable");
    }
}
