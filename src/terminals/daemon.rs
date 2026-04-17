mod client;
mod owned_tmux;
mod protocol;
mod server;
mod session;
mod session_metrics;


pub(crate) use crate::shared::daemon_socket::resolve_daemon_socket_path;
#[cfg(test)]
pub(crate) use crate::shared::daemon_socket::resolve_daemon_socket_path_with;
pub(crate) use crate::shared::daemon_wire::DaemonSessionInfo;
pub(crate) use client::{AttachedDaemonSession, TerminalDaemonClientResource};
#[cfg(test)]
pub(crate) use client::{SocketTerminalDaemonClient, TerminalDaemonClient};
pub(crate) use owned_tmux::OwnedTmuxSessionInfo;
#[cfg(test)]
pub(crate) use protocol::{
    read_client_message, read_server_message, write_client_message, write_server_message,
    ClientMessage, DaemonEvent, DaemonRequest, ServerMessage,
};
pub(crate) use server::run_daemon_server;
#[cfg(test)]
pub(crate) use server::DaemonServerHandle;
#[cfg(test)]
pub(crate) use session::is_persistent_session_name;
pub(crate) use session::{PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminals::{
        TerminalCommand, TerminalLifecycle, TerminalRuntimeState, TerminalSurface, TerminalUpdate,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::{mpsc, Arc, OnceLock},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    /// Creates a unique temporary directory for daemon integration tests.
    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    /// Starts a dedicated daemon server for an integration test and returns both the handle and socket
    /// path.
    fn start_test_daemon(prefix: &str) -> (DaemonServerHandle, PathBuf) {
        let dir = temp_dir(prefix);
        let socket_path = dir.join("daemon.sock");
        let handle = DaemonServerHandle::start(socket_path.clone()).expect("daemon should start");
        (handle, socket_path)
    }

    /// Flattens a terminal surface into newline-separated text for daemon integration assertions.
    fn run_tmux(args: &[&str]) -> Result<String, String> {
        let output = Command::new("tmux")
            .args(args)
            .output()
            .map_err(|error| format!("tmux {:?} failed: {error}", args))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let detail = if !stderr.is_empty() { stderr } else { stdout };
            return Err(if detail.is_empty() {
                format!("tmux {:?} exited with status {}", args, output.status)
            } else {
                format!("tmux {:?} failed: {detail}", args)
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn kill_tmux_session_if_exists(session_name: &str) {
        let _ = run_tmux(&["kill-session", "-t", session_name]);
    }

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn test_target_dir(name: &str) -> PathBuf {
        repo_root().join(".tmpbuild").join(name)
    }

    fn build_test_binary(bin_name: &str, target_dir_name: &str) -> PathBuf {
        let target_dir = test_target_dir(target_dir_name);
        fs::create_dir_all(&target_dir).expect("test target dir should create");
        let output = Command::new("cargo")
            .current_dir(repo_root())
            .env("TMPDIR", repo_root().join(".tmpbuild"))
            .arg("build")
            .arg("--quiet")
            .arg("--bin")
            .arg(bin_name)
            .arg("--target-dir")
            .arg(&target_dir)
            .output()
            .expect("cargo build should start");
        if !output.status.success() {
            panic!(
                "failed to build {bin_name}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
        target_dir.join("debug").join(bin_name)
    }

    fn neozeus_tmux_binary() -> &'static PathBuf {
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| build_test_binary("neozeus-tmux", "tmux-helper-e2e-target"))
    }

    fn shell_quote(value: &str) -> String {
        if !value.is_empty()
            && value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
        {
            value.to_owned()
        } else {
            format!("'{}'", value.replace('\'', "'\\''"))
        }
    }

    fn wait_for_owned_tmux_session(
        client: &SocketTerminalDaemonClient,
        owner_agent_uid: &str,
    ) -> OwnedTmuxSessionInfo {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(sessions) = client.list_owned_tmux_sessions() {
                if let Some(session) = sessions
                    .into_iter()
                    .find(|session| session.owner_agent_uid == owner_agent_uid)
                {
                    return session;
                }
            }
            if std::time::Instant::now() >= deadline {
                panic!("timed out waiting for owned tmux session for {owner_agent_uid}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_for_owned_tmux_capture_containing(
        client: &SocketTerminalDaemonClient,
        session_uid: &str,
        needle: &str,
    ) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(text) = client.capture_owned_tmux_session(session_uid, 200) {
                if text.contains(needle) {
                    return text;
                }
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "timed out waiting for owned tmux capture for {session_uid} to contain {needle}"
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn unique_tmux_name(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        format!("{prefix}-{nanos}")
    }

    fn surface_to_text(surface: &TerminalSurface) -> String {
        let mut text = String::new();
        for y in 0..surface.rows {
            if y > 0 {
                text.push('\n');
            }
            for x in 0..surface.cols {
                text.push_str(&surface.cell(x, y).content.to_owned_string());
            }
        }
        text
    }

    /// Waits until the daemon update stream yields a surface whose rendered text contains the requested
    /// substring.
    fn wait_for_surface_containing(
        updates: &mpsc::Receiver<TerminalUpdate>,
        needle: &str,
    ) -> TerminalSurface {
        wait_for_surface_containing_with_timeout(updates, needle, Duration::from_secs(3))
    }

    fn wait_for_surface_containing_with_timeout(
        updates: &mpsc::Receiver<TerminalUpdate>,
        needle: &str,
        timeout: Duration,
    ) -> TerminalSurface {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("timed out waiting for daemon update");
            let update = updates
                .recv_timeout(remaining)
                .expect("timed out waiting for daemon update");
            let surface = match update {
                TerminalUpdate::Frame(frame) => frame.surface,
                TerminalUpdate::Status {
                    surface: Some(surface),
                    ..
                } => surface,
                TerminalUpdate::Status { .. } => continue,
            };
            if surface_to_text(&surface).contains(needle) {
                return surface;
            }
        }
    }

    /// Waits until the daemon update stream yields a runtime state whose lifecycle matches the supplied
    /// predicate.
    fn wait_for_lifecycle(
        updates: &mpsc::Receiver<TerminalUpdate>,
        predicate: impl Fn(&TerminalLifecycle) -> bool,
    ) -> TerminalRuntimeState {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("timed out waiting for daemon lifecycle update");
            let update = updates
                .recv_timeout(remaining)
                .expect("timed out waiting for daemon lifecycle update");
            let runtime = match update {
                TerminalUpdate::Frame(frame) => frame.runtime,
                TerminalUpdate::Status { runtime, .. } => runtime,
            };
            if predicate(&runtime.lifecycle) {
                return runtime;
            }
        }
    }

    /// Waits until the daemon update stream yields a surface with the requested dimensions.
    fn wait_for_surface_dimensions(
        updates: &mpsc::Receiver<TerminalUpdate>,
        cols: usize,
        rows: usize,
    ) -> TerminalSurface {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("timed out waiting for resized surface");
            let update = updates
                .recv_timeout(remaining)
                .expect("timed out waiting for resized surface");
            let surface = match update {
                TerminalUpdate::Frame(frame) => frame.surface,
                TerminalUpdate::Status {
                    surface: Some(surface),
                    ..
                } => surface,
                TerminalUpdate::Status { .. } => continue,
            };
            if surface.cols == cols && surface.rows == rows {
                return surface;
            }
        }
    }

    /// Waits until a file appears and returns its trimmed contents.
    fn wait_for_file_text(path: &std::path::Path) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(text) = fs::read_to_string(path) {
                let text = text.trim().to_owned();
                if !text.is_empty() {
                    return text;
                }
            }
            if std::time::Instant::now() >= deadline {
                panic!("timed out waiting for file {}", path.display());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn process_exists(pid: u32) -> bool {
        let proc_dir = format!("/proc/{pid}");
        if !std::path::Path::new(&proc_dir).exists() {
            return false;
        }
        let stat_path = format!("{proc_dir}/stat");
        match fs::read_to_string(stat_path) {
            Ok(stat) => stat
                .rsplit_once(") ")
                .and_then(|(_, suffix)| suffix.chars().next())
                .is_none_or(|state| state != 'Z'),
            Err(_) => true,
        }
    }

    /// Verifies daemon socket-path resolution precedence: explicit override, then XDG runtime, then the
    /// per-user temp-dir fallback.
    #[test]
    fn daemon_socket_path_prefers_override_then_xdg_runtime_then_tmp_user() {
        let override_path = resolve_daemon_socket_path_with(
            Some("/tmp/neozeus-test/daemon.sock"),
            None,
            Some("/run/user/1000"),
            Some("/home/alvatar"),
            Some("oracle"),
        )
        .expect("override path should resolve");
        assert_eq!(
            override_path,
            PathBuf::from("/tmp/neozeus-test/daemon.sock")
        );

        let path = resolve_daemon_socket_path_with(
            None,
            None,
            Some("/run/user/1000"),
            Some("/home/alvatar"),
            Some("oracle"),
        )
        .expect("xdg runtime path should resolve");
        assert_eq!(path, PathBuf::from("/run/user/1000/neozeus/daemon.v2.sock"));

        let fallback =
            resolve_daemon_socket_path_with(None, None, None, Some("/home/alvatar"), Some("oracle"))
                .expect("tmp fallback should resolve");
        assert!(fallback.ends_with("neozeus-oracle/daemon.v2.sock"));
    }

    /// Verifies that the default daemon socket path is versioned so new clients do not attach to an
    /// older incompatible daemon left running from a previous build.
    #[test]
    fn daemon_socket_path_uses_versioned_filename() {
        let resolved = resolve_daemon_socket_path_with(
            None,
            None,
            Some("/run/user/1000"),
            Some("/home/alvatar"),
            Some("oracle"),
        )
        .expect("socket path should resolve");
        assert_eq!(
            resolved.file_name().and_then(|name| name.to_str()),
            Some("daemon.v2.sock")
        );
        assert_ne!(
            resolved,
            PathBuf::from("/run/user/1000/neozeus/daemon.sock")
        );
    }

    /// Verifies representative client and server daemon protocol messages round-trip through the binary
    /// wire format unchanged.
    #[test]
    fn daemon_protocol_roundtrip_preserves_terminal_messages() {
        let message = ClientMessage::Request {
            request_id: 7,
            request: DaemonRequest::SendCommand {
                session_id: "neozeus-session-7".into(),
                command: TerminalCommand::SendCommand("printf 'hi'".into()),
            },
        };
        let mut bytes = Vec::new();
        write_client_message(&mut bytes, &message).expect("client message should encode");
        let decoded = read_client_message(&mut bytes.as_slice()).expect("client message should decode");
        assert_eq!(decoded, message);

        let mut surface = TerminalSurface::new(3, 1);
        surface.set_text_cell(0, 0, "h");
        surface.set_text_cell(1, 0, "i");
        let response = ServerMessage::Event(DaemonEvent::SessionUpdated {
            session_id: "neozeus-session-7".into(),
            update: TerminalUpdate::Status {
                runtime: TerminalRuntimeState::running("daemon"),
                surface: Some(surface.clone()),
            },
            revision: 9,
        });
        let mut server_bytes = Vec::new();
        write_server_message(&mut server_bytes, &response).expect("server message should encode");
        let decoded =
            read_server_message(&mut server_bytes.as_slice()).expect("server message should decode");
        assert_eq!(decoded, response);
    }

    /// Verifies that daemon startup replaces an orphaned stale socket file and still accepts client
    /// connections afterwards.
    #[test]
    fn daemon_server_cleans_up_stale_socket_file() {
        let dir = temp_dir("neozeus-daemon-stale-socket");
        let socket_path = dir.join("daemon.sock");
        fs::write(&socket_path, b"stale").expect("failed to write stale socket file");

        let _server =
            DaemonServerHandle::start(socket_path.clone()).expect("server should replace stale socket");
        let _client = SocketTerminalDaemonClient::connect(&socket_path)
            .expect("client should connect after stale cleanup");
    }

    /// End-to-end daemon integration test covering create, list, attach, streamed output, and explicit
    /// kill removal.
    #[test]
    fn daemon_create_attach_command_output_and_kill_roundtrip() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-roundtrip");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");
        let sessions = client.list_sessions().expect("daemon sessions should list");
        assert!(sessions
            .iter()
            .any(|session| session.session_id == session_id));

        let attached = client
            .attach_session(&session_id)
            .expect("daemon session should attach");
        assert!(attached.snapshot.surface.is_some());

        client
            .send_command(
                &session_id,
                TerminalCommand::SendCommand("printf 'neozeus-daemon-ok'".into()),
            )
            .expect("daemon command should send");
        let surface = wait_for_surface_containing(&attached.updates, "neozeus-daemon-ok");
        assert!(surface_to_text(&surface).contains("neozeus-daemon-ok"));

        client
            .kill_session(&session_id)
            .expect("daemon session should kill");
        let sessions = client
            .list_sessions()
            .expect("daemon sessions should relist");
        assert!(!sessions
            .iter()
            .any(|session| session.session_id == session_id));
    }

    /// Verifies that daemon-created sessions expose explicit per-session env overrides to the shell.
    #[test]
    fn daemon_create_session_exposes_env_overrides_to_shell() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-env");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session_with_env(
                PERSISTENT_SESSION_PREFIX,
                None,
                &[
                    ("NEOZEUS_AGENT_UID".into(), "agent-uid-test".into()),
                    ("NEOZEUS_AGENT_LABEL".into(), "AGENT-X".into()),
                ],
            )
            .expect("daemon session should be created");
        let attached = client
            .attach_session(&session_id)
            .expect("daemon session should attach");

        client
            .send_command(
                &session_id,
                TerminalCommand::SendCommand(
                    "printf 'env:%s|%s' \"$NEOZEUS_AGENT_UID\" \"$NEOZEUS_AGENT_LABEL\"".into(),
                ),
            )
            .expect("env command should send");
        let surface = wait_for_surface_containing(&attached.updates, "env:agent-uid-test|AGENT-X");
        assert!(surface_to_text(&surface).contains("env:agent-uid-test|AGENT-X"));
    }

    #[test]
    fn daemon_list_sessions_exposes_live_session_metrics() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-session-metrics");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session_with_env(PERSISTENT_SESSION_PREFIX, None, &[])
            .expect("daemon session should be created");

        let first = client
            .list_sessions()
            .expect("sessions should list")
            .into_iter()
            .find(|session| session.session_id == session_id)
            .expect("created session should list");
        assert!(first.metrics.ram_bytes.is_some());

        std::thread::sleep(Duration::from_millis(25));

        let second = client
            .list_sessions()
            .expect("sessions should list")
            .into_iter()
            .find(|session| session.session_id == session_id)
            .expect("created session should list");
        assert!(second.metrics.cpu_pct_milli.is_some());
        assert!(second.metrics.net_rx_bytes_per_sec.is_some());
        assert!(second.metrics.net_tx_bytes_per_sec.is_some());
    }

    #[test]
    fn daemon_list_sessions_exposes_agent_metadata() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-session-metadata");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session_with_env(
                PERSISTENT_SESSION_PREFIX,
                None,
                &[
                    ("NEOZEUS_AGENT_UID".into(), "agent-uid-test".into()),
                    ("NEOZEUS_AGENT_LABEL".into(), "AGENT-X".into()),
                    ("NEOZEUS_AGENT_KIND".into(), "claude".into()),
                ],
            )
            .expect("daemon session should be created");

        let session = client
            .list_sessions()
            .expect("sessions should list")
            .into_iter()
            .find(|session| session.session_id == session_id)
            .expect("created session should list");
        assert_eq!(
            session.metadata.agent_uid.as_deref(),
            Some("agent-uid-test")
        );
        assert_eq!(session.metadata.agent_label.as_deref(), Some("AGENT-X"));
        assert_eq!(
            session.metadata.agent_kind,
            Some(crate::shared::daemon_wire::DaemonAgentKind::Claude)
        );
    }

    #[test]
    fn daemon_update_session_metadata_label_updates_live_session_list() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-session-metadata-rename");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session_with_env(
                PERSISTENT_SESSION_PREFIX,
                None,
                &[("NEOZEUS_AGENT_LABEL".into(), "ALPHA".into())],
            )
            .expect("daemon session should be created");

        client
            .update_session_metadata(
                &session_id,
                &crate::shared::daemon_wire::DaemonSessionMetadata {
                    agent_uid: None,
                    agent_label: Some("BETA".into()),
                    agent_kind: None,
                },
            )
            .expect("metadata update should succeed");
        let session = client
            .list_sessions()
            .expect("sessions should list")
            .into_iter()
            .find(|session| session.session_id == session_id)
            .expect("created session should list");
        assert_eq!(session.metadata.agent_label.as_deref(), Some("BETA"));
    }

    /// Verifies that ordinary daemon sessions do not receive fake agent owner env by default.
    #[test]
    fn daemon_create_session_without_env_overrides_leaves_agent_env_unset() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-env-empty");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");
        let attached = client
            .attach_session(&session_id)
            .expect("daemon session should attach");

        client
            .send_command(
                &session_id,
                TerminalCommand::SendCommand("printf 'env:%s' \"${NEOZEUS_AGENT_UID-unset}\"".into()),
            )
            .expect("env command should send");
        let surface = wait_for_surface_containing(&attached.updates, "env:unset");
        assert!(surface_to_text(&surface).contains("env:unset"));
    }

    /// Verifies that owned tmux sessions are created, stamped, capturable, and explicitly killable.
    #[test]
    fn daemon_owned_tmux_create_list_capture_and_kill_roundtrip() {
        let (_server, socket_path) = start_test_daemon("neozeus-owned-tmux-roundtrip");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let cwd = temp_dir("neozeus-owned-tmux-cwd");
        let session = client
            .create_owned_tmux_session(
                "agent-uid-roundtrip",
                "BUILD",
                Some(cwd.to_str().expect("cwd should be utf-8")),
                "printf 'owned-tmux-roundtrip'",
            )
            .expect("owned tmux session should create");
        assert!(session.tmux_name.starts_with("neozeus-tmux-"));
        assert!(!session.tmux_name.contains(' '));

        let listed = client
            .list_owned_tmux_sessions()
            .expect("owned tmux sessions should list");
        let listed_session = listed
            .iter()
            .find(|candidate| candidate.session_uid == session.session_uid)
            .expect("created owned tmux session should list");
        assert_eq!(listed_session.owner_agent_uid, "agent-uid-roundtrip");
        assert_eq!(listed_session.display_name, "BUILD");
        assert_eq!(listed_session.cwd, cwd.to_string_lossy());

        let backend = run_tmux(&[
            "show-options",
            "-t",
            session.tmux_name.as_str(),
            "-qv",
            "@neozeus_backend",
        ])
        .expect("backend option should read");
        assert_eq!(backend.trim(), "agent-owned-tmux");
        let stamped_uid = run_tmux(&[
            "show-options",
            "-t",
            session.tmux_name.as_str(),
            "-qv",
            "@neozeus_id",
        ])
        .expect("session uid option should read");
        assert_eq!(stamped_uid.trim(), session.session_uid);

        let capture = client
            .capture_owned_tmux_session(&session.session_uid, 80)
            .expect("owned tmux capture should succeed");
        assert!(capture.contains("owned-tmux-roundtrip"));

        client
            .kill_owned_tmux_session(&session.session_uid)
            .expect("owned tmux kill should succeed");
        let listed = client
            .list_owned_tmux_sessions()
            .expect("owned tmux sessions should relist");
        assert!(!listed
            .iter()
            .any(|candidate| candidate.session_uid == session.session_uid));
    }

    /// Verifies that owned tmux kill does not return until the child pane process is actually gone.
    #[test]
    fn daemon_owned_tmux_kill_waits_for_child_process_exit() {
        let (_server, socket_path) = start_test_daemon("neozeus-owned-tmux-kill-wait");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let pid_file = temp_dir("neozeus-owned-tmux-kill-pid").join("tmux-child.pid");
        let session = client
            .create_owned_tmux_session(
                "agent-uid-kill-wait",
                "BUILD",
                None,
                &format!(
                    "exec sh -c {}",
                    shell_quote(&format!(
                        "echo $$ > {}; trap '' HUP; sleep 60",
                        pid_file.display()
                    ))
                ),
            )
            .expect("owned tmux session should create");

        let pid_text = wait_for_file_text(&pid_file);
        let pid: u32 = pid_text.parse().expect("tmux child pid should parse");
        assert!(
            process_exists(pid),
            "tmux child pid {pid} should exist before kill"
        );

        client
            .kill_owned_tmux_session(&session.session_uid)
            .expect("owned tmux kill should succeed");

        assert!(
            !process_exists(pid),
            "tmux child pid {pid} should be gone after kill_owned_tmux_session returns"
        );
    }

    /// Verifies that daemon restart rediscovers owned tmux sessions from tmux metadata.
    #[test]
    fn daemon_owned_tmux_sessions_survive_daemon_restart_and_rediscover() {
        let dir = temp_dir("neozeus-owned-tmux-recover");
        let socket_path = dir.join("daemon.sock");
        let session = {
            let _server = DaemonServerHandle::start(socket_path.clone()).expect("daemon should start");
            let client = SocketTerminalDaemonClient::connect(&socket_path)
                .expect("daemon client should connect");
            client
                .create_owned_tmux_session(
                    "agent-uid-recover",
                    "RECOVER",
                    None,
                    "printf 'recover-owned-tmux'",
                )
                .expect("owned tmux session should create")
        };

        let _server = DaemonServerHandle::start(socket_path.clone()).expect("daemon should restart");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should reconnect");
        let listed = client
            .list_owned_tmux_sessions()
            .expect("owned tmux sessions should list after restart");
        assert!(listed.iter().any(|candidate| {
            candidate.session_uid == session.session_uid && candidate.tmux_name == session.tmux_name
        }));

        client
            .kill_owned_tmux_session(&session.session_uid)
            .expect("owned tmux cleanup should succeed");
    }

    /// Verifies that discovery ignores arbitrary unstamped tmux sessions.
    #[test]
    fn daemon_owned_tmux_discovery_ignores_unstamped_tmux_sessions() {
        let raw_tmux_name = unique_tmux_name("neozeus-raw-tmux");
        run_tmux(&[
            "new-session",
            "-d",
            "-s",
            raw_tmux_name.as_str(),
            "exec zsh -il",
        ])
        .expect("raw tmux session should create");

        let (_server, socket_path) = start_test_daemon("neozeus-owned-tmux-ignore");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let listed = client
            .list_owned_tmux_sessions()
            .expect("owned tmux sessions should list");
        assert!(!listed
            .iter()
            .any(|candidate| candidate.tmux_name == raw_tmux_name));

        kill_tmux_session_if_exists(&raw_tmux_name);
    }

    /// Verifies that owner-wide tmux kill reuses the hardened child-exit semantics and does not return
    /// while matching pane processes are still alive.
    #[test]
    fn daemon_owned_tmux_kill_for_owner_uid_waits_for_child_process_exit() {
        let (_server, socket_path) = start_test_daemon("neozeus-owned-tmux-owner-kill-wait");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let pid_file = temp_dir("neozeus-owned-tmux-owner-kill-pid").join("tmux-child.pid");
        let owner_a = client
            .create_owned_tmux_session(
                "agent-owner-a",
                "A-1",
                None,
                &format!(
                    "exec sh -c {}",
                    shell_quote(&format!(
                        "echo $$ > {}; trap '' HUP; sleep 60",
                        pid_file.display()
                    ))
                ),
            )
            .expect("owned tmux session should create");
        let owner_b = client
            .create_owned_tmux_session("agent-owner-b", "B-1", None, "printf b1")
            .expect("other owner session should create");

        let pid_text = wait_for_file_text(&pid_file);
        let pid: u32 = pid_text.parse().expect("tmux child pid should parse");
        assert!(
            process_exists(pid),
            "tmux child pid {pid} should exist before owner kill"
        );

        client
            .kill_owned_tmux_sessions_for_agent("agent-owner-a")
            .expect("owner kill should succeed");

        assert!(
            !process_exists(pid),
            "tmux child pid {pid} should be gone after kill_owned_tmux_sessions_for_agent returns"
        );
        let listed = client
            .list_owned_tmux_sessions()
            .expect("owned tmux sessions should relist");
        assert!(!listed
            .iter()
            .any(|candidate| candidate.session_uid == owner_a.session_uid));
        assert!(listed
            .iter()
            .any(|candidate| candidate.session_uid == owner_b.session_uid));

        client
            .kill_owned_tmux_session(&owner_b.session_uid)
            .expect("owned tmux cleanup should succeed");
    }

    /// Verifies that killing by owner agent uid removes only that owner's tmux child sessions.
    #[test]
    fn daemon_owned_tmux_kill_for_owner_uid_is_scoped() {
        let (_server, socket_path) = start_test_daemon("neozeus-owned-tmux-owner-kill");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let owner_a_one = client
            .create_owned_tmux_session("agent-owner-a", "A-1", None, "printf a1")
            .expect("first owned tmux should create");
        let owner_a_two = client
            .create_owned_tmux_session("agent-owner-a", "A-2", None, "printf a2")
            .expect("second owned tmux should create");
        let owner_b = client
            .create_owned_tmux_session("agent-owner-b", "B-1", None, "printf b1")
            .expect("third owned tmux should create");

        client
            .kill_owned_tmux_sessions_for_agent("agent-owner-a")
            .expect("owner kill should succeed");

        let listed = client
            .list_owned_tmux_sessions()
            .expect("owned tmux sessions should relist");
        assert!(!listed
            .iter()
            .any(|candidate| candidate.session_uid == owner_a_one.session_uid));
        assert!(!listed
            .iter()
            .any(|candidate| candidate.session_uid == owner_a_two.session_uid));
        assert!(listed
            .iter()
            .any(|candidate| candidate.session_uid == owner_b.session_uid));

        client
            .kill_owned_tmux_session(&owner_b.session_uid)
            .expect("owned tmux cleanup should succeed");
    }

    /// Verifies that rediscovery keys owned tmux sessions by stable uid rather than tmux session name.
    #[test]
    fn daemon_owned_tmux_rediscovery_tracks_stable_uid_across_tmux_rename() {
        let dir = temp_dir("neozeus-owned-tmux-rename");
        let socket_path = dir.join("daemon.sock");
        let session = {
            let _server = DaemonServerHandle::start(socket_path.clone()).expect("daemon should start");
            let client = SocketTerminalDaemonClient::connect(&socket_path)
                .expect("daemon client should connect");
            client
                .create_owned_tmux_session(
                    "agent-uid-rename",
                    "RENAMED",
                    None,
                    "printf 'rename-owned-tmux'",
                )
                .expect("owned tmux session should create")
        };
        let renamed_tmux_name = unique_tmux_name("neozeus-owned-renamed");
        run_tmux(&[
            "rename-session",
            "-t",
            session.tmux_name.as_str(),
            renamed_tmux_name.as_str(),
        ])
        .expect("tmux session should rename");

        let _server = DaemonServerHandle::start(socket_path.clone()).expect("daemon should restart");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should reconnect");
        let listed = client
            .list_owned_tmux_sessions()
            .expect("owned tmux sessions should list after rename");
        let renamed = listed
            .iter()
            .find(|candidate| candidate.session_uid == session.session_uid)
            .expect("renamed session should rediscover by stable uid");
        assert_eq!(renamed.tmux_name, renamed_tmux_name);

        client
            .kill_owned_tmux_session(&session.session_uid)
            .expect("owned tmux cleanup should succeed");
    }

    /// Verifies the true end-to-end helper path: a real agent shell with injected ownership env runs
    /// `neozeus-tmux`, which creates a daemon-visible tmux child and yields capturable output.
    #[test]
    fn daemon_owned_tmux_helper_runs_inside_real_agent_shell() {
        let (_server, socket_path) = start_test_daemon("neozeus-owned-tmux-helper-shell");
        let client = Arc::new(
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect"),
        );
        let runtime_spawner = crate::terminals::TerminalRuntimeSpawner::for_tests(
            TerminalDaemonClientResource::from_client(client.clone()),
        );
        let owner_agent_uid = unique_tmux_name("agent-uid-helper");
        let helper_output_marker = unique_tmux_name("helper-output");
        let child_cwd = temp_dir("neozeus-owned-helper-child-cwd");
        let parent_cwd = repo_root().to_string_lossy().into_owned();
        let session_id = runtime_spawner
            .create_shell_session_with_cwd_and_env(
                "neozeus-agent-shell-",
                Some(&parent_cwd),
                &[
                    ("NEOZEUS_AGENT_UID".into(), owner_agent_uid.clone()),
                    (
                        "NEOZEUS_DAEMON_SOCKET".into(),
                        socket_path.to_string_lossy().into_owned(),
                    ),
                    ("NEOZEUS_AGENT_LABEL".into(), "ALPHA".into()),
                    ("NEOZEUS_AGENT_KIND".into(), "terminal".into()),
                ],
            )
            .expect("agent shell should create");
        let attached = client
            .attach_session(&session_id)
            .expect("agent shell should attach");
        let helper_command = format!(
            "cd {} && {} run --name BUILD --cwd {} -- bash -lc {}",
            shell_quote(&parent_cwd),
            shell_quote(&neozeus_tmux_binary().display().to_string()),
            shell_quote(&child_cwd.display().to_string()),
            shell_quote(&format!("printf '{helper_output_marker}'; pwd; sleep 60")),
        );
        client
            .send_command(&session_id, TerminalCommand::SendCommand(helper_command))
            .expect("helper command should send");

        let parent_surface = wait_for_surface_containing_with_timeout(
            &attached.updates,
            "tmux attach -t",
            Duration::from_secs(10),
        );
        assert!(surface_to_text(&parent_surface).contains("tmux attach -t"));

        let session = wait_for_owned_tmux_session(&client, &owner_agent_uid);
        assert_eq!(session.display_name, "BUILD");
        assert_eq!(session.cwd, child_cwd.display().to_string());
        let capture = wait_for_owned_tmux_capture_containing(
            &client,
            &session.session_uid,
            &helper_output_marker,
        );
        assert!(capture.contains(&helper_output_marker));

        client
            .kill_owned_tmux_session(&session.session_uid)
            .expect("owned tmux cleanup should succeed");
        client
            .kill_session(&session_id)
            .expect("parent shell cleanup should succeed");
    }

    /// Verifies that helper-created tmux children survive daemon restart and rediscover with the same
    /// stable uid and captured content.
    #[test]
    fn daemon_owned_tmux_helper_created_session_survives_daemon_restart() {
        let dir = temp_dir("neozeus-owned-tmux-helper-restart");
        let socket_path = dir.join("daemon.sock");
        let server = DaemonServerHandle::start(socket_path.clone()).expect("daemon should start");
        let client = Arc::new(
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect"),
        );
        let runtime_spawner = crate::terminals::TerminalRuntimeSpawner::for_tests(
            TerminalDaemonClientResource::from_client(client.clone()),
        );
        let owner_agent_uid = unique_tmux_name("agent-uid-restart");
        let helper_output_marker = unique_tmux_name("helper-restart-output");
        let child_cwd = temp_dir("neozeus-owned-helper-restart-cwd");
        let parent_cwd = repo_root().to_string_lossy().into_owned();
        let session_id = runtime_spawner
            .create_shell_session_with_cwd_and_env(
                "neozeus-agent-shell-",
                Some(&parent_cwd),
                &[
                    ("NEOZEUS_AGENT_UID".into(), owner_agent_uid.clone()),
                    (
                        "NEOZEUS_DAEMON_SOCKET".into(),
                        socket_path.to_string_lossy().into_owned(),
                    ),
                    ("NEOZEUS_AGENT_LABEL".into(), "BETA".into()),
                    ("NEOZEUS_AGENT_KIND".into(), "terminal".into()),
                ],
            )
            .expect("agent shell should create");
        let helper_command = format!(
            "cd {} && {} run --name RESTART --cwd {} -- bash -lc {}",
            shell_quote(&parent_cwd),
            shell_quote(&neozeus_tmux_binary().display().to_string()),
            shell_quote(&child_cwd.display().to_string()),
            shell_quote(&format!("printf '{helper_output_marker}'; pwd; sleep 60")),
        );
        client
            .send_command(&session_id, TerminalCommand::SendCommand(helper_command))
            .expect("helper command should send");

        let created = wait_for_owned_tmux_session(&client, &owner_agent_uid);
        let created_capture = wait_for_owned_tmux_capture_containing(
            &client,
            &created.session_uid,
            &helper_output_marker,
        );
        assert!(created_capture.contains(&helper_output_marker));

        client
            .kill_session(&session_id)
            .expect("parent shell cleanup should succeed");
        drop(server);

        let restarted_server =
            DaemonServerHandle::start(socket_path.clone()).expect("daemon should restart");
        let restarted_client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should reconnect");
        let rebuilt = wait_for_owned_tmux_session(&restarted_client, &owner_agent_uid);
        assert_eq!(rebuilt.session_uid, created.session_uid);
        assert_eq!(rebuilt.display_name, "RESTART");
        assert_eq!(rebuilt.cwd, child_cwd.display().to_string());
        let rebuilt_capture = wait_for_owned_tmux_capture_containing(
            &restarted_client,
            &rebuilt.session_uid,
            &helper_output_marker,
        );
        assert!(rebuilt_capture.contains(&helper_output_marker));

        restarted_client
            .kill_owned_tmux_session(&rebuilt.session_uid)
            .expect("owned tmux cleanup should succeed");
        drop(restarted_server);
    }

    /// Verifies that daemon-created sessions honor the requested initial working directory.
    #[test]
    fn daemon_create_session_honors_requested_cwd() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-cwd");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let cwd = temp_dir("neozeus-daemon-session-cwd");
        let session_id = client
            .create_session(
                PERSISTENT_SESSION_PREFIX,
                Some(cwd.to_str().expect("cwd should be utf-8")),
            )
            .expect("daemon session should be created");
        let attached = client
            .attach_session(&session_id)
            .expect("daemon session should attach");

        client
            .send_command(&session_id, TerminalCommand::SendCommand("pwd".into()))
            .expect("pwd command should send");
        let surface = wait_for_surface_containing(&attached.updates, cwd.to_str().unwrap());
        assert!(surface_to_text(&surface).contains(cwd.to_str().unwrap()));
    }

    /// Verifies that daemon sessions are server-owned and remain attachable after one UI client drops
    /// and another reconnects.
    #[test]
    fn daemon_sessions_survive_client_reconnect() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-reconnect");
        let client_a =
            SocketTerminalDaemonClient::connect(&socket_path).expect("first client should connect");
        let session_id = client_a
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");
        let attached_a = client_a
            .attach_session(&session_id)
            .expect("first client should attach");
        client_a
            .send_command(
                &session_id,
                TerminalCommand::SendCommand("printf 'persist-across-ui'".into()),
            )
            .expect("first client command should send");
        let _ = wait_for_surface_containing(&attached_a.updates, "persist-across-ui");
        drop(client_a);

        let client_b =
            SocketTerminalDaemonClient::connect(&socket_path).expect("second client should connect");
        let sessions = client_b
            .list_sessions()
            .expect("sessions should still exist after reconnect");
        assert!(sessions
            .iter()
            .any(|session| session.session_id == session_id));
        let attached_b = client_b
            .attach_session(&session_id)
            .expect("second client should reattach");
        let snapshot = attached_b
            .snapshot
            .surface
            .expect("reattach snapshot should include surface");
        assert!(surface_to_text(&snapshot).contains("persist-across-ui"));
    }

    /// Verifies that exited daemon sessions stay visible in session listings until the client explicitly
    /// kills/removes them.
    #[test]
    fn daemon_exited_sessions_remain_listed_until_explicit_kill() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-exited-listed");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");
        let attached = client
            .attach_session(&session_id)
            .expect("daemon session should attach");

        client
            .send_command(&session_id, TerminalCommand::SendCommand("exit".into()))
            .expect("exit command should send");
        let runtime = wait_for_lifecycle(&attached.updates, |lifecycle| {
            matches!(lifecycle, TerminalLifecycle::Exited { .. })
        });
        assert!(matches!(
            runtime.lifecycle,
            TerminalLifecycle::Exited { .. }
        ));

        let sessions = client.list_sessions().expect("daemon sessions should list");
        let session = sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .expect("exited session should remain listed");
        assert!(matches!(
            session.runtime.lifecycle,
            TerminalLifecycle::Exited { .. }
        ));

        client
            .kill_session(&session_id)
            .expect("explicit kill should remove exited session");
        let sessions = client
            .list_sessions()
            .expect("daemon sessions should relist");
        assert!(!sessions
            .iter()
            .any(|session| session.session_id == session_id));
    }

    /// Verifies that daemon session listings preserve daemon creation order rather than lexical session
    /// id order.
    #[test]
    fn daemon_session_listing_preserves_creation_order_not_lexical_order() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-list-order");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");

        let mut created = Vec::new();
        for _ in 0..12 {
            created.push(
                client
                    .create_session(PERSISTENT_SESSION_PREFIX, None)
                    .expect("daemon session should be created"),
            );
        }

        let listed = client
            .list_sessions()
            .expect("daemon sessions should list")
            .into_iter()
            .map(|session| session.session_id)
            .collect::<Vec<_>>();
        assert_eq!(listed, created);
    }

    /// Verifies that the daemon accepts an explicit resize request for a live session.
    #[test]
    fn daemon_resize_session_request_succeeds() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-resize");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");
        client
            .resize_session(&session_id, 100, 30)
            .expect("daemon resize should succeed");
    }

    /// Verifies that attaching to a missing daemon session returns a not-found error instead of
    /// succeeding silently.
    #[test]
    fn daemon_attach_missing_session_returns_error() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-missing-attach");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let error = client
            .attach_session("neozeus-session-missing")
            .expect_err("missing daemon session attach should fail");
        assert!(error.contains("not found"));
    }

    /// Verifies that killing a missing daemon session returns a not-found error.
    #[test]
    fn daemon_kill_missing_session_returns_error() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-missing-kill");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let error = client
            .kill_session("neozeus-session-missing")
            .expect_err("missing daemon session kill should fail");
        assert!(error.contains("not found"));
    }

    /// Verifies that multiple attached clients each receive the same streamed updates for a shared daemon
    /// session.
    #[test]
    fn daemon_multiple_clients_receive_updates_for_same_session() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-multi-client");
        let client_a =
            SocketTerminalDaemonClient::connect(&socket_path).expect("first client should connect");
        let session_id = client_a
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");
        let attached_a = client_a
            .attach_session(&session_id)
            .expect("first client should attach");

        let client_b =
            SocketTerminalDaemonClient::connect(&socket_path).expect("second client should connect");
        let attached_b = client_b
            .attach_session(&session_id)
            .expect("second client should attach");

        client_a
            .send_command(
                &session_id,
                TerminalCommand::SendCommand("printf 'fanout'".into()),
            )
            .expect("daemon command should send");

        let surface_a = wait_for_surface_containing(&attached_a.updates, "fanout");
        let surface_b = wait_for_surface_containing(&attached_b.updates, "fanout");
        assert!(surface_to_text(&surface_a).contains("fanout"));
        assert!(surface_to_text(&surface_b).contains("fanout"));
    }

    /// Verifies that daemon protocol decoding rejects a frame whose advertised payload is truncated.
    #[test]
    fn daemon_protocol_rejects_truncated_frame() {
        let bytes = vec![8, 0, 0, 0, 1, 2, 3];
        let error = read_client_message(&mut bytes.as_slice())
            .expect_err("truncated protocol frame should fail");
        assert!(error.contains("frame payload") || error.contains("truncated"));
    }

    /// Verifies that daemon protocol decoding rejects frames whose payload contains trailing garbage after
    /// a valid message.
    #[test]
    fn daemon_protocol_rejects_trailing_bytes_in_frame() {
        let message = ClientMessage::Request {
            request_id: 11,
            request: DaemonRequest::ListSessions,
        };
        let mut bytes = Vec::new();
        write_client_message(&mut bytes, &message).expect("client message should encode");
        let original_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let payload = &bytes[4..4 + original_len];
        let mut corrupted = Vec::new();
        corrupted.extend_from_slice(&((original_len + 1) as u32).to_le_bytes());
        corrupted.extend_from_slice(payload);
        corrupted.push(0xff);
        let error = read_client_message(&mut corrupted.as_slice())
            .expect_err("protocol frame with trailing payload bytes should fail");
        assert!(error.contains("trailing bytes"));
    }

    /// Verifies that a successful daemon resize eventually streams back a surface with the requested
    /// dimensions to attached clients.
    #[test]
    fn daemon_resize_session_updates_attached_surface_dimensions() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-resize-surface");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");
        let attached = client
            .attach_session(&session_id)
            .expect("daemon session should attach");
        client
            .resize_session(&session_id, 100, 30)
            .expect("daemon resize should succeed");
        let surface = wait_for_surface_dimensions(&attached.updates, 100, 30);
        assert_eq!((surface.cols, surface.rows), (100, 30));
    }

    /// Verifies that one client process cannot attach the same daemon session twice simultaneously.
    #[test]
    fn daemon_duplicate_attach_in_same_client_is_rejected() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-duplicate-attach");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");
        let _attached = client
            .attach_session(&session_id)
            .expect("first attach should succeed");
        let error = client
            .attach_session(&session_id)
            .expect_err("duplicate attach in same client should fail");
        assert!(error.contains("already attached"));
    }

    /// Verifies that killing one daemon session does not disturb other live daemon sessions.
    #[test]
    fn daemon_killing_one_session_preserves_other_sessions() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-isolated-kill");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let first = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("first daemon session should be created");
        let second = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("second daemon session should be created");
        client
            .kill_session(&first)
            .expect("first session should kill");
        let sessions = client
            .list_sessions()
            .expect("sessions should list after kill");
        assert!(!sessions.iter().any(|session| session.session_id == first));
        assert!(sessions.iter().any(|session| session.session_id == second));
    }

    /// Verifies that daemon `kill_session` only returns after the shell process has actually been
    /// hard-killed and reaped.
    #[test]
    fn daemon_kill_session_waits_for_shell_exit() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-kill-wait");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        let session_id = client
            .create_session(PERSISTENT_SESSION_PREFIX, None)
            .expect("daemon session should be created");

        let pid_file = temp_dir("neozeus-daemon-kill-wait-pid").join("shell.pid");
        client
            .send_command(
                &session_id,
                TerminalCommand::SendCommand(format!(
                    "echo $$ > {}; trap '' HUP; sleep 5",
                    pid_file.display()
                )),
            )
            .expect("daemon command should send");

        let pid_text = wait_for_file_text(&pid_file);
        let pid: u32 = pid_text.parse().expect("shell pid should parse");
        assert!(std::path::Path::new(&format!("/proc/{pid}")).exists());

        client
            .kill_session(&session_id)
            .expect("daemon session should kill");

        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "shell pid {pid} should be gone after kill_session returns"
        );
    }

    /// Stress-smoke test that repeated daemon create/attach/kill churn leaves the daemon in a clean
    /// empty state.
    #[test]
    fn daemon_session_lifecycle_churn_stays_consistent() {
        let (_server, socket_path) = start_test_daemon("neozeus-daemon-churn");
        let client =
            SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
        for _ in 0..5 {
            let session_id = client
                .create_session(PERSISTENT_SESSION_PREFIX, None)
                .expect("daemon session should be created during churn");
            let _attached = client
                .attach_session(&session_id)
                .expect("daemon session should attach during churn");
            client
                .kill_session(&session_id)
                .expect("daemon session should kill during churn");
        }
        let sessions = client
            .list_sessions()
            .expect("sessions should list after churn");
        assert!(sessions.is_empty());
    }
}
