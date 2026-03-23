use crate::terminals::{build_attach_command_argv, PtySession, TerminalAttachTarget};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Write;

#[cfg(test)]
use std::fs;

pub(crate) fn spawn_pty(
    cols: u16,
    rows: u16,
    target: &TerminalAttachTarget,
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

    let mut command = build_attach_command(target);
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

fn build_attach_command(target: &TerminalAttachTarget) -> CommandBuilder {
    let (program, args) = build_attach_command_argv(target);
    let mut command = CommandBuilder::new(program);
    for arg in args {
        command.arg(arg);
    }
    #[cfg(test)]
    if matches!(target, TerminalAttachTarget::RawShell) {
        apply_test_shell_isolation(&mut command);
    }
    command
}

#[cfg(test)]
fn apply_test_shell_isolation(command: &mut CommandBuilder) {
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
    command.env("SHELL", "/bin/sh");
    command.env("PATH", "/usr/bin:/bin");
}

pub(crate) fn write_input(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}
