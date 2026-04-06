use crate::shared::text_escape::{
    quote_escaped_string, unquote_escaped_string, EXTENDED_QUOTED_STRING_ESCAPES,
};
use std::{
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_SESSION_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

const PI_AGENT_SESSIONS_RELATIVE_DIR: &str = ".pi/agent/sessions";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiSessionHeader {
    pub version: u64,
    pub id: String,
    pub timestamp: String,
    pub cwd: String,
    pub parent_session: Option<String>,
}

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
            let path = if let Some(rest) = value.strip_prefix("~/") {
                let home = home
                    .map(PathBuf::from)
                    .ok_or_else(|| "cannot expand `~` without HOME".to_owned())?;
                home.join(rest)
            } else {
                PathBuf::from(value)
            };
            if path.is_absolute() {
                path
            } else {
                current_dir.join(path)
            }
        }
    };

    Ok(lexically_normalize_path(&resolved)
        .to_string_lossy()
        .into_owned())
}

fn lexically_normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let popped = normalized.pop();
                if !popped {
                    normalized.push(component.as_os_str());
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Encodes one cwd into Pi's session-directory naming convention.
pub fn encode_session_dir(cwd: &str) -> String {
    format!(
        "--{}--",
        cwd.trim_start_matches('/').replace(['/', ':'], "-")
    )
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

    let timestamp_millis = current_timestamp_millis();
    let counter = NEXT_SESSION_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = format!("{timestamp_millis:020}_{counter:016x}.jsonl");
    Ok(session_dir.join(file_name).to_string_lossy().into_owned())
}

/// Reads one Pi session header from a session JSONL file.
pub fn read_session_header(session_path: &str) -> Result<PiSessionHeader, String> {
    let source_text = std::fs::read_to_string(session_path)
        .map_err(|error| format!("failed to read Pi session `{session_path}`: {error}"))?;
    parse_session_header_text(&source_text)
}

/// Forks one Pi session JSONL file into a new independent session file.
pub fn fork_session(source_path: &str, target_cwd: Option<&str>) -> Result<String, String> {
    fork_session_with(
        source_path,
        target_cwd,
        std::env::var_os("HOME").as_deref(),
        std::env::current_dir()
            .map_err(|error| format!("cannot resolve current working directory: {error}"))?,
    )
}

fn fork_session_with(
    source_path: &str,
    target_cwd: Option<&str>,
    home: Option<&std::ffi::OsStr>,
    current_dir: PathBuf,
) -> Result<String, String> {
    let source_text = std::fs::read_to_string(source_path)
        .map_err(|error| format!("failed to read Pi session `{source_path}`: {error}"))?;
    let lines = source_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (header_index, header) = parse_session_header_lines(&lines)?;
    let resolved_target_cwd = resolve_session_cwd_with(target_cwd, home, current_dir)?;
    let target_path =
        make_new_session_path_with(Some(&resolved_target_cwd), home, PathBuf::from("/"))?;
    let target_id = Path::new(&target_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("failed to derive Pi session id from `{target_path}`"))?
        .to_owned();
    let timestamp = current_timestamp_millis().to_string();
    let new_header = format!(
        "{{\"type\":\"session\",\"version\":{},\"id\":{},\"timestamp\":{},\"cwd\":{},\"parentSession\":{}}}",
        header.version,
        quote_escaped_string(&target_id, EXTENDED_QUOTED_STRING_ESCAPES),
        quote_escaped_string(&timestamp, EXTENDED_QUOTED_STRING_ESCAPES),
        quote_escaped_string(&resolved_target_cwd, EXTENDED_QUOTED_STRING_ESCAPES),
        quote_escaped_string(source_path, EXTENDED_QUOTED_STRING_ESCAPES),
    );

    let mut output = String::new();
    output.push_str(&new_header);
    output.push('\n');
    for (index, line) in lines.iter().enumerate() {
        if index == header_index {
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    std::fs::write(&target_path, output)
        .map_err(|error| format!("failed to write forked Pi session {}: {error}", target_path))?;
    Ok(target_path)
}

fn parse_session_header_text(text: &str) -> Result<PiSessionHeader, String> {
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (_, header) = parse_session_header_lines(&lines)?;
    Ok(header)
}

fn parse_session_header_lines(lines: &[String]) -> Result<(usize, PiSessionHeader), String> {
    for (index, line) in lines.iter().enumerate() {
        if json_string_field(line, "type").as_deref() != Some("session") {
            continue;
        }
        let cwd = json_string_field(line, "cwd")
            .ok_or_else(|| "Pi session header missing cwd".to_owned())?;
        let id = json_string_field(line, "id").unwrap_or_default();
        let timestamp = json_string_field(line, "timestamp").unwrap_or_default();
        let version = json_u64_field(line, "version").unwrap_or(3);
        let parent_session = json_string_field(line, "parentSession");
        return Ok((
            index,
            PiSessionHeader {
                version,
                id,
                timestamp,
                cwd,
                parent_session,
            },
        ));
    }
    Err("Pi session missing header entry".to_owned())
}

fn json_string_field(line: &str, key: &str) -> Option<String> {
    let raw = json_field_value_slice(line, key)?;
    unquote_escaped_string(raw, EXTENDED_QUOTED_STRING_ESCAPES)
}

fn json_u64_field(line: &str, key: &str) -> Option<u64> {
    json_field_value_slice(line, key)?
        .trim()
        .parse::<u64>()
        .ok()
}

fn json_field_value_slice<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("\"{key}\"");
    let key_index = line.find(&pattern)?;
    let mut cursor = key_index + pattern.len();
    cursor += line[cursor..]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    if !line[cursor..].starts_with(':') {
        return None;
    }
    cursor += 1;
    cursor += line[cursor..]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    if cursor >= line.len() {
        return None;
    }

    if line[cursor..].starts_with('"') {
        let mut escaped = false;
        for (offset, ch) in line[cursor + 1..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => return Some(&line[cursor..=cursor + offset + 1]),
                _ => {}
            }
        }
        return None;
    }

    let end = line[cursor..]
        .find([',', '}'])
        .map(|offset| cursor + offset)
        .unwrap_or(line.len());
    Some(line[cursor..end].trim())
}

fn current_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        agent_sessions_dir_with, encode_session_dir, fork_session_with, make_new_session_path_with,
        parse_session_header_text, read_session_header, resolve_session_cwd_with,
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
    fn resolve_session_cwd_expands_home_defaults_to_current_dir_and_normalizes_relative_paths() {
        let home = temp_dir("pi-session-home");
        let cwd = temp_dir("pi-session-cwd");

        assert_eq!(
            resolve_session_cwd_with(Some("~/code"), Some(home.as_os_str()), cwd.clone()).unwrap(),
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
        assert_eq!(
            resolve_session_cwd_with(
                Some("./nested/../repo"),
                Some(home.as_os_str()),
                cwd.clone()
            )
            .unwrap(),
            cwd.join("repo").to_string_lossy()
        );
    }

    #[test]
    fn make_new_session_path_uses_real_pi_session_directory_layout() {
        let home = temp_dir("pi-session-root");
        let cwd = temp_dir("pi-session-path-cwd");

        let path =
            make_new_session_path_with(Some("~/code/demo"), Some(home.as_os_str()), cwd).unwrap();
        let path = std::path::PathBuf::from(path);

        assert_eq!(
            path.parent().unwrap(),
            agent_sessions_dir_with(Some(home.as_os_str()))
                .unwrap()
                .join(encode_session_dir(
                    &home.join("code/demo").to_string_lossy()
                ))
        );
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("jsonl")
        );
    }

    #[test]
    fn read_session_header_parses_real_header_fields() {
        let dir = temp_dir("pi-session-header");
        let path = dir.join("session.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"session\",\"version\":3,\"id\":\"abc\",\"timestamp\":\"t0\",\"cwd\":\"/tmp/project\"}\n{\"type\":\"message\",\"id\":\"m1\"}\n",
        )
        .unwrap();

        let header = read_session_header(path.to_str().unwrap()).unwrap();
        assert_eq!(header.version, 3);
        assert_eq!(header.id, "abc");
        assert_eq!(header.timestamp, "t0");
        assert_eq!(header.cwd, "/tmp/project");
        assert_eq!(header.parent_session, None);
    }

    #[test]
    fn fork_session_creates_new_file_without_mutating_parent() {
        let home = temp_dir("pi-session-fork-home");
        let cwd = temp_dir("pi-session-fork-cwd");
        let source_dir = temp_dir("pi-session-fork-source");
        let source = source_dir.join("source.jsonl");
        let source_text = concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"parent-id\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp/project\"}\n",
            "{\"type\":\"message\",\"id\":\"m1\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
            "{\"type\":\"message\",\"id\":\"m2\",\"message\":{\"role\":\"assistant\",\"content\":\"hi\"}}\n"
        );
        std::fs::write(&source, source_text).unwrap();
        let parent_before = std::fs::read(&source).unwrap();

        let child_raw = fork_session_with(
            source.to_str().unwrap(),
            Some("/home/user/project"),
            Some(home.as_os_str()),
            cwd,
        )
        .unwrap();
        let child = PathBuf::from(child_raw);

        assert_ne!(child, source);
        assert!(child.is_file());
        assert_eq!(std::fs::read(&source).unwrap(), parent_before);

        let child_text = std::fs::read_to_string(&child).unwrap();
        let child_lines = child_text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        let header = parse_session_header_text(&child_text).unwrap();
        assert_eq!(header.version, 3);
        assert_eq!(header.cwd, "/home/user/project");
        assert_eq!(header.parent_session.as_deref(), source.to_str());
        assert_eq!(child_lines[1], "{\"type\":\"message\",\"id\":\"m1\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}");
        assert_eq!(child_lines[2], "{\"type\":\"message\",\"id\":\"m2\",\"message\":{\"role\":\"assistant\",\"content\":\"hi\"}}");
    }

    #[test]
    fn fork_session_rejects_missing_file() {
        let home = temp_dir("pi-session-fork-missing-home");
        let cwd = temp_dir("pi-session-fork-missing-cwd");
        let error = fork_session_with(
            "/tmp/neozeus-does-not-exist.jsonl",
            Some("/tmp/project"),
            Some(home.as_os_str()),
            cwd,
        )
        .expect_err("missing source should fail");
        assert!(error.contains("failed to read Pi session"));
    }

    #[test]
    fn fork_session_rejects_empty_or_malformed_sources() {
        let home = temp_dir("pi-session-fork-malformed-home");
        let cwd = temp_dir("pi-session-fork-malformed-cwd");
        let dir = temp_dir("pi-session-fork-malformed-source");
        let empty = dir.join("empty.jsonl");
        let malformed = dir.join("malformed.jsonl");
        std::fs::write(&empty, "\n\n").unwrap();
        std::fs::write(&malformed, "{\"type\":\"message\"}\n").unwrap();

        let empty_error = fork_session_with(
            empty.to_str().unwrap(),
            Some("/tmp/project"),
            Some(home.as_os_str()),
            cwd.clone(),
        )
        .expect_err("empty source should fail");
        assert!(empty_error.contains("missing header"));

        let malformed_error = fork_session_with(
            malformed.to_str().unwrap(),
            Some("/tmp/project"),
            Some(home.as_os_str()),
            cwd,
        )
        .expect_err("malformed source should fail");
        assert!(malformed_error.contains("missing header"));
    }

    #[test]
    fn fork_session_target_cwd_controls_destination_directory() {
        let home = temp_dir("pi-session-fork-dest-home");
        let cwd = temp_dir("pi-session-fork-dest-cwd");
        let source_dir = temp_dir("pi-session-fork-dest-source");
        let source = source_dir.join("source.jsonl");
        std::fs::write(
            &source,
            "{\"type\":\"session\",\"version\":3,\"id\":\"parent-id\",\"timestamp\":\"t0\",\"cwd\":\"/tmp/project\"}\n",
        )
        .unwrap();

        let child_raw = fork_session_with(
            source.to_str().unwrap(),
            Some("~/work/clone"),
            Some(home.as_os_str()),
            cwd,
        )
        .unwrap();
        let child = PathBuf::from(child_raw);

        assert_eq!(
            child.parent().unwrap(),
            agent_sessions_dir_with(Some(home.as_os_str()))
                .unwrap()
                .join(encode_session_dir(
                    &home.join("work/clone").to_string_lossy()
                ))
        );
    }
}
