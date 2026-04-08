use std::{
    collections::HashMap,
    net::Shutdown,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

type PendingResponses<Response> = HashMap<u64, mpsc::Sender<Result<Response, String>>>;

type WriteMessage<ClientMessage> = fn(&mut UnixStream, &ClientMessage) -> Result<(), String>;
type ReadMessage<ServerMessage> = fn(&mut UnixStream) -> Result<ServerMessage, String>;
type HandleServerMessage<ServerMessage, Response> =
    Arc<dyn Fn(ServerMessage) -> Option<(u64, Result<Response, String>)> + Send + Sync>;
type BuildRequest<ClientMessage> = Arc<dyn Fn(u64) -> ClientMessage + Send + Sync>;

pub struct SocketRequestClientCore<ClientMessage, Response> {
    writer_tx: mpsc::Sender<ClientMessage>,
    pending: Arc<Mutex<PendingResponses<Response>>>,
    next_request_id: Mutex<u64>,
    shutdown_stream: Mutex<Option<UnixStream>>,
}

impl<ClientMessage, Response> SocketRequestClientCore<ClientMessage, Response>
where
    ClientMessage: Send + 'static,
    Response: Send + 'static,
{
    pub fn connect<ServerMessage>(
        socket_path: &Path,
        write_message: WriteMessage<ClientMessage>,
        read_message: ReadMessage<ServerMessage>,
        handle_server_message: HandleServerMessage<ServerMessage, Response>,
    ) -> Result<Self, String>
    where
        ServerMessage: Send + 'static,
    {
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
                if write_message(&mut writer, &message).is_err() {
                    break;
                }
            }
        });

        let pending_reader = pending.clone();
        thread::spawn(move || {
            let _writer_thread = writer_thread;
            while let Ok(message) = read_message(&mut reader) {
                if let Some((request_id, response)) = handle_server_message(message) {
                    if let Some(waiter) = lock(&pending_reader).remove(&request_id) {
                        let _ = waiter.send(response);
                    }
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

    pub fn request_with(
        &self,
        timeout: Duration,
        build_request: BuildRequest<ClientMessage>,
    ) -> Result<Response, String> {
        let request_id = {
            let mut next = lock(&self.next_request_id);
            let request_id = *next;
            *next += 1;
            request_id
        };
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(request_id, tx);
        if self.writer_tx.send(build_request(request_id)).is_err() {
            let _ = lock(&self.pending).remove(&request_id);
            return Err("daemon writer channel disconnected".to_owned());
        }
        match rx.recv_timeout(timeout) {
            Ok(response) => response,
            Err(_) => {
                let _ = lock(&self.pending).remove(&request_id);
                Err("timed out waiting for daemon response".to_owned())
            }
        }
    }
}

impl<ClientMessage, Response> Drop for SocketRequestClientCore<ClientMessage, Response> {
    fn drop(&mut self) {
        if let Some(stream) = lock(&self.shutdown_stream).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

pub fn spawn_daemon_subprocess(socket_path: &Path) -> Result<(), String> {
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

pub fn wait_for_connect<T, F>(
    socket_path: &Path,
    timeout: Duration,
    connect: F,
) -> Result<T, String>
where
    F: Fn(&Path) -> Result<T, String>,
{
    let deadline = Instant::now() + timeout;
    loop {
        match connect(socket_path) {
            Ok(client) => return Ok(client),
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn resolve_daemon_executable() -> Result<PathBuf, String> {
    resolve_daemon_executable_with(std::env::current_exe().ok())
}

fn resolve_daemon_executable_with(current_exe: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(current_exe) = current_exe {
        if let Some(parent) = current_exe.parent() {
            let sibling = parent.join("neozeus");
            if sibling.is_file() {
                return Ok(sibling);
            }
        }
        if current_exe.file_name().and_then(|name| name.to_str()) == Some("neozeus") {
            return Ok(current_exe);
        }
    }
    Ok(PathBuf::from("neozeus"))
}

#[cfg(test)]
mod tests {
    use super::{resolve_daemon_executable_with, SocketRequestClientCore};
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{mpsc, Arc, Mutex},
        time::Duration,
    };

    #[test]
    fn daemon_executable_falls_back_to_neozeus_for_helper_binaries() {
        let resolved = resolve_daemon_executable_with(Some(PathBuf::from("/tmp/bin/neozeus-msg")))
            .expect("helper fallback should resolve");
        assert_eq!(resolved, PathBuf::from("neozeus"));
    }

    #[test]
    fn daemon_executable_keeps_current_exe_when_already_main_binary() {
        let resolved = resolve_daemon_executable_with(Some(PathBuf::from("/tmp/bin/neozeus")))
            .expect("main binary should resolve");
        assert_eq!(resolved, PathBuf::from("/tmp/bin/neozeus"));
    }

    #[test]
    fn request_with_cleans_pending_entry_when_writer_channel_is_disconnected() {
        let (writer_tx, writer_rx) = mpsc::channel::<u64>();
        drop(writer_rx);
        let client: SocketRequestClientCore<u64, u64> = SocketRequestClientCore {
            writer_tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_request_id: Mutex::new(1),
            shutdown_stream: Mutex::new(None),
        };

        let error = client
            .request_with(Duration::from_secs(1), Arc::new(|request_id| request_id))
            .expect_err("disconnected writer should fail immediately");

        assert_eq!(error, "daemon writer channel disconnected");
        assert!(client.pending.lock().unwrap().is_empty());
    }
}
