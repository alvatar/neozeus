use super::types::PtySession;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{ffi::OsString, io::Write, path::PathBuf};

/// Allocates a new PTY pair and starts the configured shell inside it.
///
/// The function opens the PTY at the requested cell size, builds the shell command, forces
/// `TERM=xterm-256color`, applies an optional working directory, spawns the child on the slave side,
/// drops the slave handle, and returns a [`PtySession`] containing the master, a writable handle,
/// and the child process.
pub(crate) fn spawn_pty(cols: u16, rows: u16, cwd: Option<&str>) -> Result<PtySession, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("openpty failed: {error}"))?;

    let mut command = build_shell_command();
    command.env("TERM", "xterm-256color");
    if let Some(cwd) = resolve_shell_cwd(cwd)? {
        command.cwd(cwd);
    }

    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("spawn_command failed: {error}"))?;

    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("take_writer failed: {error}"))?;

    Ok(PtySession {
        master: pair.master,
        writer,
        child,
    })
}

/// Resolves the optional configured shell working directory into a concrete path.
///
/// Empty strings mean "use the process default". `~` and `~/...` are expanded against `$HOME`.
fn resolve_shell_cwd(raw: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if raw == "~" {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "cannot expand `~` without HOME".to_owned())?;
        return Ok(Some(home));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "cannot expand `~` without HOME".to_owned())?;
        return Ok(Some(home.join(rest)));
    }
    Ok(Some(PathBuf::from(raw)))
}

/// Builds the shell command used for newly spawned PTY sessions.
///
/// In production this is essentially just the raw shell program. In tests, the command is further
/// rewritten by [`apply_test_shell_isolation`] so the spawned shell cannot read or pollute the real
/// user environment.
fn build_shell_command() -> CommandBuilder {
    let command = CommandBuilder::new(raw_shell_program());
    #[cfg(test)]
    let mut command = command;
    #[cfg(test)]
    tests::apply_test_shell_isolation(&mut command);
    command
}

/// Returns the shell executable NeoZeus launches inside each PTY.
///
/// Today this is hard-coded to `zsh`. Keeping it behind a function makes the choice testable and
/// centralizes the one place that would need changing if the default shell policy ever changes.
fn raw_shell_program() -> OsString {
    OsString::from("zsh")
}

/// Writes a byte payload into the PTY and flushes it immediately.
///
/// Flushing on every write is intentional here because terminal input should be observed by the PTY
/// as soon as the command path emits it; batching is handled at higher layers when needed.
pub(crate) fn write_input(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}

#[cfg(test)]
mod tests;
