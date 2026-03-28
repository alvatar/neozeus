use crate::terminals::PtySession;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{ffi::OsString, io::Write};

#[cfg(test)]
use std::fs;

/// Allocates a new PTY pair and starts the configured shell inside it.
///
/// The function opens the PTY at the requested cell size, builds the shell command, forces
/// `TERM=xterm-256color`, spawns the child on the slave side, drops the slave handle, and returns a
/// [`PtySession`] containing the master, a writable handle, and the child process.
pub(crate) fn spawn_pty(cols: u16, rows: u16) -> Result<PtySession, String> {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
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
    apply_test_shell_isolation(&mut command);
    command
}

/// Returns the shell executable NeoZeus launches inside each PTY.
///
/// Today this is hard-coded to `zsh`. Keeping it behind a function makes the choice testable and
/// centralizes the one place that would need changing if the default shell policy ever changes.
fn raw_shell_program() -> OsString {
    OsString::from("zsh")
}

/// Rewrites the shell environment so tests run against an isolated temporary home/config tree.
///
/// This function is intentionally heavy-handed: it points HOME/XDG/ZDOTDIR/history-related variables
/// into a per-process temp directory, empties `.zshenv`, forces a minimal PATH, and disables common
/// shell startup hooks. The goal is deterministic test behavior regardless of the developer's real
/// shell configuration.
#[cfg(test)]
fn apply_test_shell_isolation(command: &mut CommandBuilder) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    let root = std::env::temp_dir().join(format!("neozeus-test-shell-{}", std::process::id()));
    let home = root.join("home");
    let xdg_config = root.join("xdg-config");
    let xdg_state = root.join("xdg-state");
    let xdg_cache = root.join("xdg-cache");
    let zdotdir = root.join("zdotdir");
    let kitty = root.join("kitty");
    let history = root.join("history");
    let zshenv = zdotdir.join(".zshenv");

    for dir in [&home, &xdg_config, &xdg_state, &xdg_cache, &zdotdir, &kitty] {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&zshenv, "");

    command.env("HOME", home.as_os_str());
    command.env("XDG_CONFIG_HOME", xdg_config.as_os_str());
    command.env("XDG_STATE_HOME", xdg_state.as_os_str());
    command.env("XDG_CACHE_HOME", xdg_cache.as_os_str());
    command.env("KITTY_CONFIG_DIRECTORY", kitty.as_os_str());
    command.env("ZDOTDIR", zdotdir.as_os_str());
    command.env("ZSHENV", zshenv.as_os_str());
    command.env("HISTFILE", history.as_os_str());
    command.env("BASH_ENV", "/dev/null");
    command.env("ENV", "/dev/null");
    command.env("SHELL", "/bin/zsh");
    command.env("PATH", "/usr/bin:/bin");
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
mod tests {
    use super::raw_shell_program;
    use std::ffi::OsString;

    /// Locks down the current default shell choice used by [`spawn_pty`].
    ///
    /// This is intentionally tiny, but it protects against accidentally changing the hard-coded shell
    /// executable without noticing the behavioral impact on spawned sessions.
    #[test]
    fn raw_shell_program_is_zsh() {
        assert_eq!(raw_shell_program(), OsString::from("zsh"));
    }
}
