use super::super::types::{
    TerminalCell, TerminalCellContent, TerminalCellStyle, TerminalCommand, TerminalCursor,
    TerminalCursorShape, TerminalDamage, TerminalFrameUpdate, TerminalRuntimeState,
    TerminalSnapshot, TerminalSurface, TerminalUnderlineStyle, TerminalUpdate,
};
pub(crate) use crate::shared::daemon_wire::DaemonSessionInfo;
use crate::shared::daemon_wire::{self as wire, DaemonSessionMetadata, OwnedTmuxSessionInfo};
use bevy_egui::egui;
use std::io::{Read, Write};

type Decoder<'a> = wire::Decoder<'a>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ClientMessage {
    Request {
        request_id: u64,
        request: DaemonRequest,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DaemonRequest {
    ListSessions,
    ListSessionsDetailed,
    CreateSession {
        prefix: String,
        cwd: Option<String>,
        env_overrides: Vec<(String, String)>,
    },
    ListOwnedTmuxSessions,
    CreateOwnedTmuxSession {
        owner_agent_uid: String,
        display_name: String,
        cwd: Option<String>,
        command: String,
    },
    CaptureOwnedTmuxSession {
        session_uid: String,
        lines: usize,
    },
    AttachSession {
        session_id: String,
    },
    SendCommand {
        session_id: String,
        command: TerminalCommand,
    },
    ResizeSession {
        session_id: String,
        cols: usize,
        rows: usize,
    },
    KillSession {
        session_id: String,
    },
    KillOwnedTmuxSession {
        session_uid: String,
    },
    KillOwnedTmuxSessionsForAgent {
        owner_agent_uid: String,
    },
    UpdateSessionMetadata {
        session_id: String,
        metadata: DaemonSessionMetadata,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ServerMessage {
    Response {
        request_id: u64,
        response: Result<DaemonResponse, String>,
    },
    Event(DaemonEvent),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DaemonResponse {
    SessionList {
        sessions: Vec<DaemonSessionInfo>,
    },
    SessionListDetailed {
        sessions: Vec<DaemonSessionInfo>,
    },
    SessionCreated {
        session_id: String,
    },
    OwnedTmuxSessionList {
        sessions: Vec<OwnedTmuxSessionInfo>,
    },
    OwnedTmuxSessionCreated {
        session: OwnedTmuxSessionInfo,
    },
    OwnedTmuxSessionCapture {
        session_uid: String,
        text: String,
    },
    SessionAttached {
        session_id: String,
        snapshot: TerminalSnapshot,
        revision: u64,
    },
    Ack,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DaemonEvent {
    SessionUpdated {
        session_id: String,
        update: TerminalUpdate,
        revision: u64,
    },
}

/// Serializes and writes one client message as a length-prefixed protocol frame.
///
/// This is the public entry point used by daemon clients before bytes hit the socket.
pub(crate) fn write_client_message(
    writer: &mut impl Write,
    message: &ClientMessage,
) -> Result<(), String> {
    let mut payload = Vec::new();
    encode_client_message(&mut payload, message);
    wire::write_frame(writer, &payload)
}

/// Reads, decodes, and validates one length-prefixed client message frame.
///
/// Decoding rejects truncated payloads and trailing bytes after a valid message.
pub(crate) fn read_client_message(reader: &mut impl Read) -> Result<ClientMessage, String> {
    let payload = wire::read_frame(reader)?;
    let mut decoder = Decoder::new(&payload);
    let message = decode_client_message(&mut decoder)?;
    decoder.finish()?;
    Ok(message)
}

/// Serializes and writes one server message as a length-prefixed protocol frame.
///
/// Both daemon responses and async events go through this framing helper.
pub(crate) fn write_server_message(
    writer: &mut impl Write,
    message: &ServerMessage,
) -> Result<(), String> {
    let mut payload = Vec::new();
    encode_server_message(&mut payload, message);
    wire::write_frame(writer, &payload)
}

/// Reads, decodes, and validates one length-prefixed server message frame.
///
/// This is shared by real clients and compatibility tests.
pub(crate) fn read_server_message(reader: &mut impl Read) -> Result<ServerMessage, String> {
    let payload = wire::read_frame(reader)?;
    let mut decoder = Decoder::new(&payload);
    let message = decode_server_message(&mut decoder)?;
    decoder.finish()?;
    Ok(message)
}

/// Encodes one client message into the protocol payload format.
///
/// The first byte is a message tag; the rest is message-specific data.
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

/// Decodes one client message from the protocol payload stream.
///
/// Unknown tags are rejected immediately.
fn decode_client_message(decoder: &mut Decoder<'_>) -> Result<ClientMessage, String> {
    match decoder.read_u8()? {
        0 => Ok(ClientMessage::Request {
            request_id: decoder.read_u64()?,
            request: decode_request(decoder)?,
        }),
        tag => Err(format!("unknown client message tag {tag}")),
    }
}

/// Encodes one daemon request variant into its tagged wire representation.
fn encode_request(buffer: &mut Vec<u8>, request: &DaemonRequest) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    match request {
        DaemonRequest::CreateSession {
            prefix,
            cwd,
            env_overrides,
        } => {
            push_u8(buffer, 2);
            push_string(buffer, prefix);
            push_bool(buffer, cwd.is_some());
            if let Some(cwd) = cwd {
                push_string(buffer, cwd);
            }
            push_vec(buffer, env_overrides, |buffer, (key, value)| {
                push_string(buffer, key);
                push_string(buffer, value);
            });
        }
        DaemonRequest::CaptureOwnedTmuxSession { session_uid, lines } => {
            push_u8(buffer, 9);
            push_string(buffer, session_uid);
            push_usize(buffer, *lines);
        }
        DaemonRequest::AttachSession { session_id } => {
            push_u8(buffer, 3);
            push_string(buffer, session_id);
        }
        DaemonRequest::ResizeSession {
            session_id,
            cols,
            rows,
        } => {
            push_u8(buffer, 5);
            push_string(buffer, session_id);
            push_usize(buffer, *cols);
            push_usize(buffer, *rows);
        }
        DaemonRequest::KillSession { session_id } => {
            push_u8(buffer, 6);
            push_string(buffer, session_id);
        }
        DaemonRequest::KillOwnedTmuxSession { session_uid } => {
            push_u8(buffer, 10);
            push_string(buffer, session_uid);
        }
        DaemonRequest::KillOwnedTmuxSessionsForAgent { owner_agent_uid } => {
            push_u8(buffer, 11);
            push_string(buffer, owner_agent_uid);
        }
        DaemonRequest::ListSessions
        | DaemonRequest::ListSessionsDetailed
        | DaemonRequest::ListOwnedTmuxSessions
        | DaemonRequest::CreateOwnedTmuxSession { .. }
        | DaemonRequest::SendCommand { .. }
        | DaemonRequest::UpdateSessionMetadata { .. } => {
            wire::encode_core_daemon_request(buffer, &to_shared_request(request));
        }
    }
}

/// Decodes one daemon request variant from the payload stream.
fn decode_request(decoder: &mut Decoder<'_>) -> Result<DaemonRequest, String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    match decoder.read_u8()? {
        2 => {
            let prefix = decoder.read_string()?;
            let cwd = if decoder.read_bool()? {
                Some(decoder.read_string()?)
            } else {
                None
            };
            let env_overrides =
                decoder.read_vec(|decoder| Ok((decoder.read_string()?, decoder.read_string()?)))?;
            Ok(DaemonRequest::CreateSession {
                prefix,
                cwd,
                env_overrides,
            })
        }
        3 => Ok(DaemonRequest::AttachSession {
            session_id: decoder.read_string()?,
        }),
        5 => Ok(DaemonRequest::ResizeSession {
            session_id: decoder.read_string()?,
            cols: decoder.read_usize()?,
            rows: decoder.read_usize()?,
        }),
        6 => Ok(DaemonRequest::KillSession {
            session_id: decoder.read_string()?,
        }),
        9 => Ok(DaemonRequest::CaptureOwnedTmuxSession {
            session_uid: decoder.read_string()?,
            lines: decoder.read_usize()?,
        }),
        10 => Ok(DaemonRequest::KillOwnedTmuxSession {
            session_uid: decoder.read_string()?,
        }),
        11 => Ok(DaemonRequest::KillOwnedTmuxSessionsForAgent {
            owner_agent_uid: decoder.read_string()?,
        }),
        tag => from_shared_request(wire::decode_core_daemon_request_with_tag(decoder, tag)?),
    }
}

/// Encodes one server message into the protocol payload format.
///
/// Responses and events share the same outer tagged envelope.
fn to_shared_request(request: &DaemonRequest) -> wire::DaemonRequest {
    match request {
        DaemonRequest::ListSessions => wire::DaemonRequest::ListSessions,
        DaemonRequest::ListSessionsDetailed => wire::DaemonRequest::ListSessionsDetailed,
        DaemonRequest::ListOwnedTmuxSessions => wire::DaemonRequest::ListOwnedTmuxSessions,
        DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid,
            display_name,
            cwd,
            command,
        } => wire::DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid: owner_agent_uid.clone(),
            display_name: display_name.clone(),
            cwd: cwd.clone(),
            command: command.clone(),
        },
        DaemonRequest::SendCommand {
            session_id,
            command,
        } => wire::DaemonRequest::SendCommand {
            session_id: session_id.clone(),
            command: command.clone(),
        },
        DaemonRequest::UpdateSessionMetadata {
            session_id,
            metadata,
        } => wire::DaemonRequest::UpdateSessionMetadata {
            session_id: session_id.clone(),
            metadata: metadata.clone(),
        },
        DaemonRequest::CreateSession { .. }
        | DaemonRequest::CaptureOwnedTmuxSession { .. }
        | DaemonRequest::AttachSession { .. }
        | DaemonRequest::ResizeSession { .. }
        | DaemonRequest::KillSession { .. }
        | DaemonRequest::KillOwnedTmuxSession { .. }
        | DaemonRequest::KillOwnedTmuxSessionsForAgent { .. } => {
            unreachable!("extension-only request cannot be converted into shared core request")
        }
    }
}

fn from_shared_request(request: wire::DaemonRequest) -> Result<DaemonRequest, String> {
    match request {
        wire::DaemonRequest::ListSessions => Ok(DaemonRequest::ListSessions),
        wire::DaemonRequest::ListSessionsDetailed => Ok(DaemonRequest::ListSessionsDetailed),
        wire::DaemonRequest::ListOwnedTmuxSessions => Ok(DaemonRequest::ListOwnedTmuxSessions),
        wire::DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid,
            display_name,
            cwd,
            command,
        } => Ok(DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid,
            display_name,
            cwd,
            command,
        }),
        wire::DaemonRequest::SendCommand {
            session_id,
            command,
        } => Ok(DaemonRequest::SendCommand {
            session_id,
            command,
        }),
        wire::DaemonRequest::UpdateSessionMetadata {
            session_id,
            metadata,
        } => Ok(DaemonRequest::UpdateSessionMetadata {
            session_id,
            metadata,
        }),
    }
}

