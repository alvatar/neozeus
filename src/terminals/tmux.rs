use crate::terminals::{TerminalAttachTarget, TerminalProvisionTarget};
use bevy::prelude::Resource;
use std::{
    ffi::OsString,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const PERSISTENT_TMUX_SESSION_PREFIX: &str = "neozeus-session-";
pub(crate) const VERIFIER_TMUX_SESSION_PREFIX: &str = "neozeus-verifier-";

pub(crate) trait TmuxClient: Send + Sync {
    fn ensure_tmux_available(&self) -> Result<(), String>;
    fn create_detached_session(&self, name: &str) -> Result<(), String>;
    fn list_sessions(&self) -> Result<Vec<String>, String>;
    fn has_session(&self, name: &str) -> Result<bool, String>;
    fn kill_session(&self, name: &str) -> Result<(), String>;
}

#[derive(Resource, Clone)]
pub(crate) struct TmuxClientResource {
    client: Arc<dyn TmuxClient>,
}

impl TmuxClientResource {
    pub(crate) fn system() -> Self {
        Self {
            client: Arc::new(SystemTmuxClient),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_client(client: Arc<dyn TmuxClient>) -> Self {
        Self { client }
    }

    pub(crate) fn client(&self) -> &dyn TmuxClient {
        self.client.as_ref()
    }
}

struct SystemTmuxClient;

impl TmuxClient for SystemTmuxClient {
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

pub(crate) fn create_detached_session_tmux_commands(name: &str) -> Vec<Vec<OsString>> {
    vec![
        vec![
            OsString::from("new-session"),
            OsString::from("-d"),
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
    ]
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
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
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
    client: &dyn TmuxClient,
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
    client: &dyn TmuxClient,
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

pub(crate) fn build_attach_command_argv(
    target: &TerminalAttachTarget,
) -> (OsString, Vec<OsString>) {
    match target {
        TerminalAttachTarget::RawShell => (
            std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("bash")),
            Vec::new(),
        ),
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

pub(crate) fn is_persistent_session_name(name: &str) -> bool {
    name.starts_with(PERSISTENT_TMUX_SESSION_PREFIX)
}
