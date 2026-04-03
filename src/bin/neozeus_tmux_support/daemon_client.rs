use neozeus::shared::daemon_wire::{
    read_server_message, write_client_message, ClientMessage, DaemonRequest, DaemonResponse,
    OwnedTmuxSessionInfo, ServerMessage,
};
use std::{
    collections::HashMap,
    net::Shutdown,
    os::unix::net::UnixStream,
    path::Path,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

const DAEMON_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

type PendingResponses = HashMap<u64, mpsc::Sender<Result<DaemonResponse, String>>>;

pub(crate) trait OwnedTmuxCreator {
    fn create_owned_tmux_session(
        &self,
        owner_agent_uid: &str,
        display_name: &str,
        cwd: Option<&str>,
        command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String>;
}

pub(crate) struct SocketOwnedTmuxClient {
    writer_tx: mpsc::Sender<ClientMessage>,
    pending: Arc<Mutex<PendingResponses>>,
    next_request_id: Mutex<u64>,
    shutdown_stream: Mutex<Option<UnixStream>>,
}

impl SocketOwnedTmuxClient {
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

impl Drop for SocketOwnedTmuxClient {
    fn drop(&mut self) {
        if let Some(stream) = lock(&self.shutdown_stream).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl OwnedTmuxCreator for SocketOwnedTmuxClient {
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
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
