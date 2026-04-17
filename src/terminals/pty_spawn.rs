use super::types::PtySession;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{ffi::OsString, io::Write, path::PathBuf};

/// Allocates a new PTY pair and starts the configured shell inside it.
///
/// The function opens the PTY at the requested cell size, builds the shell command, sets the
/// terminal environment NeoZeus expects (`TERM=xterm-kitty`, `COLORTERM=truecolor`), applies any
/// caller-supplied per-session environment overrides, applies an optional working directory, spawns
/// the child on the slave side, drops the slave handle, and returns a [`PtySession`] containing the
/// master, a writable handle, and the child process.
pub(crate) fn spawn_pty(
    cols: u16,
    rows: u16,
    cwd: Option<&str>,
    env_overrides: &[(String, String)],
) -> Result<PtySession, String> {
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
    command.env("TERM", "xterm-kitty");
    command.env("COLORTERM", "truecolor");
    for (key, value) in env_overrides {
        command.env(key, value);
    }
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
mod tests {
    use super::*;
    use std::{
        ffi::OsString,
        fs,
        io::Read,
        path::PathBuf,
        time::{Duration, Instant},
    };

    /// Rewrites the shell environment so tests run against an isolated temporary home/config tree.
    ///
    /// This function is intentionally heavy-handed: it points HOME/XDG/ZDOTDIR/history-related variables
    /// into a per-process temp directory, empties `.zshenv`, forces a minimal PATH, and disables common
    /// shell startup hooks. The goal is deterministic test behavior regardless of the developer's real
    /// shell configuration.
    pub(super) fn apply_test_shell_isolation(command: &mut CommandBuilder) {
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

    /// Locks down the current default shell choice used by [`spawn_pty`].
    ///
    /// This is intentionally tiny, but it protects against accidentally changing the hard-coded shell
    /// executable without noticing the behavioral impact on spawned sessions.
    #[test]
    fn raw_shell_program_is_zsh() {
        assert_eq!(raw_shell_program(), OsString::from("zsh"));
    }

    /// Verifies that configured shell working directories expand `~` and ignore empty input.
    #[test]
    fn resolve_shell_cwd_expands_home_and_ignores_empty_values() {
        let home = std::env::temp_dir().join("neozeus-pty-home-test");
        std::env::set_var("HOME", &home);

        assert_eq!(resolve_shell_cwd(None).unwrap(), None);
        assert_eq!(resolve_shell_cwd(Some("  ")).unwrap(), None);
        assert_eq!(resolve_shell_cwd(Some("~")).unwrap(), Some(home.clone()));
        assert_eq!(
            resolve_shell_cwd(Some("~/code")).unwrap(),
            Some(home.join("code"))
        );
        assert_eq!(
            resolve_shell_cwd(Some("/tmp/work")).unwrap(),
            Some(PathBuf::from("/tmp/work"))
        );
    }

    /// Verifies that per-session env overrides are present from shell process start.
    #[test]
    fn spawn_pty_applies_env_overrides() {
        let mut session = spawn_pty(
            80,
            24,
            None,
            &[("NEOZEUS_AGENT_UID".into(), "agent-uid-test".into())],
        )
        .expect("pty should spawn");
        let mut reader = session
            .master
            .try_clone_reader()
            .expect("reader should clone");

        write_input(
            &mut *session.writer,
            b"printf 'env:%s' \"$NEOZEUS_AGENT_UID\"\r",
        )
        .expect("env command should write");

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut output = String::new();
        while Instant::now() < deadline && !output.contains("env:agent-uid-test") {
            let mut buffer = [0_u8; 1024];
            let read = reader.read(&mut buffer).expect("pty read should succeed");
            if read == 0 {
                break;
            }
            output.push_str(&String::from_utf8_lossy(&buffer[..read]));
        }

        write_input(&mut *session.writer, b"exit\r").expect("exit should write");
        let _ = session.child.wait();
        assert!(
            output.contains("env:agent-uid-test"),
            "expected env override in shell output, got: {output:?}"
        );
    }
}
