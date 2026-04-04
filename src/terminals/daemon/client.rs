use super::super::debug::append_debug_log;
use super::super::types::{TerminalCommand, TerminalSnapshot, TerminalUpdate};
use super::owned_tmux::OwnedTmuxSessionInfo;
use super::protocol::{
    read_server_message, write_client_message, ClientMessage, DaemonEvent, DaemonRequest,
    DaemonResponse, DaemonSessionInfo, ServerMessage,
};
#[cfg(any(test, debug_assertions))]
use crate::clone_state::{
    load_cloned_daemon_state, resolve_cloned_daemon_state_path, ClonedDaemonState,
};
use crate::shared::daemon_socket::resolve_daemon_socket_path;
use bevy::prelude::Resource;
use std::{
    collections::HashMap,
    env,
    net::Shutdown,
    os::unix::net::UnixStream,
    path::Path,
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

const DAEMON_CONNECT_RETRY_TIMEOUT: Duration = Duration::from_secs(2);
const DAEMON_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

type PendingResponses = HashMap<u64, mpsc::Sender<Result<DaemonResponse, String>>>;

#[derive(Debug)]
pub(crate) struct AttachedDaemonSession {
    pub(crate) snapshot: TerminalSnapshot,
    pub(crate) updates: mpsc::Receiver<TerminalUpdate>,
}

pub(crate) trait TerminalDaemonClient: Send + Sync {
    /// Returns the daemon's current session list with runtime/revision metadata.
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String>;
    /// Asks the daemon to create a new session id using the provided prefix, optional working directory,
    /// and per-session environment overrides.
    fn create_session_with_env(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        env_overrides: &[(String, String)],
    ) -> Result<String, String>;
    #[allow(
        dead_code,
        reason = "trait convenience helper remains available for tests and direct clients"
    )]
    /// Asks the daemon to create a new session id using the provided prefix and optional working directory.
    fn create_session(&self, prefix: &str, cwd: Option<&str>) -> Result<String, String> {
        self.create_session_with_env(prefix, cwd, &[])
    }
    /// Lists all persistent agent-owned tmux child sessions currently discoverable by the daemon.
    fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String>;
    #[allow(
        dead_code,
        reason = "owned tmux creation is exercised through the concrete socket client and helper CLI"
    )]
    /// Creates one persistent agent-owned tmux session.
    fn create_owned_tmux_session(
        &self,
        owner_agent_uid: &str,
        display_name: &str,
        cwd: Option<&str>,
        command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String>;
    /// Captures one owned tmux session's pane text for read-only inspection.
    fn capture_owned_tmux_session(&self, session_uid: &str, lines: usize)
        -> Result<String, String>;
    /// Kills one owned tmux session by stable uid.
    fn kill_owned_tmux_session(&self, session_uid: &str) -> Result<(), String>;
    /// Kills all owned tmux sessions belonging to one agent uid.
    fn kill_owned_tmux_sessions_for_agent(&self, owner_agent_uid: &str) -> Result<(), String>;
    /// Attaches to one daemon session and returns its current snapshot plus a live update stream.
    fn attach_session(&self, session_id: &str) -> Result<AttachedDaemonSession, String>;
    /// Sends one terminal command into the named daemon session.
    fn send_command(&self, session_id: &str, command: TerminalCommand) -> Result<(), String>;
    #[allow(
        dead_code,
        reason = "protocol includes resize even before the UI drives it"
    )]
    /// Requests a PTY resize for the named daemon session.
    fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String>;
    /// Terminates the named daemon session and removes it from the daemon registry.
    fn kill_session(&self, session_id: &str) -> Result<(), String>;
}

#[derive(Resource, Clone)]
pub(crate) struct TerminalDaemonClientResource {
    inner: Arc<dyn TerminalDaemonClient>,
}