fn to_shared_response(response: &DaemonResponse) -> Result<wire::DaemonResponse, ()> {
    match response {
        DaemonResponse::SessionList { sessions } => Ok(wire::DaemonResponse::SessionList {
            sessions: sessions.clone(),
        }),
        DaemonResponse::SessionListDetailed { sessions } => {
            Ok(wire::DaemonResponse::SessionListDetailed {
                sessions: sessions.clone(),
            })
        }
        DaemonResponse::OwnedTmuxSessionList { sessions } => {
            Ok(wire::DaemonResponse::OwnedTmuxSessionList {
                sessions: sessions.clone(),
            })
        }
        DaemonResponse::OwnedTmuxSessionCreated { session } => {
            Ok(wire::DaemonResponse::OwnedTmuxSessionCreated {
                session: session.clone(),
            })
        }
        DaemonResponse::Ack => Ok(wire::DaemonResponse::Ack),
        DaemonResponse::SessionCreated { .. }
        | DaemonResponse::OwnedTmuxSessionCapture { .. }
        | DaemonResponse::SessionAttached { .. } => Err(()),
    }
}

fn from_shared_response(response: wire::DaemonResponse) -> DaemonResponse {
    match response {
        wire::DaemonResponse::SessionList { sessions } => DaemonResponse::SessionList { sessions },
        wire::DaemonResponse::SessionListDetailed { sessions } => {
            DaemonResponse::SessionListDetailed { sessions }
        }
        wire::DaemonResponse::OwnedTmuxSessionList { sessions } => {
            DaemonResponse::OwnedTmuxSessionList { sessions }
        }
        wire::DaemonResponse::OwnedTmuxSessionCreated { session } => {
            DaemonResponse::OwnedTmuxSessionCreated { session }
        }
        wire::DaemonResponse::Ack => DaemonResponse::Ack,
    }
}

