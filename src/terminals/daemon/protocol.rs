use super::super::types::{
    TerminalCell, TerminalCellContent, TerminalCellStyle, TerminalCommand, TerminalCursor,
    TerminalCursorShape, TerminalDamage, TerminalFrameUpdate, TerminalLifecycle,
    TerminalRuntimeState, TerminalSnapshot, TerminalSurface, TerminalUnderlineStyle,
    TerminalUpdate,
};
use crate::shared::daemon_wire as wire;
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
    CreateSession {
        prefix: String,
        cwd: Option<String>,
        env_overrides: Vec<(String, String)>,
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
    SessionCreated {
        session_id: String,
    },
    SessionAttached {
        session_id: String,
        snapshot: TerminalSnapshot,
        revision: u64,
    },
    Ack,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DaemonSessionInfo {
    pub(crate) session_id: String,
    pub(crate) runtime: TerminalRuntimeState,
    pub(crate) revision: u64,
    pub(crate) created_order: u64,
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
    write_frame(writer, &payload)
}

/// Reads, decodes, and validates one length-prefixed client message frame.
///
/// Decoding rejects truncated payloads and trailing bytes after a valid message.
pub(crate) fn read_client_message(reader: &mut impl Read) -> Result<ClientMessage, String> {
    let payload = read_frame(reader)?;
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
    write_frame(writer, &payload)
}

/// Reads, decodes, and validates one length-prefixed server message frame.
///
/// This is shared by real clients and compatibility tests.
pub(crate) fn read_server_message(reader: &mut impl Read) -> Result<ServerMessage, String> {
    let payload = read_frame(reader)?;
    let mut decoder = Decoder::new(&payload);
    let message = decode_server_message(&mut decoder)?;
    decoder.finish()?;
    Ok(message)
}

/// Writes one raw protocol frame as `<u32 little-endian length><payload>`.
///
/// The writer is flushed before returning so request/response round-trips are not delayed in socket
/// buffers.
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

/// Reads one raw length-prefixed protocol frame into memory.
///
/// The whole payload is buffered because higher-level decoders work over byte slices.
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
        DaemonRequest::ListSessions => push_u8(buffer, 1),
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
        DaemonRequest::AttachSession { session_id } => {
            push_u8(buffer, 3);
            push_string(buffer, session_id);
        }
        DaemonRequest::SendCommand {
            session_id,
            command,
        } => {
            push_u8(buffer, 4);
            push_string(buffer, session_id);
            encode_command(buffer, command);
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
    }
}

/// Decodes one daemon request variant from the payload stream.
fn decode_request(decoder: &mut Decoder<'_>) -> Result<DaemonRequest, String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    match decoder.read_u8()? {
        1 => Ok(DaemonRequest::ListSessions),
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
        4 => Ok(DaemonRequest::SendCommand {
            session_id: decoder.read_string()?,
            command: decode_command(decoder)?,
        }),
        5 => Ok(DaemonRequest::ResizeSession {
            session_id: decoder.read_string()?,
            cols: decoder.read_usize()?,
            rows: decoder.read_usize()?,
        }),
        6 => Ok(DaemonRequest::KillSession {
            session_id: decoder.read_string()?,
        }),
        tag => Err(format!("unknown daemon request tag {tag}")),
    }
}

/// Encodes one server message into the protocol payload format.
///
/// Responses and events share the same outer tagged envelope.
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
    match response {
        DaemonResponse::SessionList { sessions } => {
            push_u8(buffer, 1);
            push_vec(buffer, sessions, encode_session_info);
        }
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
        DaemonResponse::Ack => push_u8(buffer, 4),
    }
}

/// Decodes one daemon response variant from the payload stream.
fn decode_response(decoder: &mut Decoder<'_>) -> Result<DaemonResponse, String> {
    match decoder.read_u8()? {
        1 => Ok(DaemonResponse::SessionList {
            sessions: decoder.read_vec(decode_session_info)?,
        }),
        2 => Ok(DaemonResponse::SessionCreated {
            session_id: decoder.read_string()?,
        }),
        3 => Ok(DaemonResponse::SessionAttached {
            session_id: decoder.read_string()?,
            snapshot: decode_snapshot(decoder)?,
            revision: decoder.read_u64()?,
        }),
        4 => Ok(DaemonResponse::Ack),
        tag => Err(format!("unknown daemon response tag {tag}")),
    }
}

/// Encodes the subset of session metadata that belongs on the daemon wire format.
///
/// `created_order` intentionally stays off-wire for protocol v1 compatibility.
fn encode_session_info(buffer: &mut Vec<u8>, info: &DaemonSessionInfo) {
    push_string(buffer, &info.session_id);
    encode_runtime_state(buffer, &info.runtime);
    // Keep the wire format compatible with protocol v1 daemons/clients. Session list ordering is
    // already defined by server response order, so `created_order` stays server-side only.
    push_u64(buffer, info.revision);
}