impl TerminalDaemonClientResource {
    /// Builds the Bevy resource wrapper around the selected daemon client.
    ///
    /// Production startup uses the socket-backed daemon client. Debug/test builds additionally honor
    /// an isolated clone-state bundle so offscreen verification can replay cloned live state without
    /// touching the Oracle's daemon.
    pub(crate) fn system() -> Result<Self, String> {
        #[cfg(any(test, debug_assertions))]
        if let Some(path) = resolve_cloned_daemon_state_path() {
            return Ok(Self {
                inner: Arc::new(ClonedTerminalDaemonClient::from_path(&path)?),
            });
        }
        Ok(Self {
            inner: Arc::new(SocketTerminalDaemonClient::connect_or_start_default()?),
        })
    }

    /// Builds this value from client.
    #[cfg(test)]
    pub(crate) fn from_client<T>(client: Arc<T>) -> Self
    where
        T: TerminalDaemonClient + 'static,
    {
        Self { inner: client }
    }

    /// Returns the erased daemon-client trait object stored inside the resource.
    pub(crate) fn client(&self) -> &dyn TerminalDaemonClient {
        self.inner.as_ref()
    }
}

#[cfg(any(test, debug_assertions))]
struct ClonedTerminalDaemonClient {
    state: ClonedDaemonState,
}

#[cfg(any(test, debug_assertions))]
impl ClonedTerminalDaemonClient {
    fn from_path(path: &Path) -> Result<Self, String> {
        Ok(Self {
            state: load_cloned_daemon_state(path)?,
        })
    }

    fn read_only_error(operation: &str) -> String {
        format!("cloned daemon state is read-only; {operation} is unavailable")
    }
}

#[cfg(any(test, debug_assertions))]
impl TerminalDaemonClient for ClonedTerminalDaemonClient {
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        Ok(self
            .state
            .sessions
            .iter()
            .map(|session| DaemonSessionInfo {
                session_id: session.session_id.clone(),
                runtime: session.snapshot.runtime.clone(),
                revision: session.revision,
                created_order: session.order_index,
            })
            .collect())
    }

    fn create_session_with_env(
        &self,
        _prefix: &str,
        _cwd: Option<&str>,
        _env_overrides: &[(String, String)],
    ) -> Result<String, String> {
        Err(Self::read_only_error("session creation"))
    }

    fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String> {
        Ok(self
            .state
            .owned_tmux_sessions
            .iter()
            .map(|session| session.info.clone())
            .collect())
    }

    fn create_owned_tmux_session(
        &self,
        _owner_agent_uid: &str,
        _display_name: &str,
        _cwd: Option<&str>,
        _command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String> {
        Err(Self::read_only_error("owned tmux creation"))
    }

    fn capture_owned_tmux_session(
        &self,
        session_uid: &str,
        _lines: usize,
    ) -> Result<String, String> {
        self.state
            .owned_tmux_sessions
            .iter()
            .find(|session| session.info.session_uid == session_uid)
            .map(|session| session.capture_text.clone())
            .ok_or_else(|| format!("owned tmux session `{session_uid}` not found in cloned state"))
    }

    fn kill_owned_tmux_session(&self, _session_uid: &str) -> Result<(), String> {
        Err(Self::read_only_error("owned tmux kill"))
    }

    fn kill_owned_tmux_sessions_for_agent(&self, _owner_agent_uid: &str) -> Result<(), String> {
        Err(Self::read_only_error("owned tmux owner kill"))
    }

    fn attach_session(&self, session_id: &str) -> Result<AttachedDaemonSession, String> {
        let session = self
            .state
            .sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .ok_or_else(|| format!("cloned daemon session `{session_id}` not found"))?;
        let (_tx, rx) = mpsc::channel();
        Ok(AttachedDaemonSession {
            snapshot: session.snapshot.clone(),
            updates: rx,
        })
    }

    fn send_command(&self, _session_id: &str, _command: TerminalCommand) -> Result<(), String> {
        Err(Self::read_only_error("send command"))
    }

    fn resize_session(&self, _session_id: &str, _cols: usize, _rows: usize) -> Result<(), String> {
        Err(Self::read_only_error("resize session"))
    }

    fn kill_session(&self, _session_id: &str) -> Result<(), String> {
        Err(Self::read_only_error("kill session"))
    }
}

