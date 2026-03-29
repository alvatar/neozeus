use super::*;
use crate::terminals::{
    TerminalCommand, TerminalLifecycle, TerminalRuntimeState, TerminalSurface, TerminalUpdate,
};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc,
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
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
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

/// Verifies daemon socket-path resolution precedence: explicit override, then XDG runtime, then the
/// per-user temp-dir fallback.
#[test]
fn daemon_socket_path_prefers_override_then_xdg_runtime_then_tmp_user() {
    let override_path = resolve_daemon_socket_path_with(
        Some("/tmp/neozeus-test/daemon.sock"),
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
        Some("/run/user/1000"),
        Some("/home/alvatar"),
        Some("oracle"),
    )
    .expect("xdg runtime path should resolve");
    assert_eq!(path, PathBuf::from("/run/user/1000/neozeus/daemon.v2.sock"));

    let fallback =
        resolve_daemon_socket_path_with(None, None, Some("/home/alvatar"), Some("oracle"))
            .expect("tmp fallback should resolve");
    assert!(fallback.ends_with("neozeus-oracle/daemon.v2.sock"));
}

/// Verifies that the default daemon socket path is versioned so new clients do not attach to an
/// older incompatible daemon left running from a previous build.
#[test]
fn daemon_socket_path_uses_versioned_filename() {
    let resolved = resolve_daemon_socket_path_with(
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
