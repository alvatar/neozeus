use std::{
    collections::BTreeSet,
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexThreadRecord {
    pub id: String,
    pub cwd: String,
    pub created_at: i64,
    pub title: String,
}

fn codex_home_dir_with(home: Option<&OsStr>) -> Option<PathBuf> {
    home.map(PathBuf::from).map(|home| home.join(".codex"))
}

fn latest_codex_state_db_path_with(home: Option<&OsStr>) -> Option<PathBuf> {
    let codex_home = codex_home_dir_with(home)?;
    let entries = std::fs::read_dir(codex_home).ok()?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| {
                    name.starts_with("state_")
                        && name.ends_with(".sqlite")
                        && name[6..name.len() - 7]
                            .chars()
                            .all(|ch| ch.is_ascii_digit())
                })
        })
        .max_by_key(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .and_then(|name| name[6..name.len() - 7].parse::<u64>().ok())
                .unwrap_or(0)
        })
}

fn list_codex_threads_with_sqlite3_path(
    sqlite3_program: &str,
    state_db_path: &Path,
) -> Result<Vec<CodexThreadRecord>, String> {
    let output = Command::new(sqlite3_program)
        .arg("-readonly")
        .arg("-noheader")
        .arg("-separator")
        .arg("\t")
        .arg(state_db_path)
        .arg("select id, cwd, created_at, title from threads order by created_at desc, id desc;")
        .output()
        .map_err(|error| {
            format!(
                "failed to run sqlite3 for {}: {error}",
                state_db_path.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "sqlite3 query failed for {}: {}",
            state_db_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|error| {
        format!(
            "sqlite3 output for {} was not utf-8: {error}",
            state_db_path.display()
        )
    })?;
    let mut threads = Vec::new();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.splitn(4, '\t');
        let id = fields.next().unwrap_or_default();
        let cwd = fields.next().unwrap_or_default();
        let created_at = fields.next().unwrap_or_default();
        let title = fields.next().unwrap_or_default();
        let created_at = created_at
            .parse::<i64>()
            .map_err(|error| format!("invalid Codex thread created_at `{created_at}`: {error}"))?;
        threads.push(CodexThreadRecord {
            id: id.to_owned(),
            cwd: cwd.to_owned(),
            created_at,
            title: title.to_owned(),
        });
    }
    Ok(threads)
}

pub fn list_codex_threads() -> Result<Vec<CodexThreadRecord>, String> {
    let Some(state_db_path) = latest_codex_state_db_path_with(env::var_os("HOME").as_deref())
    else {
        return Ok(Vec::new());
    };
    list_codex_threads_with_sqlite3_path("sqlite3", &state_db_path)
}

pub fn codex_thread_ids() -> Result<BTreeSet<String>, String> {
    Ok(list_codex_threads()?
        .into_iter()
        .map(|thread| thread.id)
        .collect())
}

pub fn wait_for_new_codex_thread_id(
    cwd: &str,
    known_thread_ids: &BTreeSet<String>,
    timeout: Duration,
) -> Result<Option<String>, String> {
    let start = Instant::now();
    loop {
        let threads = list_codex_threads()?;
        let matches = threads
            .into_iter()
            .filter(|thread| thread.cwd == cwd && !known_thread_ids.contains(&thread.id))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [thread] => return Ok(Some(thread.id.clone())),
            [] if start.elapsed() >= timeout => return Ok(None),
            [] => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let ids = matches
                    .into_iter()
                    .map(|thread| thread.id)
                    .collect::<Vec<_>>();
                return Err(format!(
                    "ambiguous Codex thread capture for cwd {cwd}: {}",
                    ids.join(", ")
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        codex_home_dir_with, latest_codex_state_db_path_with, list_codex_threads_with_sqlite3_path,
        wait_for_new_codex_thread_id,
    };
    use std::{collections::BTreeSet, path::PathBuf, process::Command, thread, time::Duration};

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    fn sqlite3_available() -> bool {
        Command::new("sqlite3")
            .arg("-version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn build_state_db(path: &PathBuf, rows: &[(&str, &str, i64, &str)]) {
        let parent = path.parent().expect("db path should have parent");
        std::fs::create_dir_all(parent).unwrap();
        let mut script = String::from(
            "create table threads (id text primary key, rollout_path text not null, created_at integer not null, updated_at integer not null, source text not null, model_provider text not null, cwd text not null, title text not null, sandbox_policy text not null, approval_mode text not null, tokens_used integer not null default 0, has_user_event integer not null default 0, archived integer not null default 0, archived_at integer, git_sha text, git_branch text, git_origin_url text, cli_version text not null default '', first_user_message text not null default '');\n",
        );
        for (id, cwd, created_at, title) in rows {
            script.push_str(&format!(
                "insert into threads (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode) values ('{}', '/tmp/out', {}, {}, 'chat', 'openai', '{}', '{}', '{{\"type\":\"workspace-write\"}}', 'on-request');\n",
                id, created_at, created_at, cwd, title.replace('\'', "''")
            ));
        }
        let output = Command::new("sqlite3")
            .arg(path)
            .arg(script)
            .output()
            .expect("sqlite3 should run");
        assert!(
            output.status.success(),
            "sqlite3 init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn codex_home_dir_resolves_under_home() {
        assert_eq!(
            codex_home_dir_with(Some("/tmp/demo".as_ref())),
            Some(PathBuf::from("/tmp/demo/.codex"))
        );
    }

    #[test]
    fn latest_codex_state_db_path_picks_highest_state_index() {
        let home = temp_dir("codex-state-path");
        let codex_home = home.join(".codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::write(codex_home.join("state_4.sqlite"), "").unwrap();
        std::fs::write(codex_home.join("state_12.sqlite"), "").unwrap();
        std::fs::write(codex_home.join("state_x.sqlite"), "").unwrap();
        assert_eq!(
            latest_codex_state_db_path_with(Some(home.as_os_str())),
            Some(codex_home.join("state_12.sqlite"))
        );
    }

    #[test]
    fn list_codex_threads_parses_sqlite_rows() {
        if !sqlite3_available() {
            return;
        }
        let home = temp_dir("codex-state-parse");
        let db = home.join(".codex").join("state_5.sqlite");
        build_state_db(
            &db,
            &[
                ("thread-a", "/tmp/a", 10, "first"),
                ("thread-b", "/tmp/b", 20, "second"),
            ],
        );
        let threads = list_codex_threads_with_sqlite3_path("sqlite3", &db).unwrap();
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id, "thread-b");
        assert_eq!(threads[0].cwd, "/tmp/b");
        assert_eq!(threads[0].created_at, 20);
        assert_eq!(threads[0].title, "second");
    }

    #[test]
    fn wait_for_new_codex_thread_id_returns_unique_new_thread_for_cwd() {
        if !sqlite3_available() {
            return;
        }
        let home = temp_dir("codex-state-capture");
        let db = home.join(".codex").join("state_5.sqlite");
        build_state_db(&db, &[("thread-a", "/tmp/a", 10, "first")]);
        let known = BTreeSet::from(["thread-a".to_owned()]);
        let home_clone = home.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            build_state_db(
                &home_clone.join(".codex").join("state_6.sqlite"),
                &[
                    ("thread-a", "/tmp/a", 10, "first"),
                    ("thread-b", "/tmp/b", 20, "second"),
                ],
            );
        });
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        let result = wait_for_new_codex_thread_id("/tmp/b", &known, Duration::from_secs(2));
        if let Some(previous_home) = previous_home {
            std::env::set_var("HOME", previous_home);
        }
        assert_eq!(result.unwrap(), Some("thread-b".into()));
    }
}
