use super::super::super::types::{
    TerminalCell, TerminalCellContent, TerminalCellStyle, TerminalLifecycle, TerminalRuntimeState,
    TerminalSnapshot, TerminalSurface, TerminalUnderlineStyle,
};
use super::super::owned_tmux::OwnedTmuxSessionInfo;
use super::*;
use std::io::Cursor;

/// Encodes the legacy v1 session-info payload that predates `created_order` on the wire.
///
/// The test suite uses this helper to prove current decoders still accept old daemon payloads.
fn encode_v1_session_info(buffer: &mut Vec<u8>, info: &DaemonSessionInfo) {
    push_string(buffer, &info.session_id);
    encode_runtime_state(buffer, &info.runtime);
    push_u64(buffer, info.revision);
}

/// Verifies backward compatibility with v1 session-list payloads that omitted `created_order`.
///
/// Current decoders should fill `created_order` with zero rather than rejecting the payload.
#[test]
fn decodes_v1_session_list_payloads_without_created_order() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let response = DaemonResponse::SessionList {
        sessions: vec![DaemonSessionInfo {
            session_id: "neozeus-session-7".into(),
            runtime: TerminalRuntimeState {
                status: "running".into(),
                lifecycle: TerminalLifecycle::Running,
                last_error: None,
            },
            revision: 42,
            created_order: 7,
            metadata: crate::shared::daemon_wire::DaemonSessionMetadata::default(),
        }],
    };

    let mut payload = Vec::new();
    push_u8(&mut payload, 0);
    push_u64(&mut payload, 9);
    encode_result(
        &mut payload,
        &Ok(response),
        |buffer, response| match response {
            DaemonResponse::SessionList { sessions } => {
                push_u8(buffer, 1);
                push_vec(buffer, sessions, encode_v1_session_info);
            }
            _ => unreachable!("test only encodes session list"),
        },
    );

    let mut framed = Vec::new();
    push_u32(&mut framed, payload.len() as u32);
    framed.extend_from_slice(&payload);

    let message = read_server_message(&mut Cursor::new(framed)).expect("v1 payload should decode");
    let ServerMessage::Response {
        request_id,
        response,
    } = message
    else {
        panic!("expected response message");
    };
    assert_eq!(request_id, 9);
    let response = response.expect("response should be ok");
    let DaemonResponse::SessionList { sessions } = response else {
        panic!("expected session list response");
    };
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "neozeus-session-7");
    assert_eq!(sessions[0].revision, 42);
    assert_eq!(sessions[0].created_order, 0);
}

/// Verifies that modern encoders still omit `created_order` from the session-list wire format.
///
/// Ordering is conveyed by server list order, so keeping the field off-wire preserves v1
/// compatibility without losing semantics.
#[test]
fn session_list_wire_format_omits_created_order_for_v1_compatibility() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let message = ServerMessage::Response {
        request_id: 3,
        response: Ok(DaemonResponse::SessionList {
            sessions: vec![DaemonSessionInfo {
                session_id: "neozeus-session-3".into(),
                runtime: TerminalRuntimeState::running("running"),
                revision: 5,
                created_order: 999,
                metadata: crate::shared::daemon_wire::DaemonSessionMetadata::default(),
            }],
        }),
    };

    let mut bytes = Vec::new();
    write_server_message(&mut bytes, &message).expect("message should encode");
    let decoded = read_server_message(&mut Cursor::new(bytes)).expect("message should decode");
    let ServerMessage::Response { response, .. } = decoded else {
        panic!("expected response");
    };
    let DaemonResponse::SessionList { sessions } = response.expect("response should be ok") else {
        panic!("expected session list");
    };
    assert_eq!(sessions[0].session_id, "neozeus-session-3");
    assert_eq!(sessions[0].revision, 5);
    assert_eq!(sessions[0].created_order, 0);
}

