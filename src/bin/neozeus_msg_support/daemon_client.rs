use super::daemon_socket::resolve_daemon_socket_path;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

const DAEMON_CONNECT_RETRY_TIMEOUT: Duration = Duration::from_secs(2);
const DAEMON_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

type PendingResponses = HashMap<u64, mpsc::Sender<Result<DaemonResponse, String>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TerminalLifecycle {
    Running,
    Exited {
        code: Option<u32>,
        signal: Option<String>,
    },
    Disconnected,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalRuntimeState {
    pub(crate) status: String,
    pub(crate) lifecycle: TerminalLifecycle,
    pub(crate) last_error: Option<String>,
}

impl TerminalRuntimeState {
    #[cfg(test)]
    pub(crate) fn running(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            lifecycle: TerminalLifecycle::Running,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCommand {
    SendCommand(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DaemonSessionInfo {
    pub(crate) session_id: String,
    pub(crate) runtime: TerminalRuntimeState,
    pub(crate) revision: u64,
    pub(crate) created_order: u64,
}

#[derive(Clone, Debug, PartialEq)]
enum ClientMessage {
    Request {
        request_id: u64,
        request: DaemonRequest,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum DaemonRequest {
    ListSessions,
    SendCommand {
        session_id: String,
        command: TerminalCommand,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum ServerMessage {
    Response {
        request_id: u64,
        response: Result<DaemonResponse, String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum DaemonResponse {
    SessionList { sessions: Vec<DaemonSessionInfo> },
    Ack,
}

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

fn write_client_message(writer: &mut impl Write, message: &ClientMessage) -> Result<(), String> {
    let mut payload = Vec::new();
    encode_client_message(&mut payload, message);
    write_frame(writer, &payload)
}

fn read_server_message(reader: &mut impl Read) -> Result<ServerMessage, String> {
    let payload = read_frame(reader)?;
    let mut decoder = Decoder::new(&payload);
    let message = decode_server_message(&mut decoder)?;
    decoder.finish()?;
    Ok(message)
}

fn write_frame(writer: &mut impl Write, payload: &[u8]) -> Result<(), String> {
    let len = u32::try_from(payload.len()).map_err(|_| "protocol frame too large".to_owned())?;
    writer
        .write_all(&len.to_le_bytes())
        .map_err(|error| format!("failed to write frame length: {error}"))?;
    writer
        .write_all(payload)
        .map_err(|error| format!("failed to write frame payload: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("failed to flush frame payload: {error}"))
}

fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut len_buf = [0_u8; 4];
    reader
        .read_exact(&mut len_buf)
        .map_err(|error| format!("failed to read frame length: {error}"))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut payload = vec![0_u8; len];
    reader
        .read_exact(&mut payload)
        .map_err(|error| format!("failed to read frame payload: {error}"))?;
    Ok(payload)
}

fn encode_client_message(buffer: &mut Vec<u8>, message: &ClientMessage) {
    match message {
        ClientMessage::Request {
            request_id,
            request,
        } => {
            push_u8(buffer, 0);
            push_u64(buffer, *request_id);
            encode_request(buffer, request);
        }
    }
}

fn encode_request(buffer: &mut Vec<u8>, request: &DaemonRequest) {
    match request {
        DaemonRequest::ListSessions => push_u8(buffer, 1),
        DaemonRequest::SendCommand {
            session_id,
            command,
        } => {
            push_u8(buffer, 4);
            push_string(buffer, session_id);
            encode_command(buffer, command);
        }
    }
}

fn decode_server_message(decoder: &mut Decoder<'_>) -> Result<ServerMessage, String> {
    match decoder.read_u8()? {
        0 => Ok(ServerMessage::Response {
            request_id: decoder.read_u64()?,
            response: decode_result(decoder, decode_response)?,
        }),
        tag => Err(format!("unknown server message tag {tag}")),
    }
}

fn decode_response(decoder: &mut Decoder<'_>) -> Result<DaemonResponse, String> {
    match decoder.read_u8()? {
        1 => Ok(DaemonResponse::SessionList {
            sessions: decoder.read_vec(decode_session_info)?,
        }),
        4 => Ok(DaemonResponse::Ack),
        tag => Err(format!("unknown daemon response tag {tag}")),
    }
}

fn decode_session_info(decoder: &mut Decoder<'_>) -> Result<DaemonSessionInfo, String> {
    Ok(DaemonSessionInfo {
        session_id: decoder.read_string()?,
        runtime: decode_runtime_state(decoder)?,
        revision: decoder.read_u64()?,
        created_order: 0,
    })
}

fn encode_command(buffer: &mut Vec<u8>, command: &TerminalCommand) {
    match command {
        TerminalCommand::SendCommand(command) => {
            push_u8(buffer, 2);
            push_string(buffer, command);
        }
    }
}

fn decode_runtime_state(decoder: &mut Decoder<'_>) -> Result<TerminalRuntimeState, String> {
    Ok(TerminalRuntimeState {
        status: decoder.read_string()?,
        lifecycle: decode_lifecycle(decoder)?,
        last_error: decoder.read_option(|decoder| decoder.read_string())?,
    })
}

fn decode_lifecycle(decoder: &mut Decoder<'_>) -> Result<TerminalLifecycle, String> {
    match decoder.read_u8()? {
        0 => Ok(TerminalLifecycle::Running),
        1 => Ok(TerminalLifecycle::Exited {
            code: decoder.read_option(|decoder| decoder.read_u32())?,
            signal: decoder.read_option(|decoder| decoder.read_string())?,
        }),
        2 => Ok(TerminalLifecycle::Disconnected),
        3 => Ok(TerminalLifecycle::Failed),
        tag => Err(format!("unknown terminal lifecycle tag {tag}")),
    }
}

fn decode_result<T>(
    decoder: &mut Decoder<'_>,
    decode_ok: impl Fn(&mut Decoder<'_>) -> Result<T, String>,
) -> Result<Result<T, String>, String> {
    match decoder.read_u8()? {
        0 => Ok(Ok(decode_ok(decoder)?)),
        1 => Ok(Err(decoder.read_string()?)),
        tag => Err(format!("unknown result tag {tag}")),
    }
}

fn push_u8(buffer: &mut Vec<u8>, value: u8) {
    buffer.push(value);
}

fn push_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(buffer: &mut Vec<u8>, value: u64) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

fn push_string(buffer: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    push_u32(buffer, u32::try_from(bytes.len()).unwrap_or(u32::MAX));
    buffer.extend_from_slice(bytes);
}

struct Decoder<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn new(payload: &'a [u8]) -> Self {
        Self { payload, offset: 0 }
    }

    fn finish(&self) -> Result<(), String> {
        if self.offset == self.payload.len() {
            Ok(())
        } else {
            Err("trailing bytes after protocol message".to_owned())
        }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "protocol offset overflow".to_owned())?;
        if end > self.payload.len() {
            return Err("unexpected end of protocol payload".to_owned());
        }
        let slice = &self.payload[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u64(&mut self) -> Result<u64, String> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_string(&mut self) -> Result<String, String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_exact(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|error| format!("invalid utf-8 string: {error}"))
    }

    fn read_option<T>(
        &mut self,
        read_value: impl Fn(&mut Decoder<'_>) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(read_value(self)?)),
            value => Err(format!("invalid option tag {value}")),
        }
    }

    fn read_vec<T>(
        &mut self,
        read_value: impl Fn(&mut Decoder<'_>) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let len = self.read_u32()? as usize;
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(read_value(self)?);
        }
        Ok(items)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