pub(crate) struct SocketTerminalDaemonClient {
    writer_tx: mpsc::Sender<ClientMessage>,
    pending: Arc<Mutex<PendingResponses>>,
    session_routes: Arc<Mutex<HashMap<String, mpsc::Sender<TerminalUpdate>>>>,
    next_request_id: Mutex<u64>,
    shutdown_stream: Mutex<Option<UnixStream>>,
}

impl SocketTerminalDaemonClient {
    /// Connects to the default daemon socket, spawning a background daemon process if the first
    /// connect attempt fails.
    ///
    /// This is the "just make the daemon exist" entry point used by normal app startup.
    fn connect_or_start_default() -> Result<Self, String> {
        let socket_path = resolve_daemon_socket_path()
            .ok_or_else(|| "failed to resolve daemon socket path".to_owned())?;
        match Self::connect(&socket_path) {
            Ok(client) => Ok(client),
            Err(connect_error) => {
                append_debug_log(format!(
                    "daemon connect failed {}: {connect_error}; attempting start",
                    socket_path.display()
                ));
                spawn_daemon_subprocess(&socket_path)?;
                wait_for_connect(&socket_path, DAEMON_CONNECT_RETRY_TIMEOUT)
            }
        }
    }

    /// Connects to an already-running daemon socket and starts the reader/writer background threads.
    ///
    /// Requests are sent through a writer channel, responses are matched back to waiting callers by
    /// request id, and session update events are fanned out by session id.
    pub(crate) fn connect(socket_path: &Path) -> Result<Self, String> {
        // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
        let stream = UnixStream::connect(socket_path).map_err(|error| {
            format!(
                "failed to connect daemon socket {}: {error}",
                socket_path.display()
            )
        })?;
        let mut reader = stream
            .try_clone()
            .map_err(|error| format!("failed to clone daemon socket: {error}"))?;
        let shutdown_stream = stream
            .try_clone()
            .map_err(|error| format!("failed to clone daemon shutdown socket: {error}"))?;
        let mut writer = stream;
        let (writer_tx, writer_rx) = mpsc::channel::<ClientMessage>();
        let pending = Arc::new(Mutex::new(PendingResponses::new()));
        let session_routes = Arc::new(Mutex::new(
            HashMap::<String, mpsc::Sender<TerminalUpdate>>::new(),
        ));

        let writer_thread = {
            thread::spawn(move || {
                while let Ok(message) = writer_rx.recv() {
                    if write_client_message(&mut writer, &message).is_err() {
                        break;
                    }
                }
            })
        };

        let pending_reader = pending.clone();
        let routes_reader = session_routes.clone();
        thread::spawn(move || {
            let _writer_thread = writer_thread;
            while let Ok(message) = read_server_message(&mut reader) {
                match message {
                    ServerMessage::Response {
                        request_id,
                        response,
                    } => {
                        if let Some(waiter) = lock(&pending_reader).remove(&request_id) {
                            let _ = waiter.send(response);
                        }
                    }
                    ServerMessage::Event(event) => {
                        dispatch_event(&routes_reader, event);
                    }
                }
            }

            for (_, waiter) in lock(&pending_reader).drain() {
                let _ = waiter.send(Err("daemon connection closed".to_owned()));
            }
            lock(&routes_reader).clear();
        });

        Ok(Self {
            writer_tx,
            pending,
            session_routes,
            next_request_id: Mutex::new(1),
            shutdown_stream: Mutex::new(Some(shutdown_stream)),
        })
    }

    /// Sends one request to the daemon and waits synchronously for the matching response.
    ///
    /// The call allocates a fresh request id, registers a one-shot response waiter, writes the request
    /// onto the writer thread's channel, and then blocks with a timeout.
    fn request(&self, request: DaemonRequest) -> Result<DaemonResponse, String> {
        let request_id = {
            let mut next = lock(&self.next_request_id);
            let request_id = *next;
            *next += 1;
            request_id
        };
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(request_id, tx);
        self.writer_tx
            .send(ClientMessage::Request {
                request_id,
                request,
            })
            .map_err(|_| "daemon writer channel disconnected".to_owned())?;
        rx.recv_timeout(DAEMON_REQUEST_TIMEOUT)
            .map_err(|_| "timed out waiting for daemon response".to_owned())?
    }
}

