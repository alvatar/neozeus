use std::{fs, path::Path};

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
    use super::write_file_atomically;
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
    fn write_file_atomically_replaces_target_via_temp_file() {
        let dir = temp_dir("neozeus-shared-atomic-write");
        let path = dir.join("state.txt");
        std::fs::write(&path, "old").unwrap();

        write_file_atomically(&path, "new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert!(!dir.join(".state.txt.tmp").exists());
    }
}