fn encode_server_message(buffer: &mut Vec<u8>, message: &ServerMessage) {
    match message {
        ServerMessage::Response {
            request_id,
            response,
        } => {
            push_u8(buffer, 0);
            push_u64(buffer, *request_id);
            encode_result(buffer, response, encode_response);
        }
        ServerMessage::Event(event) => {
            push_u8(buffer, 1);
            encode_event(buffer, event);
        }
    }
}
/// Decodes one server message from the payload stream.
fn decode_server_message(decoder: &mut Decoder<'_>) -> Result<ServerMessage, String> {
    match decoder.read_u8()? {
        0 => Ok(ServerMessage::Response {
            request_id: decoder.read_u64()?,
            response: decode_result(decoder, decode_response)?,
        }),
        1 => Ok(ServerMessage::Event(decode_event(decoder)?)),
        tag => Err(format!("unknown server message tag {tag}")),
    }
}

/// Encodes one daemon response variant into its tagged wire representation.
fn encode_response(buffer: &mut Vec<u8>, response: &DaemonResponse) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if let Ok(shared) = to_shared_response(response) {
        wire::encode_core_daemon_response(buffer, &shared);
        return;
    }
    match response {
        DaemonResponse::SessionCreated { session_id } => {
            push_u8(buffer, 2);
            push_string(buffer, session_id);
        }
        DaemonResponse::SessionAttached {
            session_id,
            snapshot,
            revision,
        } => {
            push_u8(buffer, 3);
            push_string(buffer, session_id);
            encode_snapshot(buffer, snapshot);
            push_u64(buffer, *revision);
        }
        DaemonResponse::OwnedTmuxSessionList { sessions } => {
            push_u8(buffer, 5);
            push_vec(buffer, sessions, encode_owned_tmux_session_info);
        }
        DaemonResponse::OwnedTmuxSessionCapture { session_uid, text } => {
            push_u8(buffer, 7);
            push_string(buffer, session_uid);
            push_string(buffer, text);
        }
        DaemonResponse::SessionList { .. }
        | DaemonResponse::SessionListDetailed { .. }
        | DaemonResponse::OwnedTmuxSessionCreated { .. }
        | DaemonResponse::Ack => unreachable!("shared-core responses must have been handled above"),
    }
}

