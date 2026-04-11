use std::io::{Read, Write};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TerminalLifecycle {
    #[default]
    Running,
    Exited {
        code: Option<u32>,
        signal: Option<String>,
    },
    Disconnected,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalRuntimeState {
    pub status: String,
    pub lifecycle: TerminalLifecycle,
    pub last_error: Option<String>,
}

impl TerminalRuntimeState {
    pub fn is_interactive(&self) -> bool {
        matches!(self.lifecycle, TerminalLifecycle::Running)
    }

    pub fn running(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            lifecycle: TerminalLifecycle::Running,
            last_error: None,
        }
    }

    pub fn failed(status: impl Into<String>) -> Self {
        let status = status.into();
        Self {
            status: status.clone(),
            lifecycle: TerminalLifecycle::Failed,
            last_error: Some(status),
        }
    }

    pub fn disconnected(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            lifecycle: TerminalLifecycle::Disconnected,
            last_error: None,
        }
    }

    pub fn exited(status: impl Into<String>, code: Option<u32>, signal: Option<String>) -> Self {
        Self {
            status: status.into(),
            lifecycle: TerminalLifecycle::Exited { code, signal },
            last_error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalViewportPoint {
    pub col: usize,
    pub row: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalCommand {
    InputText(String),
    InputEvent(String),
    SendCommand(String),
    ScrollDisplay(i32),
    SetSelection {
        anchor: TerminalViewportPoint,
        focus: TerminalViewportPoint,
    },
    ClearSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonAgentKind {
    Pi,
    Claude,
    Codex,
    Terminal,
    Verifier,
}

impl DaemonAgentKind {
    pub const fn env_name(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Terminal => "terminal",
            Self::Verifier => "verifier",
        }
    }

    pub fn from_env_name(value: &str) -> Option<Self> {
        match value.trim() {
            "pi" => Some(Self::Pi),
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "terminal" => Some(Self::Terminal),
            "verifier" => Some(Self::Verifier),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DaemonSessionMetadata {
    pub agent_uid: Option<String>,
    pub agent_label: Option<String>,
    pub agent_kind: Option<DaemonAgentKind>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonSessionInfo {
    pub session_id: String,
    pub runtime: TerminalRuntimeState,
    pub revision: u64,
    pub created_order: u64,
    pub metadata: DaemonSessionMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedTmuxSessionInfo {
    pub session_uid: String,
    pub owner_agent_uid: String,
    pub tmux_name: String,
    pub display_name: String,
    pub cwd: String,
    pub attached: bool,
    pub created_unix: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientMessage {
    Request {
        request_id: u64,
        request: DaemonRequest,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DaemonRequest {
    ListSessions,
    ListSessionsDetailed,
    ListOwnedTmuxSessions,
    CreateOwnedTmuxSession {
        owner_agent_uid: String,
        display_name: String,
        cwd: Option<String>,
        command: String,
    },
    SendCommand {
        session_id: String,
        command: TerminalCommand,
    },
    UpdateSessionMetadata {
        session_id: String,
        metadata: DaemonSessionMetadata,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ServerMessage {
    Response {
        request_id: u64,
        response: Result<DaemonResponse, String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DaemonResponse {
    SessionList { sessions: Vec<DaemonSessionInfo> },
    SessionListDetailed { sessions: Vec<DaemonSessionInfo> },
    OwnedTmuxSessionList { sessions: Vec<OwnedTmuxSessionInfo> },
    OwnedTmuxSessionCreated { session: OwnedTmuxSessionInfo },
    Ack,
}

/// Serializes and writes one CLI-compatible client message as a length-prefixed daemon frame.
pub fn write_client_message(
    writer: &mut impl Write,
    message: &ClientMessage,
) -> Result<(), String> {
    let mut payload = Vec::new();
    encode_client_message(&mut payload, message);
    write_frame(writer, &payload)
}

/// Reads and decodes one CLI-compatible server response frame.
pub fn read_server_message(reader: &mut impl Read) -> Result<ServerMessage, String> {
    let payload = read_frame(reader)?;
    let mut decoder = Decoder::new(&payload);
    let message = decode_server_message(&mut decoder)?;
    decoder.finish()?;
    Ok(message)
}

pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> Result<(), String> {
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

pub fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, String> {
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
            encode_core_daemon_request(buffer, request);
        }
    }
}

pub fn encode_core_daemon_request(buffer: &mut Vec<u8>, request: &DaemonRequest) {
    match request {
        DaemonRequest::ListSessions => push_u8(buffer, 1),
        DaemonRequest::ListSessionsDetailed => push_u8(buffer, 12),
        DaemonRequest::ListOwnedTmuxSessions => push_u8(buffer, 7),
        DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid,
            display_name,
            cwd,
            command,
        } => {
            push_u8(buffer, 8);
            push_string(buffer, owner_agent_uid);
            push_string(buffer, display_name);
            push_bool(buffer, cwd.is_some());
            if let Some(cwd) = cwd {
                push_string(buffer, cwd);
            }
            push_string(buffer, command);
        }
        DaemonRequest::SendCommand {
            session_id,
            command,
        } => {
            push_u8(buffer, 4);
            push_string(buffer, session_id);
            encode_wire_terminal_command(buffer, command);
        }
        DaemonRequest::UpdateSessionMetadata {
            session_id,
            metadata,
        } => {
            push_u8(buffer, 13);
            push_string(buffer, session_id);
            encode_daemon_session_metadata(buffer, metadata);
        }
    }
}

pub fn decode_core_daemon_request_with_tag(
    decoder: &mut Decoder<'_>,
    tag: u8,
) -> Result<DaemonRequest, String> {
    match tag {
        1 => Ok(DaemonRequest::ListSessions),
        4 => Ok(DaemonRequest::SendCommand {
            session_id: decoder.read_string()?,
            command: decode_wire_terminal_command(decoder)?,
        }),
        7 => Ok(DaemonRequest::ListOwnedTmuxSessions),
        8 => Ok(DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid: decoder.read_string()?,
            display_name: decoder.read_string()?,
            cwd: decoder.read_option(|decoder| decoder.read_string())?,
            command: decoder.read_string()?,
        }),
        12 => Ok(DaemonRequest::ListSessionsDetailed),
        13 => Ok(DaemonRequest::UpdateSessionMetadata {
            session_id: decoder.read_string()?,
            metadata: decode_daemon_session_metadata(decoder)?,
        }),
        tag => Err(format!("unknown daemon request tag {tag}")),
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

pub fn decode_core_daemon_response_with_tag(
    decoder: &mut Decoder<'_>,
    tag: u8,
) -> Result<DaemonResponse, String> {
    match tag {
        1 => Ok(DaemonResponse::SessionList {
            sessions: decoder.read_vec(decode_wire_daemon_session_info_legacy)?,
        }),
        4 => Ok(DaemonResponse::Ack),
        5 => Ok(DaemonResponse::OwnedTmuxSessionList {
            sessions: decoder.read_vec(decode_owned_tmux_session_info)?,
        }),
        6 => Ok(DaemonResponse::OwnedTmuxSessionCreated {
            session: decode_owned_tmux_session_info(decoder)?,
        }),
        8 => Ok(DaemonResponse::SessionListDetailed {
            sessions: decoder.read_vec(decode_wire_daemon_session_info)?,
        }),
        tag => Err(format!("unknown daemon response tag {tag}")),
    }
}

fn decode_response(decoder: &mut Decoder<'_>) -> Result<DaemonResponse, String> {
    let tag = decoder.read_u8()?;
    decode_core_daemon_response_with_tag(decoder, tag)
}

pub fn encode_core_daemon_response(buffer: &mut Vec<u8>, response: &DaemonResponse) {
    match response {
        DaemonResponse::SessionList { sessions } => {
            push_u8(buffer, 1);
            push_vec(buffer, sessions, encode_wire_daemon_session_info_legacy);
        }
        DaemonResponse::SessionListDetailed { sessions } => {
            push_u8(buffer, 8);
            push_vec(buffer, sessions, encode_wire_daemon_session_info);
        }
        DaemonResponse::OwnedTmuxSessionList { sessions } => {
            push_u8(buffer, 5);
            push_vec(buffer, sessions, encode_owned_tmux_session_info);
        }
        DaemonResponse::OwnedTmuxSessionCreated { session } => {
            push_u8(buffer, 6);
            encode_owned_tmux_session_info(buffer, session);
        }
        DaemonResponse::Ack => push_u8(buffer, 4),
    }
}

pub fn encode_owned_tmux_session_info(buffer: &mut Vec<u8>, info: &OwnedTmuxSessionInfo) {
    push_string(buffer, &info.session_uid);
    push_string(buffer, &info.owner_agent_uid);
    push_string(buffer, &info.tmux_name);
    push_string(buffer, &info.display_name);
    push_string(buffer, &info.cwd);
    push_bool(buffer, info.attached);
    push_u64(buffer, info.created_unix);
}

pub(crate) fn decode_owned_tmux_session_info(
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

pub fn encode_wire_daemon_session_info_legacy(buffer: &mut Vec<u8>, info: &DaemonSessionInfo) {
    push_string(buffer, &info.session_id);
    encode_wire_runtime_state(buffer, &info.runtime);
    // Session-list wire compatibility intentionally omits `created_order`; daemon response order
    // already defines list ordering and the main daemon protocol keeps this field off-wire.
    push_u64(buffer, info.revision);
}

pub fn encode_wire_daemon_session_info(buffer: &mut Vec<u8>, info: &DaemonSessionInfo) {
    encode_wire_daemon_session_info_legacy(buffer, info);
    encode_daemon_session_metadata(buffer, &info.metadata);
}

pub(crate) fn decode_wire_daemon_session_info_legacy(
    decoder: &mut Decoder<'_>,
) -> Result<DaemonSessionInfo, String> {
    Ok(DaemonSessionInfo {
        session_id: decoder.read_string()?,
        runtime: decode_wire_runtime_state(decoder)?,
        revision: decoder.read_u64()?,
        created_order: 0,
        metadata: DaemonSessionMetadata::default(),
    })
}

pub(crate) fn decode_wire_daemon_session_info(
    decoder: &mut Decoder<'_>,
) -> Result<DaemonSessionInfo, String> {
    let mut info = decode_wire_daemon_session_info_legacy(decoder)?;
    info.metadata = decode_daemon_session_metadata(decoder)?;
    Ok(info)
}

pub fn encode_daemon_session_metadata(buffer: &mut Vec<u8>, metadata: &DaemonSessionMetadata) {
    push_bool(buffer, metadata.agent_uid.is_some());
    if let Some(agent_uid) = &metadata.agent_uid {
        push_string(buffer, agent_uid);
    }
    push_bool(buffer, metadata.agent_label.is_some());
    if let Some(agent_label) = &metadata.agent_label {
        push_string(buffer, agent_label);
    }
    push_bool(buffer, metadata.agent_kind.is_some());
    if let Some(agent_kind) = metadata.agent_kind {
        push_u8(
            buffer,
            match agent_kind {
                DaemonAgentKind::Pi => 0,
                DaemonAgentKind::Claude => 1,
                DaemonAgentKind::Codex => 2,
                DaemonAgentKind::Terminal => 3,
                DaemonAgentKind::Verifier => 4,
            },
        );
    }
}

pub fn decode_daemon_session_metadata(
    decoder: &mut Decoder<'_>,
) -> Result<DaemonSessionMetadata, String> {
    let agent_uid = decoder.read_option(|decoder| decoder.read_string())?;
    let agent_label = decoder.read_option(|decoder| decoder.read_string())?;
    let agent_kind = decoder.read_option(|decoder| match decoder.read_u8()? {
        0 => Ok(DaemonAgentKind::Pi),
        1 => Ok(DaemonAgentKind::Claude),
        2 => Ok(DaemonAgentKind::Codex),
        3 => Ok(DaemonAgentKind::Terminal),
        4 => Ok(DaemonAgentKind::Verifier),
        tag => Err(format!("unknown daemon agent-kind tag {tag}")),
    })?;
    Ok(DaemonSessionMetadata {
        agent_uid,
        agent_label,
        agent_kind,
    })
}

pub fn encode_wire_terminal_command(buffer: &mut Vec<u8>, command: &TerminalCommand) {
    match command {
        TerminalCommand::InputText(text) => {
            push_u8(buffer, 0);
            push_string(buffer, text);
        }
        TerminalCommand::InputEvent(event) => {
            push_u8(buffer, 1);
            push_string(buffer, event);
        }
        TerminalCommand::SendCommand(command) => {
            push_u8(buffer, 2);
            push_string(buffer, command);
        }
        TerminalCommand::ScrollDisplay(delta) => {
            push_u8(buffer, 3);
            push_i32(buffer, *delta);
        }
        TerminalCommand::SetSelection { anchor, focus } => {
            push_u8(buffer, 4);
            push_usize(buffer, anchor.col);
            push_usize(buffer, anchor.row);
            push_usize(buffer, focus.col);
            push_usize(buffer, focus.row);
        }
        TerminalCommand::ClearSelection => {
            push_u8(buffer, 5);
        }
    }
}

pub fn decode_wire_terminal_command(decoder: &mut Decoder<'_>) -> Result<TerminalCommand, String> {
    match decoder.read_u8()? {
        0 => Ok(TerminalCommand::InputText(decoder.read_string()?)),
        1 => Ok(TerminalCommand::InputEvent(decoder.read_string()?)),
        2 => Ok(TerminalCommand::SendCommand(decoder.read_string()?)),
        3 => Ok(TerminalCommand::ScrollDisplay(decoder.read_i32()?)),
        4 => Ok(TerminalCommand::SetSelection {
            anchor: TerminalViewportPoint {
                col: decoder.read_usize()?,
                row: decoder.read_usize()?,
            },
            focus: TerminalViewportPoint {
                col: decoder.read_usize()?,
                row: decoder.read_usize()?,
            },
        }),
        5 => Ok(TerminalCommand::ClearSelection),
        tag => Err(format!("unknown terminal command tag {tag}")),
    }
}

pub fn encode_wire_runtime_state(buffer: &mut Vec<u8>, state: &TerminalRuntimeState) {
    push_string(buffer, &state.status);
    encode_wire_lifecycle(buffer, &state.lifecycle);
    push_bool(buffer, state.last_error.is_some());
    if let Some(error) = &state.last_error {
        push_string(buffer, error);
    }
}

pub fn decode_wire_runtime_state(
    decoder: &mut Decoder<'_>,
) -> Result<TerminalRuntimeState, String> {
    Ok(TerminalRuntimeState {
        status: decoder.read_string()?,
        lifecycle: decode_wire_lifecycle(decoder)?,
        last_error: if decoder.read_bool()? {
            Some(decoder.read_string()?)
        } else {
            None
        },
    })
}

pub(crate) fn encode_wire_lifecycle(buffer: &mut Vec<u8>, lifecycle: &TerminalLifecycle) {
    match lifecycle {
        TerminalLifecycle::Running => push_u8(buffer, 0),
        TerminalLifecycle::Exited { code, signal } => {
            push_u8(buffer, 1);
            push_option_u32(buffer, *code);
            push_bool(buffer, signal.is_some());
            if let Some(signal) = signal {
                push_string(buffer, signal);
            }
        }
        TerminalLifecycle::Disconnected => push_u8(buffer, 2),
        TerminalLifecycle::Failed => push_u8(buffer, 3),
    }
}

pub(crate) fn decode_wire_lifecycle(
    decoder: &mut Decoder<'_>,
) -> Result<TerminalLifecycle, String> {
    match decoder.read_u8()? {
        0 => Ok(TerminalLifecycle::Running),
        1 => Ok(TerminalLifecycle::Exited {
            code: decoder.read_option_u32()?,
            signal: if decoder.read_bool()? {
                Some(decoder.read_string()?)
            } else {
                None
            },
        }),
        2 => Ok(TerminalLifecycle::Disconnected),
        3 => Ok(TerminalLifecycle::Failed),
        tag => Err(format!("unknown lifecycle tag {tag}")),
    }
}

pub(crate) fn decode_result<T>(
    decoder: &mut Decoder<'_>,
    decode_ok: impl Fn(&mut Decoder<'_>) -> Result<T, String>,
) -> Result<Result<T, String>, String> {
    match decoder.read_u8()? {
        0 => Ok(Ok(decode_ok(decoder)?)),
        1 => Ok(Err(decoder.read_string()?)),
        tag => Err(format!("unknown result tag {tag}")),
    }
}

pub(crate) fn push_vec<T>(buffer: &mut Vec<u8>, items: &[T], encode: impl Fn(&mut Vec<u8>, &T)) {
    push_u32(buffer, u32::try_from(items.len()).unwrap_or(u32::MAX));
    for item in items {
        encode(buffer, item);
    }
}

pub(crate) fn push_bool(buffer: &mut Vec<u8>, value: bool) {
    push_u8(buffer, u8::from(value));
}

pub(crate) fn push_u8(buffer: &mut Vec<u8>, value: u8) {
    buffer.push(value);
}

pub(crate) fn push_i32(buffer: &mut Vec<u8>, value: i32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u64(buffer: &mut Vec<u8>, value: u64) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_option_u32(buffer: &mut Vec<u8>, value: Option<u32>) {
    push_bool(buffer, value.is_some());
    if let Some(value) = value {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
}

pub(crate) fn push_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_usize(buffer: &mut Vec<u8>, value: usize) {
    push_u64(buffer, value as u64);
}

pub(crate) fn push_string(buffer: &mut Vec<u8>, value: &str) {
    push_u32(buffer, u32::try_from(value.len()).unwrap_or(u32::MAX));
    buffer.extend_from_slice(value.as_bytes());
}

pub struct Decoder<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    pub fn finish(&self) -> Result<(), String> {
        if self.offset == self.input.len() {
            Ok(())
        } else {
            Err("trailing bytes after protocol message".to_owned())
        }
    }

    pub fn read_u8(&mut self) -> Result<u8, String> {
        let Some(value) = self.input.get(self.offset).copied() else {
            return Err("unexpected eof reading u8".to_owned());
        };
        self.offset += 1;
        Ok(value)
    }

    pub fn read_bool(&mut self) -> Result<bool, String> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(format!("invalid bool tag {value}")),
        }
    }

    pub fn read_i32(&mut self) -> Result<i32, String> {
        let bytes = self.read_array::<4>()?;
        Ok(i32::from_le_bytes(bytes))
    }

    pub fn read_u32(&mut self) -> Result<u32, String> {
        let bytes = self.read_array::<4>()?;
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn read_u64(&mut self) -> Result<u64, String> {
        let bytes = self.read_array::<8>()?;
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn read_usize(&mut self) -> Result<usize, String> {
        let value = self.read_u64()?;
        usize::try_from(value).map_err(|_| format!("usize out of range {value}"))
    }

    pub fn read_option_u32(&mut self) -> Result<Option<u32>, String> {
        if !self.read_bool()? {
            return Ok(None);
        }
        let bytes = self.read_array::<4>()?;
        Ok(Some(u32::from_le_bytes(bytes)))
    }

    pub fn read_char(&mut self) -> Result<char, String> {
        char::from_u32(self.read_u32()?).ok_or_else(|| "invalid char codepoint".to_owned())
    }

    pub fn read_string(&mut self) -> Result<String, String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|error| format!("invalid utf8 string: {error}"))
    }

    pub fn read_vec<T>(
        &mut self,
        decode: impl Fn(&mut Decoder<'_>) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let len = self.read_u32()? as usize;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(decode(self)?);
        }
        Ok(values)
    }

    pub fn read_option<T>(
        &mut self,
        decode: impl Fn(&mut Decoder<'_>) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        if self.read_bool()? {
            Ok(Some(decode(self)?))
        } else {
            Ok(None)
        }
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let bytes = self.read_bytes(N)?;
        let mut array = [0_u8; N];
        array.copy_from_slice(bytes);
        Ok(array)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "protocol length overflow".to_owned())?;
        let Some(bytes) = self.input.get(self.offset..end) else {
            return Err("unexpected eof reading bytes".to_owned());
        };
        self.offset = end;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        read_server_message, write_client_message, ClientMessage, DaemonRequest, DaemonResponse,
        DaemonSessionInfo, DaemonSessionMetadata, ServerMessage, TerminalCommand,
        TerminalLifecycle, TerminalRuntimeState,
    };
    use std::io::Cursor;

    #[test]
    fn subset_client_message_roundtrips_send_command() {
        let message = ClientMessage::Request {
            request_id: 7,
            request: DaemonRequest::SendCommand {
                session_id: "alpha".into(),
                command: TerminalCommand::SendCommand("echo hi".into()),
            },
        };
        let mut bytes = Vec::new();
        write_client_message(&mut bytes, &message).unwrap();
        let mut cursor = Cursor::new(bytes);
        let payload = super::read_frame(&mut cursor).unwrap();
        let mut decoder = super::Decoder::new(&payload);
        assert_eq!(decode_client_message(&mut decoder).unwrap(), message);
        decoder.finish().unwrap();
    }

    #[test]
    fn subset_server_message_roundtrips_session_list() {
        let mut bytes = Vec::new();
        let encoded = ServerMessage::Response {
            request_id: 4,
            response: Ok(DaemonResponse::SessionList {
                sessions: vec![DaemonSessionInfo {
                    session_id: "alpha".into(),
                    runtime: TerminalRuntimeState {
                        status: "running".into(),
                        lifecycle: TerminalLifecycle::Running,
                        last_error: None,
                    },
                    revision: 9,
                    created_order: 3,
                    metadata: DaemonSessionMetadata::default(),
                }],
            }),
        };
        let expected = ServerMessage::Response {
            request_id: 4,
            response: Ok(DaemonResponse::SessionList {
                sessions: vec![DaemonSessionInfo {
                    session_id: "alpha".into(),
                    runtime: TerminalRuntimeState {
                        status: "running".into(),
                        lifecycle: TerminalLifecycle::Running,
                        last_error: None,
                    },
                    revision: 9,
                    created_order: 0,
                    metadata: DaemonSessionMetadata::default(),
                }],
            }),
        };
        let mut payload = Vec::new();
        encode_server_message(&mut payload, &encoded);
        super::write_frame(&mut bytes, &payload).unwrap();
        assert_eq!(
            read_server_message(&mut Cursor::new(bytes)).unwrap(),
            expected
        );
    }

    #[test]
    fn subset_server_message_decodes_real_daemon_session_list_without_created_order() {
        let mut payload = Vec::new();
        super::push_u8(&mut payload, 0);
        super::push_u64(&mut payload, 11);
        super::push_u8(&mut payload, 0);
        super::push_u8(&mut payload, 1);
        super::push_u32(&mut payload, 1);
        super::push_string(&mut payload, "session-1");
        super::encode_wire_runtime_state(
            &mut payload,
            &TerminalRuntimeState {
                status: "running".into(),
                lifecycle: TerminalLifecycle::Running,
                last_error: None,
            },
        );
        super::push_u64(&mut payload, 21);

        let mut framed = Vec::new();
        super::write_frame(&mut framed, &payload).unwrap();
        let message = read_server_message(&mut Cursor::new(framed)).unwrap();
        assert_eq!(
            message,
            ServerMessage::Response {
                request_id: 11,
                response: Ok(DaemonResponse::SessionList {
                    sessions: vec![DaemonSessionInfo {
                        session_id: "session-1".into(),
                        runtime: TerminalRuntimeState {
                            status: "running".into(),
                            lifecycle: TerminalLifecycle::Running,
                            last_error: None,
                        },
                        revision: 21,
                        created_order: 0,
                        metadata: DaemonSessionMetadata::default(),
                    }],
                }),
            }
        );
    }

    fn encode_server_message(buffer: &mut Vec<u8>, message: &ServerMessage) {
        match message {
            ServerMessage::Response {
                request_id,
                response,
            } => {
                super::push_u8(buffer, 0);
                super::push_u64(buffer, *request_id);
                match response {
                    Ok(DaemonResponse::SessionList { sessions }) => {
                        super::push_u8(buffer, 0);
                        super::push_u8(buffer, 1);
                        super::push_u32(buffer, u32::try_from(sessions.len()).unwrap());
                        for session in sessions {
                            super::encode_wire_daemon_session_info_legacy(buffer, session);
                        }
                    }
                    Ok(DaemonResponse::SessionListDetailed { sessions }) => {
                        super::push_u8(buffer, 0);
                        super::push_u8(buffer, 8);
                        super::push_u32(buffer, u32::try_from(sessions.len()).unwrap());
                        for session in sessions {
                            super::encode_wire_daemon_session_info(buffer, session);
                        }
                    }
                    Ok(DaemonResponse::Ack) => {
                        super::push_u8(buffer, 0);
                        super::push_u8(buffer, 4);
                    }
                    Ok(DaemonResponse::OwnedTmuxSessionList { sessions }) => {
                        super::push_u8(buffer, 0);
                        super::push_u8(buffer, 5);
                        super::push_vec(buffer, sessions, super::encode_owned_tmux_session_info);
                    }
                    Ok(DaemonResponse::OwnedTmuxSessionCreated { session }) => {
                        super::push_u8(buffer, 0);
                        super::push_u8(buffer, 6);
                        super::encode_owned_tmux_session_info(buffer, session);
                    }
                    Err(error) => {
                        super::push_u8(buffer, 1);
                        super::push_string(buffer, error);
                    }
                }
            }
        }
    }

    fn decode_client_message(decoder: &mut super::Decoder<'_>) -> Result<ClientMessage, String> {
        match decoder.read_u8()? {
            0 => Ok(ClientMessage::Request {
                request_id: decoder.read_u64()?,
                request: match decoder.read_u8()? {
                    1 => DaemonRequest::ListSessions,
                    12 => DaemonRequest::ListSessionsDetailed,
                    4 => DaemonRequest::SendCommand {
                        session_id: decoder.read_string()?,
                        command: super::decode_wire_terminal_command(decoder)?,
                    },
                    tag => return Err(format!("unknown daemon request tag {tag}")),
                },
            }),
            tag => Err(format!("unknown client message tag {tag}")),
        }
    }
}
