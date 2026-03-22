use crate::terminals::{build_attach_command_argv, PtySession, TerminalAttachTarget};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Write;

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
    command
}

pub(crate) fn write_input(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}