/// Decodes one daemon response variant from the payload stream.
fn decode_response(decoder: &mut Decoder<'_>) -> Result<DaemonResponse, String> {
    match decoder.read_u8()? {
        2 => Ok(DaemonResponse::SessionCreated {
            session_id: decoder.read_string()?,
        }),
        3 => Ok(DaemonResponse::SessionAttached {
            session_id: decoder.read_string()?,
            snapshot: decode_snapshot(decoder)?,
            revision: decoder.read_u64()?,
        }),
        5 => Ok(DaemonResponse::OwnedTmuxSessionList {
            sessions: decoder.read_vec(decode_owned_tmux_session_info)?,
        }),
        7 => Ok(DaemonResponse::OwnedTmuxSessionCapture {
            session_uid: decoder.read_string()?,
            text: decoder.read_string()?,
        }),
        tag => wire::decode_core_daemon_response_with_tag(decoder, tag).map(from_shared_response),
    }
}

/// Encodes the subset of session metadata that belongs on the daemon wire format.
///
/// `created_order` intentionally stays off-wire for protocol v1 compatibility.
fn encode_owned_tmux_session_info(buffer: &mut Vec<u8>, info: &OwnedTmuxSessionInfo) {
    push_string(buffer, &info.session_uid);
    push_string(buffer, &info.owner_agent_uid);
    push_string(buffer, &info.tmux_name);
    push_string(buffer, &info.display_name);
    push_string(buffer, &info.cwd);
    push_bool(buffer, info.attached);
    push_u64(buffer, info.created_unix);
}

