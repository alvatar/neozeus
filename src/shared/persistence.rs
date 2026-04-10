use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn resolve_state_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
    app_dir: &str,
    filename: &str,
) -> Option<PathBuf> {
    if let Some(xdg_state_home) = xdg_state_home.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(xdg_state_home).join(app_dir).join(filename));
    }
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(home)
                .join(format!(".local/state/{app_dir}"))
                .join(filename),
        );
    }
    if let Some(xdg_config_home) = xdg_config_home.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(xdg_config_home).join(app_dir).join(filename));
    }
    None
}

pub fn resolve_config_path_with(
    xdg_config_home: Option<&str>,
    home: Option<&str>,
    app_dir: &str,
    filename: &str,
) -> Option<PathBuf> {
    if let Some(xdg_config_home) = xdg_config_home.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(xdg_config_home).join(app_dir).join(filename));
    }
    home.filter(|value| !value.is_empty()).map(|home| {
        PathBuf::from(home)
            .join(format!(".config/{app_dir}"))
            .join(filename)
    })
}

/// Atomically replaces the target file by writing a sibling temp file and renaming it into place.
///
/// Callers are responsible for ensuring the parent directory already exists.
pub fn write_file_atomically(path: &Path, content: &str) -> Result<(), String> {
    let mut tmp_path = path.to_path_buf();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("invalid file name {}", path.display()))?;
    tmp_path.set_file_name(format!(".{file_name}.tmp"));
    fs::write(&tmp_path, content)
        .map_err(|error| format!("failed to write temp file {}: {error}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).map_err(|error| {
        let _ = fs::remove_file(&tmp_path);
        format!(
            "failed to replace {} from {}: {error}",
            path.display(),
            tmp_path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_config_path_with, resolve_state_path_with, write_file_atomically};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should advance")
            .as_nanos();
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{stamp:032x}-{id:016x}"));
        std::fs::create_dir_all(&path).expect("temp dir should create");
        path
    }

    #[test]
    fn resolve_state_path_prefers_xdg_state_then_home_then_xdg_config() {
        assert_eq!(
            resolve_state_path_with(
                Some("/tmp/state"),
                Some("/tmp/home"),
                Some("/tmp/config"),
                "neozeus",
                "state.v1"
            ),
            Some(PathBuf::from("/tmp/state/neozeus/state.v1"))
        );
        assert_eq!(
            resolve_state_path_with(
                None,
                Some("/tmp/home"),
                Some("/tmp/config"),
                "neozeus",
                "state.v1"
            ),
            Some(PathBuf::from("/tmp/home/.local/state/neozeus/state.v1"))
        );
        assert_eq!(
            resolve_state_path_with(None, None, Some("/tmp/config"), "neozeus", "state.v1"),
            Some(PathBuf::from("/tmp/config/neozeus/state.v1"))
        );
    }

    #[test]
    fn resolve_config_path_prefers_xdg_config_then_home_config() {
        assert_eq!(
            resolve_config_path_with(Some("/tmp/config"), Some("/tmp/home"), "neozeus", "hud.v1"),
            Some(PathBuf::from("/tmp/config/neozeus/hud.v1"))
        );
        assert_eq!(
            resolve_config_path_with(None, Some("/tmp/home"), "neozeus", "hud.v1"),
            Some(PathBuf::from("/tmp/home/.config/neozeus/hud.v1"))
        );
    }

    #[test]
    fn write_file_atomically_replaces_target_via_temp_file() {
        let dir = temp_dir("neozeus-shared-atomic-write");
        let path = dir.join("state.txt");
        std::fs::write(&path, "old").unwrap();

        write_file_atomically(&path, "new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert!(!dir.join(".state.txt.tmp").exists());
    }
}
