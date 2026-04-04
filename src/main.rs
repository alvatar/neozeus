mod agents;
mod app;
mod app_config;
mod composer;
mod clone_state;
mod conversations;
mod dialogs;
mod hud;
mod input;
pub use neozeus::shared;
mod startup;
mod terminals;
mod text_selection;
mod usage;
mod verification;

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;

use crate::{
    app::build_app,
    app_config::DEBUG_LOG_PATH,
    clone_state::{
        save_cloned_daemon_state, ClonedDaemonSession, ClonedDaemonState,
        ClonedOwnedTmuxSession, CLONED_DAEMON_STATE_FILENAME, CLONED_DAEMON_STATE_ENV,
    },
    terminals::{append_debug_log, resolve_daemon_socket_path, run_daemon_server},
};
use std::{env, fs, path::{Path, PathBuf}};

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
    if args.get(1).is_some_and(|arg| arg == "clone-state") {
        if let Err(error) = run_clone_state_mode(&args[2..]) {
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

fn run_clone_state_mode(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("capture-current") => capture_current_state_clone(&args[1..]),
        _ => Err(
            "usage: neozeus clone-state capture-current --out-root <dir> [--socket <path>] [--home <dir>] [--xdg-config-home <dir>] [--xdg-state-home <dir>] [--xdg-cache-home <dir>]".to_owned(),
        ),
    }
}

fn capture_current_state_clone(args: &[String]) -> Result<(), String> {
    use crate::terminals::{SocketTerminalDaemonClient, TerminalDaemonClient};

    let request = parse_clone_current_request(args)?;
    let state_home = resolve_state_home(request.xdg_state_home.as_deref(), request.home.as_deref())
        .ok_or_else(|| "failed to resolve source state home".to_owned())?;
    let config_home = resolve_config_home(request.xdg_config_home.as_deref(), request.home.as_deref())
        .ok_or_else(|| "failed to resolve source config home".to_owned())?;
    let cache_home = resolve_cache_home(request.xdg_cache_home.as_deref(), request.home.as_deref())
        .ok_or_else(|| "failed to resolve source cache home".to_owned())?;

    let dest_home = request.out_root.join("home");
    let dest_config_home = request.out_root.join("xdg-config");
    let dest_state_home = request.out_root.join("xdg-state");
    let dest_cache_home = request.out_root.join("xdg-cache");
    for dir in [
        &request.out_root,
        &dest_home,
        &dest_config_home,
        &dest_state_home,
        &dest_cache_home,
    ] {
        fs::create_dir_all(dir)
            .map_err(|error| format!("failed to create clone dir {}: {error}", dir.display()))?;
    }

    copy_dir_if_exists(&config_home.join("neozeus"), &dest_config_home.join("neozeus"))?;
    copy_dir_if_exists(&state_home.join("neozeus"), &dest_state_home.join("neozeus"))?;
    copy_dir_if_exists(&cache_home.join("neozeus"), &dest_cache_home.join("neozeus"))?;

    let daemon = SocketTerminalDaemonClient::connect(&request.socket_path)?;
    let session_infos = daemon.list_sessions()?;
    let mut sessions = Vec::with_capacity(session_infos.len());
    for (order_index, info) in session_infos.iter().enumerate() {
        let attached = daemon.attach_session(&info.session_id)?;
        sessions.push(ClonedDaemonSession {
            session_id: info.session_id.clone(),
            snapshot: attached.snapshot,
            revision: info.revision,
            order_index: order_index as u64,
        });
    }

    let owned_tmux_sessions = daemon
        .list_owned_tmux_sessions()?
        .into_iter()
        .map(|info| {
            let capture_text = daemon.capture_owned_tmux_session(&info.session_uid, 200)?;
            Ok(ClonedOwnedTmuxSession { info, capture_text })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let cloned_state_path = dest_state_home
        .join("neozeus")
        .join(CLONED_DAEMON_STATE_FILENAME);
    save_cloned_daemon_state(
        &cloned_state_path,
        &ClonedDaemonState {
            sessions,
            owned_tmux_sessions,
        },
    )?;
    write_clone_env_file(
        &request.out_root,
        &dest_home,
        &dest_config_home,
        &dest_state_home,
        &dest_cache_home,
        &cloned_state_path,
    )?;
    println!("clone root: {}", request.out_root.display());
    println!("clone env: {}", request.out_root.join("neozeus-clone-env.sh").display());
    println!("cloned daemon state: {}", cloned_state_path.display());
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CloneCurrentRequest {
    out_root: PathBuf,
    socket_path: PathBuf,
    home: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
    xdg_state_home: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
}

fn parse_clone_current_request(args: &[String]) -> Result<CloneCurrentRequest, String> {
    let mut out_root = None;
    let mut socket_path = None;
    let mut home = env::var("HOME").ok().map(PathBuf::from);
    let mut xdg_config_home = env::var("XDG_CONFIG_HOME").ok().map(PathBuf::from);
    let mut xdg_state_home = env::var("XDG_STATE_HOME").ok().map(PathBuf::from);
    let mut xdg_cache_home = env::var("XDG_CACHE_HOME").ok().map(PathBuf::from);
    let mut index = 0usize;
    while index < args.len() {
        let flag = args[index].as_str();
        let Some(value) = args.get(index + 1) else {
            return Err(format!("{flag} requires a value"));
        };
        match flag {
            "--out-root" => out_root = Some(PathBuf::from(value)),
            "--socket" => socket_path = Some(PathBuf::from(value)),
            "--home" => home = Some(PathBuf::from(value)),
            "--xdg-config-home" => xdg_config_home = Some(PathBuf::from(value)),
            "--xdg-state-home" => xdg_state_home = Some(PathBuf::from(value)),
            "--xdg-cache-home" => xdg_cache_home = Some(PathBuf::from(value)),
            unknown => return Err(format!("unknown clone-state flag `{unknown}`")),
        }
        index += 2;
    }
    Ok(CloneCurrentRequest {
        out_root: out_root.ok_or_else(|| "--out-root is required".to_owned())?,
        socket_path: socket_path
            .or_else(resolve_daemon_socket_path)
            .ok_or_else(|| "failed to resolve source daemon socket".to_owned())?,
        home,
        xdg_config_home,
        xdg_state_home,
        xdg_cache_home,
    })
}

fn resolve_state_home(xdg_state_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg_state_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".local/state")))
}

fn resolve_config_home(xdg_config_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg_config_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")))
}

fn resolve_cache_home(xdg_cache_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg_cache_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".cache")))
}