fn decode_owned_tmux_session_info(
    decoder: &mut Decoder<'_>,
) -> Result<OwnedTmuxSessionInfo, String> {
    Ok(OwnedTmuxSessionInfo {
        session_uid: decoder.read_string()?,
        owner_agent_uid: decoder.read_string()?,
        tmux_name: decoder.read_string()?,
        display_name: decoder.read_string()?,
        cwd: decoder.read_string()?,
        attached: decoder.read_bool()?,
        created_unix: decoder.read_u64()?,
    })
}

/// Encodes one async daemon event into its tagged wire representation.
fn encode_event(buffer: &mut Vec<u8>, event: &DaemonEvent) {
    match event {
        DaemonEvent::SessionUpdated {
            session_id,
            update,
            revision,
        } => {
            push_u8(buffer, 0);
            push_string(buffer, session_id);
            encode_update(buffer, update);
            push_u64(buffer, *revision);
        }
    }
}

/// Decodes one async daemon event from the payload stream.
fn decode_event(decoder: &mut Decoder<'_>) -> Result<DaemonEvent, String> {
    match decoder.read_u8()? {
        0 => Ok(DaemonEvent::SessionUpdated {
            session_id: decoder.read_string()?,
            update: decode_update(decoder)?,
            revision: decoder.read_u64()?,
        }),
        tag => Err(format!("unknown daemon event tag {tag}")),
    }
}

/// Encodes a full terminal snapshot consisting of optional surface plus runtime state.
fn encode_snapshot(buffer: &mut Vec<u8>, snapshot: &TerminalSnapshot) {
    push_option(buffer, snapshot.surface.as_ref(), encode_surface);
    encode_runtime_state(buffer, &snapshot.runtime);
}

/// Decodes a full terminal snapshot from the payload stream.
fn decode_snapshot(decoder: &mut Decoder<'_>) -> Result<TerminalSnapshot, String> {
    Ok(TerminalSnapshot {
        surface: decoder.read_option(decode_surface)?,
        runtime: decode_runtime_state(decoder)?,
    })
}

/// Encodes either a frame update or a status update into the wire format.
fn encode_update(buffer: &mut Vec<u8>, update: &TerminalUpdate) {
    match update {
        TerminalUpdate::Frame(frame) => {
            push_u8(buffer, 0);
            encode_frame_update(buffer, frame);
        }
        TerminalUpdate::Status { runtime, surface } => {
            push_u8(buffer, 1);
            encode_runtime_state(buffer, runtime);
            push_option(buffer, surface.as_ref(), encode_surface);
        }
    }
}

/// Decodes either a frame update or a status update from the payload stream.
fn decode_update(decoder: &mut Decoder<'_>) -> Result<TerminalUpdate, String> {
    match decoder.read_u8()? {
        0 => Ok(TerminalUpdate::Frame(decode_frame_update(decoder)?)),
        1 => Ok(TerminalUpdate::Status {
            runtime: decode_runtime_state(decoder)?,
            surface: decoder.read_option(decode_surface)?,
        }),
        tag => Err(format!("unknown terminal update tag {tag}")),
    }
}

/// Encodes the full payload of a frame update: surface, damage, and runtime.
fn encode_frame_update(buffer: &mut Vec<u8>, frame: &TerminalFrameUpdate) {
    encode_surface(buffer, &frame.surface);
    encode_damage(buffer, &frame.damage);
    encode_runtime_state(buffer, &frame.runtime);
}

