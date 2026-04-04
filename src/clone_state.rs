use crate::{
    shared::daemon_wire::{self, Decoder, TerminalRuntimeState as WireTerminalRuntimeState},
    terminals::{
        OwnedTmuxSessionInfo, TerminalCell, TerminalCellContent, TerminalCellStyle,
        TerminalCursor, TerminalCursorShape, TerminalSnapshot, TerminalSurface,
        TerminalUnderlineStyle,
    },
};
use bevy_egui::egui::Color32;
use std::{env, fs, path::Path};

pub(crate) const CLONED_DAEMON_STATE_ENV: &str = "NEOZEUS_CLONED_DAEMON_STATE_PATH";
pub(crate) const CLONED_DAEMON_STATE_FILENAME: &str = "cloned-daemon-state.v1";
const CLONED_DAEMON_STATE_MAGIC: &[u8] = b"neozeus cloned daemon state v1\n";

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ClonedDaemonState {
    pub(crate) sessions: Vec<ClonedDaemonSession>,
    pub(crate) owned_tmux_sessions: Vec<ClonedOwnedTmuxSession>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClonedDaemonSession {
    pub(crate) session_id: String,
    pub(crate) snapshot: TerminalSnapshot,
    pub(crate) revision: u64,
    pub(crate) order_index: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClonedOwnedTmuxSession {
    pub(crate) info: OwnedTmuxSessionInfo,
    pub(crate) capture_text: String,
}

pub(crate) fn resolve_cloned_daemon_state_path() -> Option<std::path::PathBuf> {
    env::var(CLONED_DAEMON_STATE_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

pub(crate) fn save_cloned_daemon_state(path: &Path, state: &ClonedDaemonState) -> Result<(), String> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(CLONED_DAEMON_STATE_MAGIC);
    push_u32(
        &mut bytes,
        u32::try_from(state.sessions.len())
            .map_err(|_| "too many cloned daemon sessions to encode".to_owned())?,
    );
    for session in &state.sessions {
        encode_cloned_session(&mut bytes, session);
    }
    push_u32(
        &mut bytes,
        u32::try_from(state.owned_tmux_sessions.len())
            .map_err(|_| "too many cloned tmux sessions to encode".to_owned())?,
    );
    for session in &state.owned_tmux_sessions {
        encode_cloned_owned_tmux_session(&mut bytes, session);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create clone-state dir {}: {error}", parent.display()))?;
    }
    fs::write(path, bytes)
        .map_err(|error| format!("failed to write cloned daemon state {}: {error}", path.display()))
}

pub(crate) fn load_cloned_daemon_state(path: &Path) -> Result<ClonedDaemonState, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read cloned daemon state {}: {error}", path.display()))?;
    if !bytes.starts_with(CLONED_DAEMON_STATE_MAGIC) {
        return Err(format!(
            "invalid cloned daemon state header {}",
            path.display()
        ));
    }
    let mut decoder = Decoder::new(&bytes[CLONED_DAEMON_STATE_MAGIC.len()..]);
    let sessions = decoder.read_vec(decode_cloned_session)?;
    let owned_tmux_sessions = decoder.read_vec(decode_cloned_owned_tmux_session)?;
    decoder.finish()?;
    Ok(ClonedDaemonState {
        sessions,
        owned_tmux_sessions,
    })
}

fn encode_cloned_session(buffer: &mut Vec<u8>, session: &ClonedDaemonSession) {
    push_string(buffer, &session.session_id);
    encode_snapshot(buffer, &session.snapshot);
    push_u64(buffer, session.revision);
    push_u64(buffer, session.order_index);
}

fn decode_cloned_session(decoder: &mut Decoder<'_>) -> Result<ClonedDaemonSession, String> {
    Ok(ClonedDaemonSession {
        session_id: decoder.read_string()?,
        snapshot: decode_snapshot(decoder)?,
        revision: decoder.read_u64()?,
        order_index: decoder.read_u64()?,
    })
}

fn encode_cloned_owned_tmux_session(buffer: &mut Vec<u8>, session: &ClonedOwnedTmuxSession) {
    encode_owned_tmux_session_info(buffer, &session.info);
    push_string(buffer, &session.capture_text);
}

fn decode_cloned_owned_tmux_session(
    decoder: &mut Decoder<'_>,
) -> Result<ClonedOwnedTmuxSession, String> {
    Ok(ClonedOwnedTmuxSession {
        info: decode_owned_tmux_session_info(decoder)?,
        capture_text: decoder.read_string()?,
    })
}

fn encode_snapshot(buffer: &mut Vec<u8>, snapshot: &TerminalSnapshot) {
    push_bool(buffer, snapshot.surface.is_some());
    if let Some(surface) = &snapshot.surface {
        encode_surface(buffer, surface);
    }
    daemon_wire::encode_wire_runtime_state(buffer, &encode_runtime_state(&snapshot.runtime));
}

fn decode_snapshot(decoder: &mut Decoder<'_>) -> Result<TerminalSnapshot, String> {
    Ok(TerminalSnapshot {
        surface: if decoder.read_bool()? {
            Some(decode_surface(decoder)?)
        } else {
            None
        },
        runtime: decode_runtime_state(daemon_wire::decode_wire_runtime_state(decoder)?),
    })
}

fn encode_surface(buffer: &mut Vec<u8>, surface: &TerminalSurface) {
    push_u64(buffer, surface.cols as u64);
    push_u64(buffer, surface.rows as u64);
    push_u32(buffer, surface.cells.len() as u32);
    for cell in &surface.cells {
        encode_cell(buffer, cell);
    }
    push_bool(buffer, surface.cursor.is_some());
    if let Some(cursor) = &surface.cursor {
        encode_cursor(buffer, cursor);
    }
}

fn decode_surface(decoder: &mut Decoder<'_>) -> Result<TerminalSurface, String> {
    let cols = decoder.read_usize()?;
    let rows = decoder.read_usize()?;
    let cells = decoder.read_vec(decode_cell)?;
    let cursor = if decoder.read_bool()? {
        Some(decode_cursor(decoder)?)
    } else {
        None
    };
    Ok(TerminalSurface {
        cols,
        rows,
        cells,
        cursor,
    })
}

fn encode_cell(buffer: &mut Vec<u8>, cell: &TerminalCell) {
    push_string(buffer, &cell.content.to_owned_string());
    push_color(buffer, cell.fg);
    push_color(buffer, cell.bg);
    push_bool(buffer, cell.style.bold);
    push_bool(buffer, cell.style.italic);
    push_bool(buffer, cell.style.dim);
    push_u8(buffer, encode_underline_style(cell.style.underline));
    push_bool(buffer, cell.style.strikeout);
    push_bool(buffer, cell.style.underline_color.is_some());
    if let Some(color) = cell.style.underline_color {
        push_color(buffer, color);
    }
    push_u8(buffer, cell.width);
}

fn decode_cell(decoder: &mut Decoder<'_>) -> Result<TerminalCell, String> {
    let content = decode_cell_content(&decoder.read_string()?);
    let fg = decode_color(decoder)?;
    let bg = decode_color(decoder)?;
    let bold = decoder.read_bool()?;
    let italic = decoder.read_bool()?;
    let dim = decoder.read_bool()?;
    let underline = decode_underline_style(decoder.read_u8()?)?;
    let strikeout = decoder.read_bool()?;
    let underline_color = if decoder.read_bool()? {
        Some(decode_color(decoder)?)
    } else {
        None
    };
    let width = decoder.read_u8()?;
    Ok(TerminalCell {
        content,
        fg,
        bg,
        style: TerminalCellStyle {
            bold,
            italic,
            dim,
            underline,
            strikeout,
            underline_color,
        },
        width,
    })
}

fn encode_cursor(buffer: &mut Vec<u8>, cursor: &TerminalCursor) {
    push_u64(buffer, cursor.x as u64);
    push_u64(buffer, cursor.y as u64);
    push_u8(buffer, encode_cursor_shape(cursor.shape));
    push_bool(buffer, cursor.visible);
    push_color(buffer, cursor.color);
}

fn decode_cursor(decoder: &mut Decoder<'_>) -> Result<TerminalCursor, String> {
    Ok(TerminalCursor {
        x: decoder.read_usize()?,
        y: decoder.read_usize()?,
        shape: decode_cursor_shape(decoder.read_u8()?)?,
        visible: decoder.read_bool()?,
        color: decode_color(decoder)?,
    })
}

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

fn encode_runtime_state(
    state: &crate::terminals::TerminalRuntimeState,
) -> WireTerminalRuntimeState {
    WireTerminalRuntimeState {
        status: state.status.clone(),
        lifecycle: match &state.lifecycle {
            crate::terminals::TerminalLifecycle::Running => daemon_wire::TerminalLifecycle::Running,
            crate::terminals::TerminalLifecycle::Exited { code, signal } => {
                daemon_wire::TerminalLifecycle::Exited {
                    code: *code,
                    signal: signal.clone(),
                }
            }
            crate::terminals::TerminalLifecycle::Disconnected => {
                daemon_wire::TerminalLifecycle::Disconnected
            }
            crate::terminals::TerminalLifecycle::Failed => daemon_wire::TerminalLifecycle::Failed,
        },
        last_error: state.last_error.clone(),
    }
}

fn decode_runtime_state(state: WireTerminalRuntimeState) -> crate::terminals::TerminalRuntimeState {
    crate::terminals::TerminalRuntimeState {
        status: state.status,
        lifecycle: match state.lifecycle {
            daemon_wire::TerminalLifecycle::Running => crate::terminals::TerminalLifecycle::Running,
            daemon_wire::TerminalLifecycle::Exited { code, signal } => {
                crate::terminals::TerminalLifecycle::Exited { code, signal }
            }
            daemon_wire::TerminalLifecycle::Disconnected => {
                crate::terminals::TerminalLifecycle::Disconnected
            }
            daemon_wire::TerminalLifecycle::Failed => crate::terminals::TerminalLifecycle::Failed,
        },
        last_error: state.last_error,
    }
}

fn decode_cell_content(text: &str) -> TerminalCellContent {
    let mut chars = text.chars();
    let Some(base) = chars.next() else {
        return TerminalCellContent::Empty;
    };
    let extra = chars.collect::<Vec<_>>();
    TerminalCellContent::from_parts(base, Some(&extra))
}

fn encode_underline_style(style: TerminalUnderlineStyle) -> u8 {
    match style {
        TerminalUnderlineStyle::None => 0,
        TerminalUnderlineStyle::Single => 1,
        TerminalUnderlineStyle::Double => 2,
        TerminalUnderlineStyle::Curly => 3,
        TerminalUnderlineStyle::Dotted => 4,
        TerminalUnderlineStyle::Dashed => 5,
    }
}

fn decode_underline_style(tag: u8) -> Result<TerminalUnderlineStyle, String> {
    match tag {
        0 => Ok(TerminalUnderlineStyle::None),
        1 => Ok(TerminalUnderlineStyle::Single),
        2 => Ok(TerminalUnderlineStyle::Double),
        3 => Ok(TerminalUnderlineStyle::Curly),
        4 => Ok(TerminalUnderlineStyle::Dotted),
        5 => Ok(TerminalUnderlineStyle::Dashed),
        _ => Err(format!("invalid underline style tag {tag}")),
    }
}

fn encode_cursor_shape(shape: TerminalCursorShape) -> u8 {
    match shape {
        TerminalCursorShape::Block => 0,
        TerminalCursorShape::Underline => 1,
        TerminalCursorShape::Beam => 2,
    }
}

fn decode_cursor_shape(tag: u8) -> Result<TerminalCursorShape, String> {
    match tag {
        0 => Ok(TerminalCursorShape::Block),
        1 => Ok(TerminalCursorShape::Underline),
        2 => Ok(TerminalCursorShape::Beam),
        _ => Err(format!("invalid cursor shape tag {tag}")),
    }
}

fn push_bool(buffer: &mut Vec<u8>, value: bool) {
    buffer.push(u8::from(value));
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
    push_u32(buffer, value.len() as u32);
    buffer.extend_from_slice(value.as_bytes());
}

fn push_color(buffer: &mut Vec<u8>, color: Color32) {
    buffer.extend_from_slice(&[color.r(), color.g(), color.b(), color.a()]);
}

fn decode_color(decoder: &mut Decoder<'_>) -> Result<Color32, String> {
    Ok(Color32::from_rgba_premultiplied(
        decoder.read_u8()?,
        decoder.read_u8()?,
        decoder.read_u8()?,
        decoder.read_u8()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> ClonedDaemonState {
        let mut surface = TerminalSurface::new(2, 2);
        surface.set_cell(
            0,
            0,
            TerminalCell {
                content: TerminalCellContent::from_parts('A', Some(&['B'])),
                fg: Color32::from_rgb(1, 2, 3),
                bg: Color32::from_rgb(4, 5, 6),
                style: TerminalCellStyle {
                    bold: true,
                    italic: true,
                    dim: false,
                    underline: TerminalUnderlineStyle::Curly,
                    strikeout: true,
                    underline_color: Some(Color32::from_rgb(7, 8, 9)),
                },
                width: 2,
            },
        );
        surface.cursor = Some(TerminalCursor {
            x: 1,
            y: 1,
            shape: TerminalCursorShape::Beam,
            visible: true,
            color: Color32::from_rgb(10, 11, 12),
        });
        ClonedDaemonState {
            sessions: vec![ClonedDaemonSession {
                session_id: "session-1".into(),
                snapshot: TerminalSnapshot {
                    surface: Some(surface),
                    runtime: crate::terminals::TerminalRuntimeState::running("running"),
                },
                revision: 7,
                order_index: 2,
            }],
            owned_tmux_sessions: vec![ClonedOwnedTmuxSession {
                info: OwnedTmuxSessionInfo {
                    session_uid: "tmux-1".into(),
                    owner_agent_uid: "agent-1".into(),
                    tmux_name: "neozeus-tmux-1".into(),
                    display_name: "BUILD".into(),
                    cwd: "/tmp/work".into(),
                    attached: false,
                    created_unix: 99,
                },
                capture_text: "line one\nline two".into(),
            }],
        }
    }

    #[test]
    fn cloned_daemon_state_roundtrips() {
        let path = std::env::temp_dir().join(format!(
            "neozeus-cloned-daemon-state-{}.bin",
            std::process::id()
        ));
        let state = sample_state();
        save_cloned_daemon_state(&path, &state).unwrap();
        let loaded = load_cloned_daemon_state(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(loaded, state);
    }
}