fn copy_dir_if_exists(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    if src.is_dir() {
        fs::create_dir_all(dst)
            .map_err(|error| format!("failed to create cloned dir {}: {error}", dst.display()))?;
        for entry in fs::read_dir(src)
            .map_err(|error| format!("failed to read dir {}: {error}", src.display()))?
        {
            let entry = entry
                .map_err(|error| format!("failed to read dir entry in {}: {error}", src.display()))?;
            copy_dir_if_exists(&entry.path(), &dst.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create cloned file dir {}: {error}", parent.display()))?;
    }
    fs::copy(src, dst).map_err(|error| {
        format!(
            "failed to copy clone source {} -> {}: {error}",
            src.display(),
            dst.display()
        )
    })?;
    Ok(())
}

fn write_clone_env_file(
    out_root: &Path,
    home: &Path,
    xdg_config_home: &Path,
    xdg_state_home: &Path,
    xdg_cache_home: &Path,
    cloned_state_path: &Path,
) -> Result<(), String> {
    let env_path = out_root.join("neozeus-clone-env.sh");
    let daemon_socket_path = out_root.join("daemon.sock");
    let content = format!(
        "export HOME='{home}'\nexport XDG_CONFIG_HOME='{config}'\nexport XDG_STATE_HOME='{state}'\nexport XDG_CACHE_HOME='{cache}'\nexport NEOZEUS_DAEMON_SOCKET_PATH='{socket}'\nexport {clone_env}='{clone_path}'\n",
        home = shell_quote_path(home),
        config = shell_quote_path(xdg_config_home),
        state = shell_quote_path(xdg_state_home),
        cache = shell_quote_path(xdg_cache_home),
        socket = shell_quote_path(&daemon_socket_path),
        clone_env = CLONED_DAEMON_STATE_ENV,
        clone_path = shell_quote_path(cloned_state_path),
    );
    fs::write(&env_path, content)
        .map_err(|error| format!("failed to write clone env file {}: {error}", env_path.display()))
}

fn shell_quote_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "'\\''")
}