/// Decodes the full payload of a frame update from the payload stream.
fn decode_frame_update(decoder: &mut Decoder<'_>) -> Result<TerminalFrameUpdate, String> {
    Ok(TerminalFrameUpdate {
        surface: decode_surface(decoder)?,
        damage: decode_damage(decoder)?,
        runtime: decode_runtime_state(decoder)?,
    })
}

/// Encodes terminal damage either as full redraw or as an explicit row list.
fn encode_damage(buffer: &mut Vec<u8>, damage: &TerminalDamage) {
    match damage {
        TerminalDamage::Full => push_u8(buffer, 0),
        TerminalDamage::Rows(rows) => {
            push_u8(buffer, 1);
            push_vec(buffer, rows, |buffer, row| push_usize(buffer, *row));
        }
    }
}

/// Decodes terminal damage from the payload stream.
fn decode_damage(decoder: &mut Decoder<'_>) -> Result<TerminalDamage, String> {
    match decoder.read_u8()? {
        0 => Ok(TerminalDamage::Full),
        1 => Ok(TerminalDamage::Rows(
            decoder.read_vec(|decoder| decoder.read_usize())?,
        )),
        tag => Err(format!("unknown terminal damage tag {tag}")),
    }
}

/// Encodes a full terminal surface grid including dimensions, cells, and optional cursor.
fn encode_surface(buffer: &mut Vec<u8>, surface: &TerminalSurface) {
    push_usize(buffer, surface.cols);
    push_usize(buffer, surface.rows);
    push_vec(buffer, &surface.cells, encode_cell);
    push_option(buffer, surface.cursor.as_ref(), encode_cursor);
    push_option(buffer, surface.selected_text.as_ref(), |buffer, text| {
        push_string(buffer, text)
    });
    push_usize(buffer, surface.display_offset);
}

/// Decodes a full terminal surface grid from the payload stream.
fn decode_surface(decoder: &mut Decoder<'_>) -> Result<TerminalSurface, String> {
    Ok(TerminalSurface {
        cols: decoder.read_usize()?,
        rows: decoder.read_usize()?,
        cells: decoder.read_vec(decode_cell)?,
        cursor: decoder.read_option(decode_cursor)?,
        selected_text: decoder.read_option(|decoder| decoder.read_string())?,
        display_offset: decoder.read_usize()?,
    })
}

/// Encodes one terminal cell's content, colors, and width metadata.
fn encode_cell(buffer: &mut Vec<u8>, cell: &TerminalCell) {
    encode_cell_content(buffer, &cell.content);
    encode_color(buffer, cell.fg);
    encode_color(buffer, cell.bg);
    encode_cell_style(buffer, &cell.style);
    push_u8(buffer, cell.width);
    push_bool(buffer, cell.selected);
}

/// Decodes one terminal cell from the payload stream.
fn decode_cell(decoder: &mut Decoder<'_>) -> Result<TerminalCell, String> {
    Ok(TerminalCell {
        content: decode_cell_content(decoder)?,
        fg: decode_color(decoder)?,
        bg: decode_color(decoder)?,
        style: decode_cell_style(decoder)?,
        width: decoder.read_u8()?,
        selected: decoder.read_bool()?,
    })
}

/// Encodes cell styling metadata that survives parser → daemon → renderer round-trips.
fn encode_cell_style(buffer: &mut Vec<u8>, style: &TerminalCellStyle) {
    push_bool(buffer, style.bold);
    push_bool(buffer, style.italic);
    push_bool(buffer, style.dim);
    encode_underline_style(buffer, style.underline);
    push_bool(buffer, style.strikeout);
    push_option(buffer, style.underline_color.as_ref(), |buffer, color| {
        encode_color(buffer, *color)
    });
}

/// Decodes cell styling metadata from the payload stream.
fn decode_cell_style(decoder: &mut Decoder<'_>) -> Result<TerminalCellStyle, String> {
    Ok(TerminalCellStyle {
        bold: decoder.read_bool()?,
        italic: decoder.read_bool()?,
        dim: decoder.read_bool()?,
        underline: decode_underline_style(decoder)?,
        strikeout: decoder.read_bool()?,
        underline_color: decoder.read_option(decode_color)?,
    })
}