impl Drop for SocketTerminalDaemonClient {
    /// Shuts down the socket clone used to unblock the client background threads during drop.
    ///
    /// Without this explicit shutdown, threads waiting on socket I/O could linger until process exit.
    fn drop(&mut self) {
        // Shutting down the cloned socket side unblocks the reader/writer threads deterministically
        // so pending requests and routes drain to connection-closed errors instead of hanging.
        if let Some(stream) = lock(&self.shutdown_stream).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl TerminalDaemonClient for SocketTerminalDaemonClient {
    /// Issues a `ListSessions` request and asserts that the daemon answered with a session list.
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        match self.request(DaemonRequest::ListSessions)? {
            DaemonResponse::SessionList { sessions } => Ok(sessions),
            response => Err(format!("unexpected daemon list response: {response:?}")),
        }
    }

    /// Issues a `CreateSession` request and extracts the returned session id.
    fn create_session_with_env(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        env_overrides: &[(String, String)],
    ) -> Result<String, String> {
        match self.request(DaemonRequest::CreateSession {
            prefix: prefix.to_owned(),
            cwd: cwd.map(str::to_owned),
            env_overrides: env_overrides.to_vec(),
        })? {
            DaemonResponse::SessionCreated { session_id } => Ok(session_id),
            response => Err(format!("unexpected daemon create response: {response:?}")),
        }
    }

    fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String> {
        match self.request(DaemonRequest::ListOwnedTmuxSessions)? {
            DaemonResponse::OwnedTmuxSessionList { sessions } => Ok(sessions),
            response => Err(format!("unexpected owned tmux list response: {response:?}")),
        }
    }

    fn create_owned_tmux_session(
        &self,
        owner_agent_uid: &str,
        display_name: &str,
        cwd: Option<&str>,
        command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String> {
        match self.request(DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid: owner_agent_uid.to_owned(),
            display_name: display_name.to_owned(),
            cwd: cwd.map(str::to_owned),
            command: command.to_owned(),
        })? {
            DaemonResponse::OwnedTmuxSessionCreated { session } => Ok(session),
            response => Err(format!(
                "unexpected owned tmux create response: {response:?}"
            )),
        }
    }

    fn capture_owned_tmux_session(
        &self,
        session_uid: &str,
        lines: usize,
    ) -> Result<String, String> {
        match self.request(DaemonRequest::CaptureOwnedTmuxSession {
            session_uid: session_uid.to_owned(),
            lines,
        })? {
            DaemonResponse::OwnedTmuxSessionCapture {
                session_uid: _,
                text,
            } => Ok(text),
            response => Err(format!(
                "unexpected owned tmux capture response: {response:?}"
            )),
        }
    }

    fn kill_owned_tmux_session(&self, session_uid: &str) -> Result<(), String> {
        match self.request(DaemonRequest::KillOwnedTmuxSession {
            session_uid: session_uid.to_owned(),
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!("unexpected owned tmux kill response: {response:?}")),
        }
    }

    fn kill_owned_tmux_sessions_for_agent(&self, owner_agent_uid: &str) -> Result<(), String> {
        match self.request(DaemonRequest::KillOwnedTmuxSessionsForAgent {
            owner_agent_uid: owner_agent_uid.to_owned(),
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!(
                "unexpected owned tmux owner-kill response: {response:?}"
            )),
        }
    }

    /// Attaches to a daemon session, installs a local update route for it, and returns its current
    /// snapshot plus live update receiver.
    ///
    /// The client rejects duplicate attaches for the same session within one UI process because the
    /// routing table only supports one live receiver per session id.
    fn attach_session(&self, session_id: &str) -> Result<AttachedDaemonSession, String> {
        let (updates_tx, updates_rx) = mpsc::channel();
        {
            let mut routes = lock(&self.session_routes);
            // A single UI process owns at most one live route per session id. Re-attachment within
            // the same process must happen after the previous route is torn down.
            if routes.contains_key(session_id) {
                return Err(format!(
                    "daemon session `{session_id}` is already attached in this UI process"
                ));
            }
            routes.insert(session_id.to_owned(), updates_tx);
        }

        match self.request(DaemonRequest::AttachSession {
            session_id: session_id.to_owned(),
        }) {
            Ok(DaemonResponse::SessionAttached {
                session_id: attached_session_id,
                snapshot,
                ..
            }) => {
                if attached_session_id != session_id {
                    lock(&self.session_routes).remove(session_id);
                    return Err(format!(
                        "daemon attach returned mismatched session id `{attached_session_id}`"
                    ));
                }
                Ok(AttachedDaemonSession {
                    snapshot,
                    updates: updates_rx,
                })
            }
            Ok(response) => {
                lock(&self.session_routes).remove(session_id);
                Err(format!("unexpected daemon attach response: {response:?}"))
            }
            Err(error) => {
                lock(&self.session_routes).remove(session_id);
                Err(error)
            }
        }
    }

    /// Forwards one terminal command to the daemon and expects a plain acknowledgement.
    fn send_command(&self, session_id: &str, command: TerminalCommand) -> Result<(), String> {
        match self.request(DaemonRequest::SendCommand {
            session_id: session_id.to_owned(),
            command,
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!(
                "unexpected daemon send-command response: {response:?}"
            )),
        }
    }

    /// Forwards a resize request to the daemon and expects a plain acknowledgement.
    fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String> {
        match self.request(DaemonRequest::ResizeSession {
            session_id: session_id.to_owned(),
            cols,
            rows,
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!("unexpected daemon resize response: {response:?}")),
        }
    }

    /// Requests daemon-side session termination and drops any local update route for that session.
    fn kill_session(&self, session_id: &str) -> Result<(), String> {
        match self.request(DaemonRequest::KillSession {
            session_id: session_id.to_owned(),
        })? {
            DaemonResponse::Ack => {
                lock(&self.session_routes).remove(session_id);
                Ok(())
            }
            response => Err(format!("unexpected daemon kill response: {response:?}")),
        }
    }
}

/// Spawns a detached copy of the current executable in daemon mode bound to the chosen socket.
///
/// StdIO is nulled out because the daemon is meant to be background infrastructure, not an attached
/// child process of the UI.
fn spawn_daemon_subprocess(socket_path: &Path) -> Result<(), String> {
    let current_exe = env::current_exe().map_err(|error| {
        format!("failed to resolve current executable for daemon spawn: {error}")
    })?;
    let mut command = Command::new(current_exe);
    command
        .arg("daemon")
        .arg("--socket")
        .arg(socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to spawn daemon subprocess: {error}"))
}

/// Polls until the daemon socket accepts a connection or the timeout expires.
///
/// This is used immediately after spawning the daemon subprocess to bridge the race between process
/// spawn and socket readiness.
fn wait_for_connect(
    socket_path: &Path,
    timeout: Duration,
) -> Result<SocketTerminalDaemonClient, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match SocketTerminalDaemonClient::connect(socket_path) {
            Ok(client) => return Ok(client),
            Err(error) => {
                if std::time::Instant::now() >= deadline {
                    return Err(error);
                }
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
}

/// Routes one daemon event to the locally attached receiver for its session, if any.
///
/// If the receiver has gone away, the stale route is removed from the routing table.
fn dispatch_event(
    routes: &Arc<Mutex<HashMap<String, mpsc::Sender<TerminalUpdate>>>>,
    event: DaemonEvent,
) {
    match event {
        DaemonEvent::SessionUpdated {
            session_id, update, ..
        } => {
            let route = lock(routes).get(&session_id).cloned();
            if let Some(route) = route {
                if route.send(update).is_err() {
                    lock(routes).remove(&session_id);
                }
            }
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
