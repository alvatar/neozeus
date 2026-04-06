use crate::shared::{
    command_runner::run_command_with_timeout, labels::uppercase_owned_tmux_display_name_text,
};
use std::{
    collections::HashMap,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) const OWNED_TMUX_BACKEND_TAG: &str = "agent-owned-tmux";
const TMUX_OPTION_BACKEND: &str = "@neozeus_backend";
const TMUX_OPTION_SESSION_UID: &str = "@neozeus_id";
const TMUX_OPTION_OWNER_AGENT_UID: &str = "@neozeus_owner_agent_uid";
const TMUX_OPTION_DISPLAY_NAME: &str = "@neozeus_name";
const TMUX_OPTION_CREATED_BY: &str = "@neozeus_created_by";
const TMUX_CREATED_BY_VALUE: &str = "neozeus";
const TMUX_DISCOVER_TIMEOUT: Duration = Duration::from_secs(3);
const TMUX_MUTATION_TIMEOUT: Duration = Duration::from_secs(5);
static NEXT_OWNED_TMUX_UID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn tmux_program() -> std::ffi::OsString {
    #[cfg(test)]
    if let Some(program) = std::env::var_os("NEOZEUS_TEST_TMUX") {
        return program;
    }
    std::ffi::OsString::from("tmux")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnedTmuxSessionInfo {
    pub(crate) session_uid: String,
    pub(crate) owner_agent_uid: String,
    pub(crate) tmux_name: String,
    pub(crate) display_name: String,
    pub(crate) cwd: String,
    pub(crate) attached: bool,
    pub(crate) created_unix: u64,
}

pub(super) fn discover_owned_tmux_sessions() -> Result<Vec<OwnedTmuxSessionInfo>, String> {
    let listed = run_tmux(
        [
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_attached}\t#{session_created}",
        ],
        TMUX_DISCOVER_TIMEOUT,
    )?;
    let mut sessions = Vec::new();
    for line in listed.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.split('\t');
        let Some(tmux_name) = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let attached = parts.next().map(str::trim).unwrap_or_default() != "0";
        let created_unix = parts
            .next()
            .map(str::trim)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let backend = match read_tmux_option(tmux_name, TMUX_OPTION_BACKEND) {
            Ok(backend) => backend,
            Err(error) if is_tmux_missing_error(&error) => continue,
            Err(error) => return Err(error),
        };
        if backend != OWNED_TMUX_BACKEND_TAG {
            continue;
        }

        let (start_command, cwd) = match pane_start_command_and_cwd(tmux_name) {
            Ok(values) => values,
            Err(error) if is_tmux_missing_error(&error) => continue,
            Err(error) => return Err(error),
        };
        let session_uid = match read_tmux_option(tmux_name, TMUX_OPTION_SESSION_UID) {
            Ok(value) => value,
            Err(error) if is_tmux_missing_error(&error) => continue,
            Err(error) => return Err(error),
        };
        let owner_agent_uid = match read_tmux_option(tmux_name, TMUX_OPTION_OWNER_AGENT_UID) {
            Ok(value) => value,
            Err(error) if is_tmux_missing_error(&error) => continue,
            Err(error) => return Err(error),
        };
        let display_name = match read_tmux_option(tmux_name, TMUX_OPTION_DISPLAY_NAME) {
            Ok(value) => value,
            Err(error) if is_tmux_missing_error(&error) => continue,
            Err(error) => return Err(error),
        };

        let session_uid = if session_uid.trim().is_empty() {
            extract_env_assignment(&start_command, "NEOZEUS_TMUX_UID")
        } else {
            session_uid.trim().to_owned()
        };
        let owner_agent_uid = if owner_agent_uid.trim().is_empty() {
            extract_env_assignment(&start_command, "NEOZEUS_AGENT_UID")
        } else {
            owner_agent_uid.trim().to_owned()
        };
        if session_uid.is_empty() || owner_agent_uid.is_empty() {
            continue;
        }

        sessions.push(OwnedTmuxSessionInfo {
            session_uid,
            owner_agent_uid,
            tmux_name: tmux_name.to_owned(),
            display_name: normalize_owned_tmux_display_name(display_name.trim(), tmux_name),
            cwd,
            attached,
            created_unix,
        });
    }
    sessions.sort_by(|left, right| {
        left.created_unix
            .cmp(&right.created_unix)
            .then_with(|| left.tmux_name.cmp(&right.tmux_name))
    });
    Ok(sessions)
}

