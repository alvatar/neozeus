use super::*;
use crate::terminals::{TerminalLifecycle, TerminalRuntimeState};
use std::io::Cursor;

fn encode_v1_session_info(buffer: &mut Vec<u8>, info: &DaemonSessionInfo) {
    push_string(buffer, &info.session_id);
    encode_runtime_state(buffer, &info.runtime);
    push_u64(buffer, info.revision);
}

#[test]
fn decodes_v1_session_list_payloads_without_created_order() {
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

#[test]
fn session_list_wire_format_omits_created_order_for_v1_compatibility() {
    let message = ServerMessage::Response {
        request_id: 3,
        response: Ok(DaemonResponse::SessionList {
            sessions: vec![DaemonSessionInfo {
                session_id: "neozeus-session-3".into(),
                runtime: TerminalRuntimeState::running("running"),
                revision: 5,
                created_order: 999,
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
