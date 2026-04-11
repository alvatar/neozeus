use bevy::prelude::Time;
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
pub fn first_non_empty_trimmed_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default()
}

pub fn non_empty_trimmed_lines_after_header(text: &str) -> impl Iterator<Item = &str> + '_ {
    let mut skipped_header = false;
    text.lines().map(str::trim).filter(move |line| {
        if line.is_empty() {
            return false;
        }
        if !skipped_header {
            skipped_header = true;
            return false;
        }
        true
    })
}

pub fn load_text_file_or_default<T>(
    path: &Path,
    parse: impl FnOnce(&str) -> T,
    log_failure: impl FnOnce(&Path, &std::io::Error),
) -> T
where
    T: Default,
{
    match fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => T::default(),
        Err(error) => {
            log_failure(path, &error);
            T::default()
        }
    }
}

pub fn mark_dirty_since(dirty_since_secs: &mut Option<f32>, time: Option<&Time>) {
    if dirty_since_secs.is_none() {
        *dirty_since_secs = Some(time.map(Time::elapsed_secs).unwrap_or(0.0));
    }
}

pub fn save_debounce_elapsed(
    dirty_since_secs: Option<f32>,
    elapsed_secs: f32,
    debounce_secs: f32,
) -> bool {
    dirty_since_secs.is_some_and(|dirty_since| elapsed_secs - dirty_since >= debounce_secs)
}

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
    use super::{
        first_non_empty_trimmed_line, load_text_file_or_default, mark_dirty_since,
        non_empty_trimmed_lines_after_header, resolve_config_path_with, resolve_state_path_with,
        save_debounce_elapsed, write_file_atomically,
    };
    use bevy::prelude::Time;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
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
    fn first_non_empty_trimmed_line_skips_blanks() {
        assert_eq!(
            first_non_empty_trimmed_line("\n  \n version 2 \nbody"),
            "version 2"
        );
        assert_eq!(first_non_empty_trimmed_line("   \n\t"), "");
    }

    #[test]
    fn non_empty_trimmed_lines_after_header_skips_blanks_and_header() {
        let lines = non_empty_trimmed_lines_after_header("\n version 4 \n alpha \n\n beta \n")
            .collect::<Vec<_>>();
        assert_eq!(lines, vec!["alpha", "beta"]);
    }

    #[test]
    fn load_text_file_or_default_returns_default_for_missing_and_logs_other_errors() {
        let missing = temp_dir("neozeus-shared-load-missing").join("missing.txt");
        let mut logged = None;
        let missing_value = load_text_file_or_default(
            &missing,
            |text| text.to_owned(),
            |path, error| logged = Some(format!("{}: {error}", path.display())),
        );
        assert_eq!(missing_value, String::new());
        assert_eq!(logged, None);

        let dir = temp_dir("neozeus-shared-load-error");
        let invalid = dir.join("not-a-file");
        std::fs::create_dir_all(&invalid).unwrap();
        let mut logged = None;
        let loaded = load_text_file_or_default(
            &invalid,
            |text| text.to_owned(),
            |path, error| logged = Some(format!("{}: {error}", path.display())),
        );
        assert_eq!(loaded, String::new());
        assert!(logged
            .as_deref()
            .is_some_and(|value| value.contains("not-a-file")));
    }

    #[test]
    fn save_debounce_elapsed_only_turns_true_after_threshold() {
        assert!(!save_debounce_elapsed(None, 5.0, 0.3));
        assert!(!save_debounce_elapsed(Some(4.8), 5.0, 0.3));
        assert!(save_debounce_elapsed(Some(4.7), 5.0, 0.3));
    }

    #[test]
    fn mark_dirty_since_only_sets_initial_timestamp() {
        let mut dirty_since = None;
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(3));
        mark_dirty_since(&mut dirty_since, Some(&time));
        assert_eq!(dirty_since, Some(3.0));

        time.advance_by(Duration::from_secs(5));
        mark_dirty_since(&mut dirty_since, Some(&time));
        assert_eq!(dirty_since, Some(3.0));
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
