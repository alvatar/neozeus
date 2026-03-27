use super::protocol::{
    read_server_message, write_client_message, ClientMessage, DaemonEvent, DaemonRequest,
    DaemonResponse, DaemonSessionInfo, ServerMessage,
};
use crate::terminals::{append_debug_log, TerminalCommand, TerminalSnapshot, TerminalUpdate};
use bevy::prelude::Resource;
use std::{
    collections::HashMap,
    env,
    net::Shutdown,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

const DAEMON_SOCKET_FILENAME: &str = "daemon.sock";
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
    /// Asks the daemon to create a new session id using the provided prefix.
    fn create_session(&self, prefix: &str) -> Result<String, String>;
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
    /// Builds the Bevy resource wrapper around the default socket-backed daemon client.
    ///
    /// Startup uses this to connect to an existing daemon or auto-start one if needed.
    pub(crate) fn system() -> Result<Self, String> {
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
    pub(crate) fn connect_or_start_default() -> Result<Self, String> {
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
    fn create_session(&self, prefix: &str) -> Result<String, String> {
        match self.request(DaemonRequest::CreateSession {
            prefix: prefix.to_owned(),
        })? {
            DaemonResponse::SessionCreated { session_id } => Ok(session_id),
            response => Err(format!("unexpected daemon create response: {response:?}")),
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

/// Resolves the daemon socket path from explicit override/runtime/home inputs.
///
/// The precedence is: explicit override path, then XDG runtime dir, then a per-user directory under
/// the system temp dir when only HOME is available.
pub(crate) fn resolve_daemon_socket_path_with(
    override_path: Option<&str>,
    xdg_runtime_dir: Option<&str>,
    home: Option<&str>,
    user: Option<&str>,
) -> Option<PathBuf> {
    if let Some(override_path) = override_path.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(override_path));
    }

    if let Some(xdg_runtime_dir) = xdg_runtime_dir.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg_runtime_dir)
                .join("neozeus")
                .join(DAEMON_SOCKET_FILENAME),
        );
    }

    let user = user.filter(|value| !value.is_empty()).unwrap_or("user");
    if home.is_some() {
        return Some(
            std::env::temp_dir()
                .join(format!("neozeus-{user}"))
                .join(DAEMON_SOCKET_FILENAME),
        );
    }

    None
}

/// Resolves the daemon socket path from the real process environment.
///
/// This thin wrapper exists so the path policy can be tested separately from environment access.
pub(crate) fn resolve_daemon_socket_path() -> Option<PathBuf> {
    resolve_daemon_socket_path_with(
        env::var("NEOZEUS_DAEMON_SOCKET_PATH").ok().as_deref(),
        env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("USER").ok().as_deref(),
    )
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
