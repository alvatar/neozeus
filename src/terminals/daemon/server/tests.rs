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
