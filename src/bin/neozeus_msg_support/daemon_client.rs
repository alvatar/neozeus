use neozeus::shared::{
    daemon_socket::resolve_daemon_socket_path,
    daemon_wire::{
        read_server_message, write_client_message, ClientMessage, DaemonRequest, DaemonResponse,
        ServerMessage,
    },
};
use std::{
    collections::HashMap,
    net::Shutdown,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

pub(crate) use neozeus::shared::daemon_wire::{DaemonSessionInfo, TerminalCommand};
#[cfg(test)]
pub(crate) use neozeus::shared::daemon_wire::{TerminalLifecycle, TerminalRuntimeState};

const DAEMON_CONNECT_RETRY_TIMEOUT: Duration = Duration::from_secs(2);
const DAEMON_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

type PendingResponses = HashMap<u64, mpsc::Sender<Result<DaemonResponse, String>>>;

pub(crate) trait DaemonMessenger {
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String>;
    fn send_command(&self, session_name: &str, command: TerminalCommand) -> Result<(), String>;
}

pub(crate) struct SocketDaemonMessenger {
    writer_tx: mpsc::Sender<ClientMessage>,
    pending: Arc<Mutex<PendingResponses>>,
    next_request_id: Mutex<u64>,
    shutdown_stream: Mutex<Option<UnixStream>>,
}

impl SocketDaemonMessenger {
    pub(crate) fn connect_or_start_default() -> Result<Self, String> {
        let socket_path = resolve_daemon_socket_path()
            .ok_or_else(|| "failed to resolve daemon socket path".to_owned())?;
        match Self::connect(&socket_path) {
            Ok(client) => Ok(client),
            Err(_) => {
                spawn_daemon_subprocess(&socket_path)?;
                wait_for_connect(&socket_path, DAEMON_CONNECT_RETRY_TIMEOUT)
            }
        }
    }

    fn connect(socket_path: &Path) -> Result<Self, String> {
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

        let writer_thread = thread::spawn(move || {
            while let Ok(message) = writer_rx.recv() {
                if write_client_message(&mut writer, &message).is_err() {
                    break;
                }
            }
        });

        let pending_reader = pending.clone();
        thread::spawn(move || {
            let _writer_thread = writer_thread;
            while let Ok(message) = read_server_message(&mut reader) {
                let ServerMessage::Response {
                    request_id,
                    response,
                } = message;
                if let Some(waiter) = lock(&pending_reader).remove(&request_id) {
                    let _ = waiter.send(response);
                }
            }

            for (_, waiter) in lock(&pending_reader).drain() {
                let _ = waiter.send(Err("daemon connection closed".to_owned()));
            }
        });

        Ok(Self {
            writer_tx,
            pending,
            next_request_id: Mutex::new(1),
            shutdown_stream: Mutex::new(Some(shutdown_stream)),
        })
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

impl Drop for SocketDaemonMessenger {
    fn drop(&mut self) {
        if let Some(stream) = lock(&self.shutdown_stream).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl DaemonMessenger for SocketDaemonMessenger {
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        match self.request(DaemonRequest::ListSessions)? {
            DaemonResponse::SessionList { sessions } => Ok(sessions),
            response => Err(format!("unexpected daemon list response: {response:?}")),
        }
    }

    fn send_command(&self, session_name: &str, command: TerminalCommand) -> Result<(), String> {
        match self.request(DaemonRequest::SendCommand {
            session_id: session_name.to_owned(),
            command,
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!("unexpected daemon send response: {response:?}")),
        }
    }
}

fn spawn_daemon_subprocess(socket_path: &Path) -> Result<(), String> {
    let daemon_executable = resolve_daemon_executable()?;
    Command::new(&daemon_executable)
        .arg("daemon")
        .arg("--socket")
        .arg(socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            format!(
                "failed to start daemon via {}: {error}",
                daemon_executable.display()
            )
        })?;
    Ok(())
}

fn resolve_daemon_executable() -> Result<PathBuf, String> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let sibling = parent.join("neozeus");
            if sibling.is_file() {
                return Ok(sibling);
            }
        }
    }
    Ok(PathBuf::from("neozeus"))
}

fn wait_for_connect(
    socket_path: &Path,
    timeout: Duration,
) -> Result<SocketDaemonMessenger, String> {
    let start = std::time::Instant::now();
    let mut last_error = None;
    while start.elapsed() < timeout {
        match SocketDaemonMessenger::connect(socket_path) {
            Ok(client) => return Ok(client),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        format!(
            "timed out connecting daemon socket {}",
            socket_path.display()
        )
    }))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