/// Decodes session metadata from the wire format, defaulting missing legacy `created_order` to 0.
fn decode_session_info(decoder: &mut Decoder<'_>) -> Result<DaemonSessionInfo, String> {
    Ok(DaemonSessionInfo {
        session_id: decoder.read_string()?,
        runtime: decode_runtime_state(decoder)?,
        revision: decoder.read_u64()?,
        created_order: 0,
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

/// Encodes one terminal command into its tagged wire representation.
fn encode_command(buffer: &mut Vec<u8>, command: &TerminalCommand) {
    wire::encode_wire_terminal_command(buffer, &to_wire_command(command));
}

/// Decodes one terminal command from the payload stream.
fn decode_command(decoder: &mut Decoder<'_>) -> Result<TerminalCommand, String> {
    wire::decode_wire_terminal_command(decoder).map(from_wire_command)
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
}

/// Decodes a full terminal surface grid from the payload stream.
fn decode_surface(decoder: &mut Decoder<'_>) -> Result<TerminalSurface, String> {
    Ok(TerminalSurface {
        cols: decoder.read_usize()?,
        rows: decoder.read_usize()?,
        cells: decoder.read_vec(decode_cell)?,
        cursor: decoder.read_option(decode_cursor)?,
    })
}

/// Encodes one terminal cell's content, colors, and width metadata.
fn encode_cell(buffer: &mut Vec<u8>, cell: &TerminalCell) {
    encode_cell_content(buffer, &cell.content);
    encode_color(buffer, cell.fg);
    encode_color(buffer, cell.bg);
    encode_cell_style(buffer, &cell.style);
    push_u8(buffer, cell.width);
}

/// Decodes one terminal cell from the payload stream.
fn decode_cell(decoder: &mut Decoder<'_>) -> Result<TerminalCell, String> {
    Ok(TerminalCell {
        content: decode_cell_content(decoder)?,
        fg: decode_color(decoder)?,
        bg: decode_color(decoder)?,
        style: decode_cell_style(decoder)?,
        width: decoder.read_u8()?,
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
    wire::encode_wire_runtime_state(buffer, &to_wire_runtime_state(state));
}

/// Decodes runtime status metadata from the payload stream.
fn decode_runtime_state(decoder: &mut Decoder<'_>) -> Result<TerminalRuntimeState, String> {
    wire::decode_wire_runtime_state(decoder).map(from_wire_runtime_state)
}

fn to_wire_command(command: &TerminalCommand) -> wire::TerminalCommand {
    match command {
        TerminalCommand::InputText(text) => wire::TerminalCommand::InputText(text.clone()),
        TerminalCommand::InputEvent(event) => wire::TerminalCommand::InputEvent(event.clone()),
        TerminalCommand::SendCommand(command) => {
            wire::TerminalCommand::SendCommand(command.clone())
        }
        TerminalCommand::ScrollDisplay(lines) => wire::TerminalCommand::ScrollDisplay(*lines),
    }
}

fn from_wire_command(command: wire::TerminalCommand) -> TerminalCommand {
    match command {
        wire::TerminalCommand::InputText(text) => TerminalCommand::InputText(text),
        wire::TerminalCommand::InputEvent(event) => TerminalCommand::InputEvent(event),
        wire::TerminalCommand::SendCommand(command) => TerminalCommand::SendCommand(command),
        wire::TerminalCommand::ScrollDisplay(lines) => TerminalCommand::ScrollDisplay(lines),
    }
}

fn to_wire_runtime_state(state: &TerminalRuntimeState) -> wire::TerminalRuntimeState {
    wire::TerminalRuntimeState {
        status: state.status.clone(),
        lifecycle: to_wire_lifecycle(&state.lifecycle),
        last_error: state.last_error.clone(),
    }
}

fn from_wire_runtime_state(state: wire::TerminalRuntimeState) -> TerminalRuntimeState {
    TerminalRuntimeState {
        status: state.status,
        lifecycle: from_wire_lifecycle(state.lifecycle),
        last_error: state.last_error,
    }
}

fn to_wire_lifecycle(lifecycle: &TerminalLifecycle) -> wire::TerminalLifecycle {
    match lifecycle {
        TerminalLifecycle::Running => wire::TerminalLifecycle::Running,
        TerminalLifecycle::Exited { code, signal } => wire::TerminalLifecycle::Exited {
            code: *code,
            signal: signal.clone(),
        },
        TerminalLifecycle::Disconnected => wire::TerminalLifecycle::Disconnected,
        TerminalLifecycle::Failed => wire::TerminalLifecycle::Failed,
    }
}

fn from_wire_lifecycle(lifecycle: wire::TerminalLifecycle) -> TerminalLifecycle {
    match lifecycle {
        wire::TerminalLifecycle::Running => TerminalLifecycle::Running,
        wire::TerminalLifecycle::Exited { code, signal } => {
            TerminalLifecycle::Exited { code, signal }
        }
        wire::TerminalLifecycle::Disconnected => TerminalLifecycle::Disconnected,
        wire::TerminalLifecycle::Failed => TerminalLifecycle::Failed,
    }
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
