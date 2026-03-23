use super::protocol::{
    read_server_message, write_client_message, ClientMessage, DaemonEvent, DaemonRequest,
    DaemonResponse, DaemonSessionInfo, ServerMessage, DAEMON_PROTOCOL_VERSION,
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
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String>;
    fn create_session(&self, prefix: &str) -> Result<String, String>;
    fn attach_session(&self, session_id: &str) -> Result<AttachedDaemonSession, String>;
    fn send_command(&self, session_id: &str, command: TerminalCommand) -> Result<(), String>;
    #[allow(
        dead_code,
        reason = "protocol includes resize even before the UI drives it"
    )]
    fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String>;
    fn kill_session(&self, session_id: &str) -> Result<(), String>;
}

#[derive(Resource, Clone)]
pub(crate) struct TerminalDaemonClientResource {
    inner: Arc<dyn TerminalDaemonClient>,
}

impl TerminalDaemonClientResource {
    pub(crate) fn system() -> Result<Self, String> {
        Ok(Self {
            inner: Arc::new(SocketTerminalDaemonClient::connect_or_start_default()?),
        })
    }

    #[cfg(test)]
    pub(crate) fn from_client<T>(client: Arc<T>) -> Self
    where
        T: TerminalDaemonClient + 'static,
    {
        Self { inner: client }
    }

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

        let client = Self {
            writer_tx,
            pending,
            session_routes,
            next_request_id: Mutex::new(1),
            shutdown_stream: Mutex::new(Some(shutdown_stream)),
        };
        client.handshake()?;
        Ok(client)
    }

    fn handshake(&self) -> Result<(), String> {
        match self.request(DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        })? {
            DaemonResponse::HandshakeAck { version } if version == DAEMON_PROTOCOL_VERSION => {
                Ok(())
            }
            DaemonResponse::HandshakeAck { version } => Err(format!(
                "daemon protocol mismatch after handshake: client={} server={version}",
                DAEMON_PROTOCOL_VERSION
            )),
            response => Err(format!(
                "unexpected daemon handshake response: {response:?}"
            )),
        }
    }

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
    fn drop(&mut self) {
        if let Some(stream) = lock(&self.shutdown_stream).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl TerminalDaemonClient for SocketTerminalDaemonClient {
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        match self.request(DaemonRequest::ListSessions)? {
            DaemonResponse::SessionList { sessions } => Ok(sessions),
            response => Err(format!("unexpected daemon list response: {response:?}")),
        }
    }

    fn create_session(&self, prefix: &str) -> Result<String, String> {
        match self.request(DaemonRequest::CreateSession {
            prefix: prefix.to_owned(),
        })? {
            DaemonResponse::SessionCreated { session_id } => Ok(session_id),
            response => Err(format!("unexpected daemon create response: {response:?}")),
        }
    }

    fn attach_session(&self, session_id: &str) -> Result<AttachedDaemonSession, String> {
        let (updates_tx, updates_rx) = mpsc::channel();
        {
            let mut routes = lock(&self.session_routes);
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

pub(crate) fn resolve_daemon_socket_path() -> Option<PathBuf> {
    resolve_daemon_socket_path_with(
        env::var("NEOZEUS_DAEMON_SOCKET_PATH").ok().as_deref(),
        env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("USER").ok().as_deref(),
    )
}

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

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