fn normalize_owned_tmux_display_name(display_name: &str, fallback_tmux_name: &str) -> String {
    if display_name.is_empty() {
        fallback_tmux_name.to_owned()
    } else {
        uppercase_owned_tmux_display_name_text(display_name)
    }
}

pub(super) fn create_owned_tmux_session(
    owner_agent_uid: &str,
    display_name: &str,
    cwd: Option<&str>,
    command: &str,
) -> Result<OwnedTmuxSessionInfo, String> {
    let owner_agent_uid = owner_agent_uid.trim();
    if owner_agent_uid.is_empty() {
        return Err("owned tmux session requires owner agent uid".to_owned());
    }
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err("owned tmux session requires a display name".to_owned());
    }
    let display_name = uppercase_owned_tmux_display_name_text(display_name);
    let command = command.trim();
    if command.is_empty() {
        return Err("owned tmux session requires a command".to_owned());
    }

    let session_uid = generate_owned_tmux_session_uid();
    let tmux_name = owned_tmux_session_name(&session_uid);
    let shell_script = format!("{command}; exec zsh -il");
    let start_command = format!(
        "NEOZEUS_AGENT_UID={} NEOZEUS_TMUX_UID={} exec zsh -ilc {}",
        shell_quote(owner_agent_uid),
        shell_quote(&session_uid),
        shell_quote(&shell_script),
    );

    let mut command_args = vec![
        "new-session".to_owned(),
        "-d".to_owned(),
        "-s".to_owned(),
        tmux_name.clone(),
    ];
    if let Some(cwd) = cwd.map(str::trim).filter(|value| !value.is_empty()) {
        command_args.push("-c".to_owned());
        command_args.push(cwd.to_owned());
    }
    command_args.push(start_command);
    run_tmux_owned(command_args, TMUX_MUTATION_TIMEOUT)?;

    let option_values = [
        (TMUX_OPTION_BACKEND, OWNED_TMUX_BACKEND_TAG),
        (TMUX_OPTION_SESSION_UID, session_uid.as_str()),
        (TMUX_OPTION_OWNER_AGENT_UID, owner_agent_uid),
        (TMUX_OPTION_DISPLAY_NAME, display_name.as_str()),
        (TMUX_OPTION_CREATED_BY, TMUX_CREATED_BY_VALUE),
    ];
    for (option, value) in option_values {
        if let Err(error) = set_tmux_option(&tmux_name, option, value) {
            let _ = run_tmux_owned(
                vec![
                    "kill-session".to_owned(),
                    "-t".to_owned(),
                    tmux_name.clone(),
                ],
                TMUX_MUTATION_TIMEOUT,
            );
            return Err(error);
        }
    }

    let created_fallback = OwnedTmuxSessionInfo {
        session_uid: session_uid.clone(),
        owner_agent_uid: owner_agent_uid.to_owned(),
        tmux_name: tmux_name.clone(),
        display_name,
        cwd: cwd.unwrap_or_default().trim().to_owned(),
        attached: false,
        created_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
    };

    for _ in 0..10 {
        if let Some(session) = discover_owned_tmux_sessions()?
            .into_iter()
            .find(|session| session.session_uid == session_uid)
        {
            return Ok(session);
        }
        thread::sleep(Duration::from_millis(20));
    }

    Ok(created_fallback)
}

pub(super) fn capture_owned_tmux_session(
    session_uid: &str,
    lines: usize,
) -> Result<String, String> {
    let session = discover_owned_tmux_sessions()?
        .into_iter()
        .find(|session| session.session_uid == session_uid)
        .ok_or_else(|| format!("owned tmux session `{session_uid}` not found"))?;
    let start = format!("-{}", lines.max(1));
    run_tmux(
        [
            "capture-pane",
            "-t",
            session.tmux_name.as_str(),
            "-p",
            "-e",
            "-S",
            start.as_str(),
        ],
        TMUX_DISCOVER_TIMEOUT,
    )
}

