use crate::{
    app_config::{DEFAULT_COLS, DEFAULT_ROWS},
    terminals::{TerminalAttachTarget, TerminalProvisionTarget},
};
use bevy::prelude::Resource;
use std::{
    ffi::OsString,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const PERSISTENT_TMUX_SESSION_PREFIX: &str = "neozeus-session-";
pub(crate) const VERIFIER_TMUX_SESSION_PREFIX: &str = "neozeus-verifier-";

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

#[derive(Resource, Clone)]
pub(crate) struct TmuxClientResource {
    session_client: Arc<dyn TerminalSessionClient>,
    pane_client: Arc<dyn TmuxPaneClient>,
}

impl TmuxClientResource {
    pub(crate) fn system() -> Self {
        let client = Arc::new(SystemTmuxClient);
        Self {
            session_client: client.clone(),
            pane_client: client,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_client<T>(client: Arc<T>) -> Self
    where
        T: TerminalSessionClient + TmuxPaneClient + 'static,
    {
        Self {
            session_client: client.clone(),
            pane_client: client,
        }
    }

    pub(crate) fn session_client(&self) -> &dyn TerminalSessionClient {
        self.session_client.as_ref()
    }

    pub(crate) fn shared_pane_client(&self) -> Arc<dyn TmuxPaneClient> {
        self.pane_client.clone()
    }
}

struct SystemTmuxClient;

impl TerminalSessionClient for SystemTmuxClient {
    fn ensure_tmux_available(&self) -> Result<(), String> {
        let output = Command::new("tmux")
            .arg("-V")
            .output()
            .map_err(|error| format!("failed to execute tmux -V: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(stderr_or_status(&output.stderr, output.status.code()))
        }
    }

    fn create_detached_session(&self, name: &str) -> Result<(), String> {
        for args in create_detached_session_tmux_commands(name) {
            run_tmux_os(&args).map(|_| ())?;
        }
        Ok(())
    }

    fn list_sessions(&self) -> Result<Vec<String>, String> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()
            .map_err(|error| format!("failed to execute tmux list-sessions: {error}"))?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect())
        } else if output.status.code() == Some(1)
            && String::from_utf8_lossy(&output.stderr).contains("no server running")
        {
            Ok(Vec::new())
        } else {
            Err(stderr_or_status(&output.stderr, output.status.code()))
        }
    }

    fn has_session(&self, name: &str) -> Result<bool, String> {
        let output = Command::new("tmux")
            .args(["has-session", "-t", name])
            .output()
            .map_err(|error| format!("failed to execute tmux has-session: {error}"))?;
        Ok(output.status.success())
    }

    fn kill_session(&self, name: &str) -> Result<(), String> {
        run_tmux(&["kill-session", "-t", name]).map(|_| ())
    }
}

impl TmuxPaneClient for SystemTmuxClient {
    fn list_panes(&self, session_name: &str) -> Result<Vec<TmuxPaneDescriptor>, String> {
        let output = run_tmux_os(&list_panes_tmux_command(session_name))?;
        Ok(output
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let pane_id = parts.next()?.trim();
                if pane_id.is_empty() {
                    return None;
                }
                let active = parts.next().and_then(|value| value.parse::<u8>().ok()) == Some(1);
                Some(TmuxPaneDescriptor {
                    pane_id: pane_id.to_owned(),
                    active,
                })
            })
            .collect())
    }

    fn pane_state(&self, pane_target: &str) -> Result<TmuxPaneState, String> {
        let output = run_tmux_os(&pane_state_tmux_command(pane_target))?;
        parse_pane_state_output(&output, pane_target)
    }

    fn capture_pane(&self, pane_target: &str, history_limit: usize) -> Result<String, String> {
        run_tmux_os(&capture_pane_tmux_command(pane_target, history_limit))
    }

    fn send_bytes(&self, pane_target: &str, bytes: &[u8]) -> Result<(), String> {
        for args in send_bytes_tmux_commands(pane_target, bytes) {
            run_tmux_os(&args)?;
        }
        Ok(())
    }
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

pub(crate) fn list_panes_tmux_command(session_name: &str) -> Vec<OsString> {
    vec![
        OsString::from("list-panes"),
        OsString::from("-t"),
        OsString::from(session_name),
        OsString::from("-F"),
        OsString::from("#{pane_id}\t#{pane_active}"),
    ]
}

pub(crate) fn capture_pane_tmux_command(pane_target: &str, history_limit: usize) -> Vec<OsString> {
    vec![
        OsString::from("capture-pane"),
        OsString::from("-p"),
        OsString::from("-e"),
        OsString::from("-N"),
        OsString::from("-S"),
        OsString::from(format!("-{}", history_limit.max(1))),
        OsString::from("-t"),
        OsString::from(pane_target),
    ]
}

pub(crate) fn pane_state_tmux_command(pane_target: &str) -> Vec<OsString> {
    vec![
        OsString::from("display-message"),
        OsString::from("-p"),
        OsString::from("-t"),
        OsString::from(pane_target),
        OsString::from("#{pane_width}\t#{pane_height}\t#{cursor_x}\t#{cursor_y}\t#{cursor_flag}"),
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

fn parse_pane_state_output(output: &str, pane_target: &str) -> Result<TmuxPaneState, String> {
    let mut state_parts = output.trim().split('\t');
    let cols = state_parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid tmux pane width for `{pane_target}`"))?;
    let rows = state_parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid tmux pane height for `{pane_target}`"))?;
    let cursor_x = state_parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid tmux cursor_x for `{pane_target}`"))?;
    let cursor_y = state_parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid tmux cursor_y for `{pane_target}`"))?;
    let cursor_visible = state_parts
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .is_some_and(|flag| flag != 0);

    Ok(TmuxPaneState {
        cols: cols.max(1),
        rows: rows.max(1),
        cursor_x,
        cursor_y,
        cursor_visible,
    })
}

fn run_tmux_os(args: &[OsString]) -> Result<String, String> {
    let output = Command::new("tmux").args(args).output().map_err(|error| {
        format!(
            "failed to execute tmux {}: {error}",
            args.iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        )
    })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(stderr_or_status(&output.stderr, output.status.code()))
    }
}

fn run_tmux(args: &[&str]) -> Result<String, String> {
    run_tmux_os(
        &args
            .iter()
            .map(|arg| OsString::from(*arg))
            .collect::<Vec<_>>(),
    )
}

fn stderr_or_status(stderr: &[u8], status_code: Option<i32>) -> String {
    let text = String::from_utf8_lossy(stderr).trim().to_owned();
    if text.is_empty() {
        format!("tmux exited with status {:?}", status_code)
    } else {
        text
    }
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
    OsString::from("/bin/sh")
}

#[cfg(not(test))]
fn raw_shell_program() -> OsString {
    std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("bash"))
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

pub(crate) fn resolve_tmux_active_pane_target(
    client: &dyn TmuxPaneClient,
    session_name: &str,
) -> Result<String, String> {
    let panes = client.list_panes(session_name)?;
    let mut first_pane = None;
    for pane in panes {
        if first_pane.is_none() {
            first_pane = Some(pane.pane_id.clone());
        }
        if pane.active {
            return Ok(pane.pane_id);
        }
    }
    first_pane.ok_or_else(|| format!("tmux session `{session_name}` has no panes"))
}

pub(crate) fn is_persistent_session_name(name: &str) -> bool {
    name.starts_with(PERSISTENT_TMUX_SESSION_PREFIX)
}