/// Verifies that owned tmux request/response payloads round-trip through the daemon protocol.
#[test]
fn owned_tmux_wire_roundtrip_preserves_session_metadata() {
    let session = OwnedTmuxSessionInfo {
        session_uid: "tmux-session-1".into(),
        owner_agent_uid: "agent-uid-1".into(),
        tmux_name: "neozeus-tmux-1".into(),
        display_name: "BUILD".into(),
        cwd: "/tmp/work".into(),
        attached: true,
        created_unix: 42,
    };

    let response = ServerMessage::Response {
        request_id: 12,
        response: Ok(DaemonResponse::OwnedTmuxSessionCreated {
            session: session.clone(),
        }),
    };
    let mut bytes = Vec::new();
    write_server_message(&mut bytes, &response).expect("owned tmux response should encode");
    let decoded =
        read_server_message(&mut Cursor::new(bytes)).expect("owned tmux response should decode");
    let ServerMessage::Response {
        response: Ok(DaemonResponse::OwnedTmuxSessionCreated { session: decoded }),
        ..
    } = decoded
    else {
        panic!("expected owned tmux create response");
    };
    assert_eq!(decoded, session);

    let request = ClientMessage::Request {
        request_id: 33,
        request: DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid: "agent-uid-1".into(),
            display_name: "BUILD".into(),
            cwd: Some("/tmp/work".into()),
            command: "cargo test".into(),
        },
    };
    let mut request_bytes = Vec::new();
    write_client_message(&mut request_bytes, &request).expect("owned tmux request should encode");
    let decoded = read_client_message(&mut Cursor::new(request_bytes))
        .expect("owned tmux request should decode");
    let ClientMessage::Request {
        request:
            DaemonRequest::CreateOwnedTmuxSession {
                owner_agent_uid,
                display_name,
                cwd,
                command,
            },
        ..
    } = decoded
    else {
        panic!("expected owned tmux create request");
    };
    assert_eq!(owner_agent_uid, "agent-uid-1");
    assert_eq!(display_name, "BUILD");
    assert_eq!(cwd.as_deref(), Some("/tmp/work"));
    assert_eq!(command, "cargo test");
}

/// Verifies daemon protocol round-trips full cell styling metadata together with ordinary surface data.
#[test]
fn snapshot_wire_roundtrip_preserves_cell_style() {
    let snapshot = TerminalSnapshot {
        surface: Some(TerminalSurface {
            cols: 2,
            rows: 1,
            cells: vec![
                TerminalCell {
                    content: TerminalCellContent::Single('X'),
                    fg: egui::Color32::from_rgb(200, 210, 220),
                    bg: egui::Color32::from_rgb(10, 20, 30),
                    style: TerminalCellStyle {
                        bold: true,
                        italic: true,
                        dim: true,
                        underline: TerminalUnderlineStyle::Curly,
                        strikeout: true,
                        underline_color: Some(egui::Color32::from_rgb(1, 2, 3)),
                    },
                    width: 1,
                    selected: true,
                },
                TerminalCell::default(),
            ],
            cursor: None,
            selected_text: Some("X".into()),
            display_offset: 0,
        }),
        runtime: TerminalRuntimeState::running("running"),
    };

    let message = ServerMessage::Response {
        request_id: 4,
        response: Ok(DaemonResponse::SessionAttached {
            session_id: "neozeus-session-style".into(),
            snapshot: snapshot.clone(),
            revision: 7,
        }),
    };

    let mut bytes = Vec::new();
    write_server_message(&mut bytes, &message).expect("message should encode");
    let decoded = read_server_message(&mut Cursor::new(bytes)).expect("message should decode");
    let ServerMessage::Response {
        response: Ok(DaemonResponse::SessionAttached { snapshot, .. }),
        ..
    } = decoded
    else {
        panic!("expected attach-session response");
    };

    let cell = snapshot
        .surface
        .expect("snapshot surface should exist")
        .cell(0, 0)
        .clone();
    assert!(cell.style.bold);
    assert!(cell.style.italic);
    assert!(cell.style.dim);
    assert_eq!(cell.style.underline, TerminalUnderlineStyle::Curly);
    assert!(cell.style.strikeout);
    assert_eq!(
        cell.style.underline_color,
        Some(egui::Color32::from_rgb(1, 2, 3))
    );
}
