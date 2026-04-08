use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};

const DAEMON_SOCKET_FILENAME: &str = "daemon.v2.sock";
pub const DAEMON_SOCKET_PATH_ENV: &str = "NEOZEUS_DAEMON_SOCKET_PATH";
pub const DAEMON_SOCKET_ENV: &str = "NEOZEUS_DAEMON_SOCKET";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonEndpointConfig {
    pub socket_path: PathBuf,
}

impl DaemonEndpointConfig {
    pub fn env_pairs(&self) -> [(String, String); 2] {
        daemon_socket_env_pairs(&self.socket_path)
    }
}

pub fn daemon_socket_env_pairs(socket_path: &Path) -> [(String, String); 2] {
    let value = socket_path.to_string_lossy().into_owned();
    [
        (DAEMON_SOCKET_PATH_ENV.to_owned(), value.clone()),
        (DAEMON_SOCKET_ENV.to_owned(), value),
    ]
}

pub fn daemon_socket_path_from_env_map<'a>(env: &'a HashMap<String, String>) -> Option<&'a str> {
    env.get(DAEMON_SOCKET_PATH_ENV)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env.get(DAEMON_SOCKET_ENV)
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
        })
}

/// Resolves the daemon socket path from explicit override/runtime/home inputs.
pub fn resolve_daemon_socket_path_with(
    override_path: Option<&str>,
    compatibility_socket_env: Option<&str>,
    xdg_runtime_dir: Option<&str>,
    home: Option<&str>,
    user: Option<&str>,
) -> Option<PathBuf> {
    if let Some(override_path) = override_path.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(override_path));
    }

    if let Some(socket_path) = compatibility_socket_env.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(socket_path));
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

pub fn resolve_daemon_endpoint_config_with(
    override_path: Option<&str>,
    compatibility_socket_env: Option<&str>,
    xdg_runtime_dir: Option<&str>,
    home: Option<&str>,
    user: Option<&str>,
) -> Option<DaemonEndpointConfig> {
    resolve_daemon_socket_path_with(
        override_path,
        compatibility_socket_env,
        xdg_runtime_dir,
        home,
        user,
    )
    .map(|socket_path| DaemonEndpointConfig { socket_path })
}

pub fn resolve_daemon_endpoint_config() -> Option<DaemonEndpointConfig> {
    resolve_daemon_endpoint_config_with(
        env::var(DAEMON_SOCKET_PATH_ENV).ok().as_deref(),
        env::var(DAEMON_SOCKET_ENV).ok().as_deref(),
        env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("USER").ok().as_deref(),
    )
}

/// Resolves the daemon socket path from the real process environment.
pub fn resolve_daemon_socket_path() -> Option<PathBuf> {
    resolve_daemon_endpoint_config().map(|config| config.socket_path)
}

#[cfg(test)]
mod tests {
    use super::{
        daemon_socket_env_pairs, daemon_socket_path_from_env_map, resolve_daemon_socket_path_with,
        DAEMON_SOCKET_ENV, DAEMON_SOCKET_PATH_ENV,
    };
    use std::{collections::HashMap, path::PathBuf};

    #[test]
    fn daemon_socket_path_prefers_path_env_over_compatibility_env() {
        let resolved = resolve_daemon_socket_path_with(
            Some("/tmp/override.sock"),
            Some("/tmp/compat.sock"),
            Some("/run/user/1000"),
            Some("/home/alvatar"),
            Some("oracle"),
        )
        .expect("override path should resolve");
        assert_eq!(resolved, PathBuf::from("/tmp/override.sock"));
    }

    #[test]
    fn daemon_socket_path_uses_compatibility_env_when_path_env_is_absent() {
        let resolved = resolve_daemon_socket_path_with(
            None,
            Some("/tmp/compat.sock"),
            Some("/run/user/1000"),
            Some("/home/alvatar"),
            Some("oracle"),
        )
        .expect("compatibility env should resolve");
        assert_eq!(resolved, PathBuf::from("/tmp/compat.sock"));
    }

    #[test]
    fn daemon_socket_path_from_env_map_accepts_both_env_names() {
        let path_env =
            HashMap::from([(DAEMON_SOCKET_PATH_ENV.to_owned(), "/tmp/path.sock".into())]);
        assert_eq!(
            daemon_socket_path_from_env_map(&path_env),
            Some("/tmp/path.sock")
        );

        let compat_env = HashMap::from([(DAEMON_SOCKET_ENV.to_owned(), "/tmp/compat.sock".into())]);
        assert_eq!(
            daemon_socket_path_from_env_map(&compat_env),
            Some("/tmp/compat.sock")
        );
    }

    #[test]
    fn daemon_socket_env_pairs_export_both_env_names() {
        let pairs = daemon_socket_env_pairs(PathBuf::from("/tmp/daemon.sock").as_path());
        assert_eq!(pairs[0].0, DAEMON_SOCKET_PATH_ENV);
        assert_eq!(pairs[0].1, "/tmp/daemon.sock");
        assert_eq!(pairs[1].0, DAEMON_SOCKET_ENV);
        assert_eq!(pairs[1].1, "/tmp/daemon.sock");
    }
}