pub(super) fn kill_owned_tmux_session(session_uid: &str) -> Result<(), String> {
    let session = discover_owned_tmux_sessions()?
        .into_iter()
        .find(|session| session.session_uid == session_uid)
        .ok_or_else(|| format!("owned tmux session `{session_uid}` not found"))?;
    kill_discovered_owned_tmux_session(&session)
}

pub(super) fn kill_owned_tmux_sessions_for_agent(owner_agent_uid: &str) -> Result<(), String> {
    let sessions = discover_owned_tmux_sessions()?;
    let mut failures = Vec::new();
    for session in sessions
        .into_iter()
        .filter(|session| session.owner_agent_uid == owner_agent_uid)
    {
        if let Err(error) = kill_discovered_owned_tmux_session(&session) {
            failures.push(format!("{}: {error}", session.tmux_name));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "failed to kill owned tmux sessions for {owner_agent_uid}: {}",
            failures.join(", ")
        ))
    }
}

pub(super) fn owned_tmux_sessions_by_uid() -> Result<HashMap<String, OwnedTmuxSessionInfo>, String>
{
    Ok(discover_owned_tmux_sessions()?
        .into_iter()
        .map(|session| (session.session_uid.clone(), session))
        .collect())
}

fn generate_owned_tmux_session_uid() -> String {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = NEXT_OWNED_TMUX_UID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tmux-{now_nanos:032x}-{counter:016x}")
}

fn owned_tmux_session_name(session_uid: &str) -> String {
    let mut compact = session_uid
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    if compact.len() > 24 {
        compact = compact[compact.len() - 24..].to_owned();
    }
    if compact.is_empty() {
        compact = "session".to_owned();
    }
    format!("neozeus-tmux-{compact}")
}

fn kill_discovered_owned_tmux_session(session: &OwnedTmuxSessionInfo) -> Result<(), String> {
    let pane_pid = pane_pid(&session.tmux_name).ok().flatten();
    if let Some(pid) = pane_pid {
        let _ = signal_process_group(pid, "TERM");
        let _ = wait_for_process_exit_until(
            pid,
            std::time::Instant::now() + Duration::from_millis(200),
        );
    }
    match run_tmux(
        ["kill-session", "-t", session.tmux_name.as_str()],
        TMUX_MUTATION_TIMEOUT,
    ) {
        Ok(_) => {}
        Err(error) if is_tmux_missing_error(&error) => {}
        Err(error) => return Err(error),
    }
    if let Some(pid) = pane_pid {
        wait_for_process_exit(pid, TMUX_MUTATION_TIMEOUT)?;
    }
    Ok(())
}

fn pane_start_command_and_cwd(tmux_name: &str) -> Result<(String, String), String> {
    let pane = run_tmux(
        [
            "list-panes",
            "-t",
            tmux_name,
            "-F",
            "#{pane_start_command}\t#{pane_current_path}",
        ],
        TMUX_DISCOVER_TIMEOUT,
    )?;
    let mut first_line = pane.lines();
    let Some(line) = first_line.next() else {
        return Ok((String::new(), String::new()));
    };
    let mut parts = line.split('\t');
    let start_command = parts.next().unwrap_or_default().trim().to_owned();
    let cwd = parts.next().unwrap_or_default().trim().to_owned();
    Ok((start_command, cwd))
}

fn pane_pid(tmux_name: &str) -> Result<Option<u32>, String> {
    let pane = run_tmux(
        ["list-panes", "-t", tmux_name, "-F", "#{pane_pid}"],
        TMUX_DISCOVER_TIMEOUT,
    )?;
    Ok(pane
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<u32>().ok()))
}

fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    if wait_for_process_exit_until(pid, deadline) {
        return Ok(());
    }

    let _ = signal_process_group(pid, "TERM");
    let term_deadline = std::time::Instant::now() + Duration::from_millis(500);
    if wait_for_process_exit_until(pid, term_deadline.min(deadline)) {
        return Ok(());
    }

    let _ = signal_process_group(pid, "KILL");
    if wait_for_process_exit_until(pid, deadline) {
        return Ok(());
    }

    Err(format!(
        "tmux pane pid {pid} still alive after kill-session"
    ))
}