/// Encodes the underline-style enum as a tiny wire tag.
fn encode_underline_style(buffer: &mut Vec<u8>, underline: TerminalUnderlineStyle) {
    match underline {
        TerminalUnderlineStyle::None => push_u8(buffer, 0),
        TerminalUnderlineStyle::Single => push_u8(buffer, 1),
        TerminalUnderlineStyle::Double => push_u8(buffer, 2),
        TerminalUnderlineStyle::Curly => push_u8(buffer, 3),
        TerminalUnderlineStyle::Dotted => push_u8(buffer, 4),
        TerminalUnderlineStyle::Dashed => push_u8(buffer, 5),
    }
}

/// Decodes the underline-style enum from its wire tag.
fn decode_underline_style(decoder: &mut Decoder<'_>) -> Result<TerminalUnderlineStyle, String> {
    match decoder.read_u8()? {
        0 => Ok(TerminalUnderlineStyle::None),
        1 => Ok(TerminalUnderlineStyle::Single),
        2 => Ok(TerminalUnderlineStyle::Double),
        3 => Ok(TerminalUnderlineStyle::Curly),
        4 => Ok(TerminalUnderlineStyle::Dotted),
        5 => Ok(TerminalUnderlineStyle::Dashed),
        tag => Err(format!("unknown underline style tag {tag}")),
    }
}

/// Encodes the compact terminal cell-content enum used by the surface grid.
///
/// Small inline grapheme storage and heap-backed text use different tags so common single/small-cell
/// cases stay cheap on the wire.
fn encode_cell_content(buffer: &mut Vec<u8>, content: &TerminalCellContent) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    match content {
        TerminalCellContent::Empty => push_u8(buffer, 0),
        TerminalCellContent::Single(ch) => {
            push_u8(buffer, 1);
            push_char(buffer, *ch);
        }
        TerminalCellContent::InlineSmall(chars, len) => {
            push_u8(buffer, 2);
            push_u8(buffer, *len);
            for ch in chars {
                push_char(buffer, *ch);
            }
        }
        TerminalCellContent::Heap(text) => {
            push_u8(buffer, 3);
            push_string(buffer, text);
        }
    }
}

/// Decodes the compact terminal cell-content enum from the payload stream.
fn decode_cell_content(decoder: &mut Decoder<'_>) -> Result<TerminalCellContent, String> {
    match decoder.read_u8()? {
        0 => Ok(TerminalCellContent::Empty),
        1 => Ok(TerminalCellContent::Single(decoder.read_char()?)),
        2 => {
            let len = decoder.read_u8()?;
            let first = decoder.read_char()?;
            let second = decoder.read_char()?;
            Ok(TerminalCellContent::InlineSmall([first, second], len))
        }
        3 => Ok(TerminalCellContent::Heap(decoder.read_string()?.into())),
        tag => Err(format!("unknown terminal cell content tag {tag}")),
    }
}

/// Encodes cursor position, shape, visibility, and color.
fn encode_cursor(buffer: &mut Vec<u8>, cursor: &TerminalCursor) {
    push_usize(buffer, cursor.x);
    push_usize(buffer, cursor.y);
    encode_cursor_shape(buffer, cursor.shape);
    push_bool(buffer, cursor.visible);
    encode_color(buffer, cursor.color);
}

/// Decodes cursor metadata from the payload stream.
fn decode_cursor(decoder: &mut Decoder<'_>) -> Result<TerminalCursor, String> {
    Ok(TerminalCursor {
        x: decoder.read_usize()?,
        y: decoder.read_usize()?,
        shape: decode_cursor_shape(decoder)?,
        visible: decoder.read_bool()?,
        color: decode_color(decoder)?,
    })
}

/// Encodes the cursor-shape enum as a tiny tag.
fn encode_cursor_shape(buffer: &mut Vec<u8>, shape: TerminalCursorShape) {
    match shape {
        TerminalCursorShape::Block => push_u8(buffer, 0),
        TerminalCursorShape::Underline => push_u8(buffer, 1),
        TerminalCursorShape::Beam => push_u8(buffer, 2),
    }
}

