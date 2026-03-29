mod agents;
mod app;
mod app_config;
mod composer;
mod conversations;
mod dialogs;
mod hud;
mod input;
mod startup;
mod terminals;
mod verification;

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;

use crate::{
    app::build_app,
    app_config::DEBUG_LOG_PATH,
    terminals::{append_debug_log, resolve_daemon_socket_path, run_daemon_server},
};
use std::{env, fs, path::PathBuf};

/// Clears the debug log if requested, then dispatches NeoZeus into app mode or daemon mode.
///
/// The binary treats `neozeus daemon ...` as a completely separate startup path; otherwise it builds
/// and runs the Bevy app.
fn main() {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if env::var("NEOZEUS_CLEAR_DEBUG_LOG")
        .map(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(true)
    {
        let _ = fs::write(DEBUG_LOG_PATH, "");
    }
    let args = env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|arg| arg == "daemon") {
        if let Err(error) = run_daemon_mode(&args[2..]) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    append_debug_log("app start");
    match build_app() {
        Ok(mut app) => {
            let _ = app.run();
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

/// Resolves the daemon socket path and starts the standalone daemon server.
///
/// Command-line `--socket` wins over environment/default resolution; failure to resolve any socket
/// path is reported as a user-facing error.
fn run_daemon_mode(args: &[String]) -> Result<(), String> {
    let socket_path = parse_daemon_socket_path(args)
        .or_else(resolve_daemon_socket_path)
        .ok_or_else(|| "failed to resolve daemon socket path".to_owned())?;
    append_debug_log(format!("daemon start socket={}", socket_path.display()));
    run_daemon_server(&socket_path)
}

/// Parses an explicit daemon socket path from `--socket <path>` command-line arguments.
///
/// Unknown flags are ignored; the function only cares about the first `--socket` occurrence.
fn parse_daemon_socket_path(args: &[String]) -> Option<PathBuf> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--socket" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}