fn wait_for_process_exit_until(pid: u32, deadline: std::time::Instant) -> bool {
    while std::time::Instant::now() < deadline {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    !std::path::Path::new(&format!("/proc/{pid}")).exists()
}

fn signal_process_group(pid: u32, signal: &str) -> Result<(), String> {
    let output = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{pid}"))
        .output()
        .map_err(|error| format!("kill -{signal} -- -{pid} failed: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        Err(if detail.is_empty() {
            format!(
                "kill -{signal} -- -{pid} exited with status {}",
                output.status
            )
        } else {
            format!("kill -{signal} -- -{pid} failed: {detail}")
        })
    }
}

fn read_tmux_option(tmux_name: &str, option: &str) -> Result<String, String> {
    run_tmux(
        ["show-options", "-t", tmux_name, "-qv", option],
        TMUX_DISCOVER_TIMEOUT,
    )
    .map(|value| value.trim().to_owned())
    .or_else(|error| {
        if error.contains("invalid option") || error.contains("unknown option") {
            Ok(String::new())
        } else {
            Err(error)
        }
    })
}

fn set_tmux_option(tmux_name: &str, option: &str, value: &str) -> Result<(), String> {
    run_tmux(
        ["set-option", "-t", tmux_name, option, value],
        TMUX_MUTATION_TIMEOUT,
    )
    .map(|_| ())
}

fn extract_env_assignment(command: &str, key: &str) -> String {
    let needle = format!("{key}=");
    let Some(start) = command.find(&needle) else {
        return String::new();
    };
    let raw = &command[start + needle.len()..];
    let value = raw
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches('"')
        .trim_matches('\'');
    value.to_owned()
}

fn is_tmux_missing_error(error: &str) -> bool {
    error.contains("can't find") || error.contains("not found")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_tmux<const N: usize>(args: [&str; N], timeout: Duration) -> Result<String, String> {
    run_tmux_owned(args.into_iter().map(str::to_owned).collect(), timeout)
}

fn run_tmux_owned(args: Vec<String>, timeout: Duration) -> Result<String, String> {
    let mut command = Command::new(tmux_program());
    command.args(&args);
    let output = run_command_with_timeout(&mut command, timeout, true)
        .map_err(|error| format!("tmux {:?} failed: {error}", args))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(if detail.is_empty() {
            format!("tmux {:?} exited with status {}", args, output.status)
        } else {
            format!("tmux {:?} failed: {detail}", args)
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::run_tmux_owned;
    use std::{
        path::PathBuf,
        process::Command,
        sync::{Mutex, OnceLock},
        time::Duration,
    };

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "neozeus-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn run_tmux_owned_times_out() {
        let _guard = env_lock().lock().unwrap();
        let dir = temp_dir("owned-tmux-timeout");
        let fake_tmux = dir.join("tmux-timeout.sh");
        std::fs::write(&fake_tmux, "#!/bin/sh\nsleep 1\n").unwrap();
        let output = Command::new("chmod")
            .arg("+x")
            .arg(&fake_tmux)
            .output()
            .unwrap();
        assert!(output.status.success());
        std::env::set_var("NEOZEUS_TEST_TMUX", &fake_tmux);
        let error = run_tmux_owned(vec!["list-sessions".into()], Duration::from_millis(1))
            .expect_err("tmux timeout should fail");
        std::env::remove_var("NEOZEUS_TEST_TMUX");
        assert!(error.contains("timed out"));
    }

    #[test]
    fn normalize_owned_tmux_display_name_uppercases_non_empty_names() {
        assert_eq!(
            super::normalize_owned_tmux_display_name("build bot", "neozeus-tmux-1"),
            "BUILD BOT"
        );
        assert_eq!(
            super::normalize_owned_tmux_display_name("", "neozeus-tmux-1"),
            "neozeus-tmux-1"
        );
    }
}
