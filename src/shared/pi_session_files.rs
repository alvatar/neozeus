use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_SESSION_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

const PI_AGENT_SESSIONS_RELATIVE_DIR: &str = ".pi/agent/sessions";

/// Resolves the root directory where Pi stores session JSONL files.
pub fn agent_sessions_dir() -> Result<PathBuf, String> {
    agent_sessions_dir_with(std::env::var_os("HOME").as_deref())
}

fn agent_sessions_dir_with(home: Option<&std::ffi::OsStr>) -> Result<PathBuf, String> {
    let home = home
        .map(PathBuf::from)
        .ok_or_else(|| "cannot resolve Pi session dir without HOME".to_owned())?;
    Ok(home.join(PI_AGENT_SESSIONS_RELATIVE_DIR))
}

/// Resolves one requested Pi session cwd into the real absolute path NeoZeus will launch.
///
/// Empty input falls back to the process current working directory. `~` and `~/...` expand against
/// `$HOME` so Pi session provenance matches the actual shell cwd instead of the raw dialog text.
pub fn resolve_session_cwd(raw: Option<&str>) -> Result<String, String> {
    resolve_session_cwd_with(
        raw,
        std::env::var_os("HOME").as_deref(),
        std::env::current_dir()
            .map_err(|error| format!("cannot resolve current working directory: {error}"))?,
    )
}

fn resolve_session_cwd_with(
    raw: Option<&str>,
    home: Option<&std::ffi::OsStr>,
    current_dir: PathBuf,
) -> Result<String, String> {
    let resolved = match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => current_dir,
        Some("~") => home
            .map(PathBuf::from)
            .ok_or_else(|| "cannot expand `~` without HOME".to_owned())?,
        Some(value) => {
            if let Some(rest) = value.strip_prefix("~/") {
                let home = home
                    .map(PathBuf::from)
                    .ok_or_else(|| "cannot expand `~` without HOME".to_owned())?;
                home.join(rest)
            } else {
                PathBuf::from(value)
            }
        }
    };

    Ok(resolved.to_string_lossy().into_owned())
}

/// Encodes one cwd into Pi's session-directory naming convention.
pub fn encode_session_dir(cwd: &str) -> String {
    format!("--{}--", cwd.trim_start_matches('/').replace(['/', ':'], "-"))
}

/// Returns a fresh Pi session file path rooted under Pi's real session directory.
pub fn make_new_session_path(target_cwd: Option<&str>) -> Result<String, String> {
    make_new_session_path_with(
        target_cwd,
        std::env::var_os("HOME").as_deref(),
        std::env::current_dir()
            .map_err(|error| format!("cannot resolve current working directory: {error}"))?,
    )
}

fn make_new_session_path_with(
    target_cwd: Option<&str>,
    home: Option<&std::ffi::OsStr>,
    current_dir: PathBuf,
) -> Result<String, String> {
    let resolved_cwd = resolve_session_cwd_with(target_cwd, home, current_dir)?;
    let session_dir = agent_sessions_dir_with(home)?.join(encode_session_dir(&resolved_cwd));
    std::fs::create_dir_all(&session_dir).map_err(|error| {
        format!(
            "failed to create Pi session directory {}: {error}",
            session_dir.display()
        )
    })?;

    let timestamp_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let counter = NEXT_SESSION_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = format!("{timestamp_millis:020}_{counter:016x}.jsonl");
    Ok(session_dir.join(file_name).to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        agent_sessions_dir_with, encode_session_dir, make_new_session_path_with,
        resolve_session_cwd_with,
    };
    use std::path::PathBuf;

    fn temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "neozeus-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&path).expect("temp dir should create");
        path
    }

    #[test]
    fn resolve_session_cwd_expands_home_and_defaults_to_current_dir() {
        let home = temp_dir("pi-session-home");
        let cwd = temp_dir("pi-session-cwd");

        assert_eq!(
            resolve_session_cwd_with(Some("~/code"), Some(home.as_os_str()), cwd.clone())
                .unwrap(),
            home.join("code").to_string_lossy()
        );
        assert_eq!(
            resolve_session_cwd_with(Some("~"), Some(home.as_os_str()), cwd.clone()).unwrap(),
            home.to_string_lossy()
        );
        assert_eq!(
            resolve_session_cwd_with(None, Some(home.as_os_str()), cwd.clone()).unwrap(),
            cwd.to_string_lossy()
        );
    }

    #[test]
    fn make_new_session_path_uses_real_pi_session_directory_layout() {
        let home = temp_dir("pi-session-root");
        let cwd = temp_dir("pi-session-path-cwd");

        let path = make_new_session_path_with(
            Some("~/code/demo"),
            Some(home.as_os_str()),
            cwd,
        )
        .unwrap();
        let path = std::path::PathBuf::from(path);

        assert_eq!(
            path.parent().unwrap(),
            agent_sessions_dir_with(Some(home.as_os_str()))
                .unwrap()
                .join(encode_session_dir(&home.join("code/demo").to_string_lossy()))
        );
        assert_eq!(path.extension().and_then(|value| value.to_str()), Some("jsonl"));
    }
}
