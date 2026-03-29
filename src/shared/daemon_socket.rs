use std::{env, path::PathBuf};

const DAEMON_SOCKET_FILENAME: &str = "daemon.v2.sock";

/// Resolves the daemon socket path from explicit override/runtime/home inputs.
pub(crate) fn resolve_daemon_socket_path_with(
    override_path: Option<&str>,
    xdg_runtime_dir: Option<&str>,
    home: Option<&str>,
    user: Option<&str>,
) -> Option<PathBuf> {
    if let Some(override_path) = override_path.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(override_path));
    }

    if let Some(xdg_runtime_dir) = xdg_runtime_dir.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg_runtime_dir)
                .join("neozeus")
                .join(DAEMON_SOCKET_FILENAME),
        );
    }

    let user = user.filter(|value| !value.is_empty()).unwrap_or("user");
    if home.is_some() {
        return Some(
            std::env::temp_dir()
                .join(format!("neozeus-{user}"))
                .join(DAEMON_SOCKET_FILENAME),
        );
    }

    None
}

/// Resolves the daemon socket path from the real process environment.
pub(crate) fn resolve_daemon_socket_path() -> Option<PathBuf> {
    resolve_daemon_socket_path_with(
        env::var("NEOZEUS_DAEMON_SOCKET_PATH").ok().as_deref(),
        env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("USER").ok().as_deref(),
    )
}
