use crate::{
    app_config::{DEFAULT_COLS, DEFAULT_ROWS},
    terminals::{TerminalAttachTarget, TerminalProvisionTarget},
};
use std::{
    ffi::OsString,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const PERSISTENT_TMUX_SESSION_PREFIX: &str = "neozeus-session-";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TmuxPaneDescriptor {
    pub(crate) pane_id: String,
    pub(crate) active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TmuxPaneState {
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    pub(crate) cursor_x: usize,
    pub(crate) cursor_y: usize,
    pub(crate) cursor_visible: bool,
}

pub(crate) trait TerminalSessionClient: Send + Sync {
    fn ensure_tmux_available(&self) -> Result<(), String>;
    fn create_detached_session(&self, name: &str) -> Result<(), String>;
    fn list_sessions(&self) -> Result<Vec<String>, String>;
    fn has_session(&self, name: &str) -> Result<bool, String>;
    fn kill_session(&self, name: &str) -> Result<(), String>;
}

pub(crate) trait TmuxPaneClient: Send + Sync {
    fn list_panes(&self, session_name: &str) -> Result<Vec<TmuxPaneDescriptor>, String>;
    fn pane_state(&self, pane_target: &str) -> Result<TmuxPaneState, String>;
    fn capture_pane(&self, pane_target: &str, history_limit: usize) -> Result<String, String>;
    fn send_bytes(&self, pane_target: &str, bytes: &[u8]) -> Result<(), String>;
}

pub(crate) fn create_detached_session_tmux_commands(name: &str) -> Vec<Vec<OsString>> {
    vec![
        vec![
            OsString::from("new-session"),
            OsString::from("-d"),
            OsString::from("-x"),
            OsString::from(DEFAULT_COLS.to_string()),
            OsString::from("-y"),
            OsString::from(DEFAULT_ROWS.to_string()),
            OsString::from("-s"),
            OsString::from(name),
        ],
        vec![
            OsString::from("set-option"),
            OsString::from("-t"),
            OsString::from(name),
            OsString::from("destroy-unattached"),
            OsString::from("off"),
        ],
        vec![
            OsString::from("set-option"),
            OsString::from("-t"),
            OsString::from(name),
            OsString::from("status"),
            OsString::from("off"),
        ],
    ]
}

pub(crate) fn send_bytes_tmux_commands(pane_target: &str, bytes: &[u8]) -> Vec<Vec<OsString>> {
    let mut commands = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        if matches!(bytes[start], 0x00..=0x1f | 0x7f) {
            commands.push(vec![
                OsString::from("send-keys"),
                OsString::from("-t"),
                OsString::from(pane_target),
                OsString::from("-H"),
                OsString::from(format!("{:02x}", bytes[start])),
            ]);
            start += 1;
            continue;
        }

        let mut end = start;
        while end < bytes.len() && !matches!(bytes[end], 0x00..=0x1f | 0x7f) {
            end += 1;
        }
        let text = String::from_utf8_lossy(&bytes[start..end]).into_owned();
        commands.push(vec![
            OsString::from("send-keys"),
            OsString::from("-t"),
            OsString::from(pane_target),
            OsString::from("-l"),
            OsString::from(text),
        ]);
        start = end;
    }
    commands
}

pub(crate) fn provision_terminal_target(
    client: &dyn TerminalSessionClient,
    target: &TerminalProvisionTarget,
) -> Result<(), String> {
    match target {
        TerminalProvisionTarget::RawShell => Ok(()),
        TerminalProvisionTarget::TmuxDetached { session_name } => {
            client.ensure_tmux_available()?;
            client.create_detached_session(session_name)
        }
    }
}

pub(crate) fn generate_unique_session_name(
    client: &dyn TerminalSessionClient,
    prefix: &str,
) -> Result<String, String> {
    client.ensure_tmux_available()?;
    let process_id = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_nanos();

    for attempt in 0..32_u32 {
        let candidate = format!("{prefix}{process_id:x}-{nanos:x}-{attempt:x}");
        if !client.has_session(&candidate)? {
            return Ok(candidate);
        }
    }

    Err(format!(
        "failed to allocate unique tmux session name for prefix `{prefix}`"
    ))
}

#[cfg(test)]
fn raw_shell_program() -> OsString {
    OsString::from("zsh")
}

#[cfg(not(test))]
fn raw_shell_program() -> OsString {
    OsString::from("zsh")
}

pub(crate) fn build_attach_command_argv(
    target: &TerminalAttachTarget,
) -> (OsString, Vec<OsString>) {
    match target {
        TerminalAttachTarget::RawShell => (raw_shell_program(), Vec::new()),
        TerminalAttachTarget::TmuxAttach { session_name } => (
            OsString::from("tmux"),
            vec![
                OsString::from("attach-session"),
                OsString::from("-t"),
                OsString::from(session_name),
            ],
        ),
    }
}