/// Decodes the cursor-shape enum from its wire tag.
fn decode_cursor_shape(decoder: &mut Decoder<'_>) -> Result<TerminalCursorShape, String> {
    match decoder.read_u8()? {
        0 => Ok(TerminalCursorShape::Block),
        1 => Ok(TerminalCursorShape::Underline),
        2 => Ok(TerminalCursorShape::Beam),
        tag => Err(format!("unknown cursor shape tag {tag}")),
    }
}

/// Encodes the runtime status string, lifecycle enum, and optional last-error text.
fn encode_runtime_state(buffer: &mut Vec<u8>, state: &TerminalRuntimeState) {
    wire::encode_wire_runtime_state(buffer, state);
}

/// Decodes runtime status metadata from the payload stream.
fn decode_runtime_state(decoder: &mut Decoder<'_>) -> Result<TerminalRuntimeState, String> {
    wire::decode_wire_runtime_state(decoder)
}

/// Encodes an `egui::Color32` as four raw RGBA bytes.
fn encode_color(buffer: &mut Vec<u8>, color: egui::Color32) {
    let [r, g, b, a] = color.to_array();
    push_u8(buffer, r);
    push_u8(buffer, g);
    push_u8(buffer, b);
    push_u8(buffer, a);
}

/// Decodes an `egui::Color32` from four raw RGBA bytes.
fn decode_color(decoder: &mut Decoder<'_>) -> Result<egui::Color32, String> {
    Ok(egui::Color32::from_rgba_unmultiplied(
        decoder.read_u8()?,
        decoder.read_u8()?,
        decoder.read_u8()?,
        decoder.read_u8()?,
    ))
}

/// Encodes result.
fn encode_result<T>(
    buffer: &mut Vec<u8>,
    result: &Result<T, String>,
    encode_ok: impl Fn(&mut Vec<u8>, &T),
) {
    match result {
        Ok(value) => {
            push_u8(buffer, 0);
            encode_ok(buffer, value);
        }
        Err(error) => {
            push_u8(buffer, 1);
            push_string(buffer, error);
        }
    }
}

/// Decodes result.
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

/// Pushes vec.
fn push_vec<T>(buffer: &mut Vec<u8>, items: &[T], encode: impl Fn(&mut Vec<u8>, &T)) {
    push_u32(buffer, u32::try_from(items.len()).unwrap_or(u32::MAX));
    for item in items {
        encode(buffer, item);
    }
}

/// Pushes option.
fn push_option<T>(buffer: &mut Vec<u8>, value: Option<&T>, encode: impl Fn(&mut Vec<u8>, &T)) {
    match value {
        Some(value) => {
            push_bool(buffer, true);
            encode(buffer, value);
        }
        None => push_bool(buffer, false),
    }
}

/// Appends a boolean as `0` or `1` to the payload buffer.
fn push_bool(buffer: &mut Vec<u8>, value: bool) {
    push_u8(buffer, u8::from(value));
}

/// Appends one raw byte to the payload buffer.
fn push_u8(buffer: &mut Vec<u8>, value: u8) {
    buffer.push(value);
}

/// Appends a little-endian `u32` to the payload buffer.
fn push_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

/// Appends a little-endian `u64` to the payload buffer.
fn push_u64(buffer: &mut Vec<u8>, value: u64) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

/// Encodes `usize` through the protocol's fixed `u64` representation.
fn push_usize(buffer: &mut Vec<u8>, value: usize) {
    push_u64(buffer, value as u64);
}

/// Encodes a Rust `char` as its Unicode scalar value in little-endian `u32` form.
fn push_char(buffer: &mut Vec<u8>, value: char) {
    push_u32(buffer, value as u32);
}

/// Encodes a UTF-8 string as `<u32 byte length><raw bytes>`.
fn push_string(buffer: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    push_u32(buffer, u32::try_from(bytes.len()).unwrap_or(u32::MAX));
    buffer.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests;
