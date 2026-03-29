use super::models::UsagePersistenceState;
use bevy::prelude::*;
use std::{
    fs,
    process::{Command, Stdio},
    time::SystemTime,
};

const CLAUDE_CACHE_MAX_AGE_SECS: f32 = 60.0;
const OPENAI_CACHE_MAX_AGE_SECS: f32 = 10.0;
const CLAUDE_REFRESH_MIN_INTERVAL_SECS: f32 = 60.0;
const OPENAI_REFRESH_MIN_INTERVAL_SECS: f32 = 5.0;

/// Returns whether one refresh attempt should be spawned now under the given throttle window.
pub(crate) fn should_spawn_refresh(
    last_attempt_secs: Option<f32>,
    now_secs: f32,
    min_interval_secs: f32,
) -> bool {
    last_attempt_secs.is_none_or(|last_attempt| now_secs - last_attempt >= min_interval_secs)
}

/// Returns whether the given cache file is missing or older than the allowed max age.
pub(crate) fn cache_missing_or_stale(path: &std::path::Path, max_age_secs: f32) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = metadata.modified() else {
        return true;
    };
    let Ok(age) = SystemTime::now().duration_since(modified) else {
        return false;
    };
    age.as_secs_f32() > max_age_secs
}

/// Spawns detached helper processes when usage caches are missing or stale.
pub(crate) fn refresh_usage_caches_if_needed(
    time: Res<Time>,
    mut persistence_state: ResMut<UsagePersistenceState>,
) {
    let now_secs = time.elapsed_secs();

    if cache_missing_or_stale(
        &persistence_state.claude_cache_path,
        CLAUDE_CACHE_MAX_AGE_SECS,
    ) && should_spawn_refresh(
        persistence_state.last_claude_refresh_attempt_secs,
        now_secs,
        CLAUDE_REFRESH_MIN_INTERVAL_SECS,
    ) && spawn_usage_fetch(&persistence_state, "fetch-claude").is_ok()
    {
        persistence_state.last_claude_refresh_attempt_secs = Some(now_secs);
    }

    if cache_missing_or_stale(
        &persistence_state.openai_cache_path,
        OPENAI_CACHE_MAX_AGE_SECS,
    ) && should_spawn_refresh(
        persistence_state.last_openai_refresh_attempt_secs,
        now_secs,
        OPENAI_REFRESH_MIN_INTERVAL_SECS,
    ) && spawn_usage_fetch(&persistence_state, "fetch-openai").is_ok()
    {
        persistence_state.last_openai_refresh_attempt_secs = Some(now_secs);
    }
}

fn spawn_usage_fetch(
    persistence_state: &UsagePersistenceState,
    command: &str,
) -> Result<(), std::io::Error> {
    Command::new(&persistence_state.python_program)
        .arg(&persistence_state.helper_script_path)
        .arg(command)
        .env("NEOZEUS_STATE_DIR", &persistence_state.state_dir)
        .env(
            "NEOZEUS_CLAUDE_USAGE_CACHE",
            &persistence_state.claude_cache_path,
        )
        .env(
            "NEOZEUS_OPENAI_USAGE_CACHE",
            &persistence_state.openai_cache_path,
        )
        .env(
            "NEOZEUS_CLAUDE_USAGE_LOG",
            &persistence_state.claude_log_path,
        )
        .env(
            "NEOZEUS_OPENAI_USAGE_LOG",
            &persistence_state.openai_log_path,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{cache_missing_or_stale, refresh_usage_caches_if_needed, should_spawn_refresh};
    use crate::usage::{UsagePersistenceState, UsageSnapshot};
    use bevy::{ecs::system::RunSystemOnce, prelude::*};
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("neozeus-usage-refresh-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn set_file_mtime_secs_ago(path: &PathBuf, seconds_ago: u64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(seconds_ago);
        let status = Command::new("touch")
            .arg("-d")
            .arg(format!("@{timestamp}"))
            .arg(path)
            .status()
            .expect("touch should run");
        assert!(status.success(), "touch should set file mtime");
    }

    #[test]
    fn should_spawn_refresh_enforces_minimum_interval() {
        assert!(should_spawn_refresh(None, 10.0, 5.0));
        assert!(!should_spawn_refresh(Some(8.0), 10.0, 5.0));
        assert!(should_spawn_refresh(Some(4.0), 10.0, 5.0));
    }

    #[test]
    fn cache_missing_or_stale_detects_missing_and_stale_files() {
        let path = temp_dir("stale").join("cache.json");
        assert!(cache_missing_or_stale(&path, 60.0));
        fs::write(&path, "{}").unwrap();
        assert!(!cache_missing_or_stale(&path, 60.0));
        thread::sleep(Duration::from_millis(20));
        assert!(cache_missing_or_stale(&path, 0.0));
    }

    #[test]
    fn refresh_usage_caches_if_needed_updates_throttle_timestamps_when_spawn_succeeds() {
        let state_dir = temp_dir("spawn");
        let helper = state_dir.join("helper.py");
        fs::write(&helper, "import sys; raise SystemExit(0)").unwrap();
        let mut world = World::default();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(UsageSnapshot::default());
        world.insert_resource(UsagePersistenceState {
            state_dir: state_dir.clone(),
            claude_cache_path: state_dir.join("missing-claude.json"),
            openai_cache_path: state_dir.join("missing-openai.json"),
            claude_log_path: state_dir.join("claude.log"),
            openai_log_path: state_dir.join("openai.log"),
            helper_script_path: helper,
            python_program: PathBuf::from("python3"),
            last_claude_refresh_attempt_secs: None,
            last_openai_refresh_attempt_secs: None,
        });
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(10));

        world
            .run_system_once(refresh_usage_caches_if_needed)
            .unwrap();

        let state = world.resource::<UsagePersistenceState>();
        assert_eq!(state.last_claude_refresh_attempt_secs, Some(10.0));
        assert_eq!(state.last_openai_refresh_attempt_secs, Some(10.0));
    }

    #[test]
    fn refresh_usage_caches_if_needed_respects_throttle_window() {
        let state_dir = temp_dir("throttle");
        let helper = state_dir.join("helper.py");
        fs::write(&helper, "import sys; raise SystemExit(0)").unwrap();
        let mut world = World::default();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(UsageSnapshot::default());
        world.insert_resource(UsagePersistenceState {
            state_dir: state_dir.clone(),
            claude_cache_path: state_dir.join("missing-claude.json"),
            openai_cache_path: state_dir.join("missing-openai.json"),
            claude_log_path: state_dir.join("claude.log"),
            openai_log_path: state_dir.join("openai.log"),
            helper_script_path: helper,
            python_program: PathBuf::from("python3"),
            last_claude_refresh_attempt_secs: Some(8.0),
            last_openai_refresh_attempt_secs: Some(8.0),
        });
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(10));

        world
            .run_system_once(refresh_usage_caches_if_needed)
            .unwrap();

        let state = world.resource::<UsagePersistenceState>();
        assert_eq!(state.last_claude_refresh_attempt_secs, Some(8.0));
        assert_eq!(state.last_openai_refresh_attempt_secs, Some(8.0));
    }

    #[test]
    fn stale_claude_cache_keeps_usable_data_while_refresh_is_triggered() {
        let state_dir = temp_dir("stale-claude");
        let helper = state_dir.join("helper.py");
        let claude_cache = state_dir.join("claude.json");
        fs::write(&helper, "import sys; raise SystemExit(0)").unwrap();
        fs::write(
            &claude_cache,
            r#"{"five_hour":{"utilization":12.0,"resets_at":"5m"},"seven_day":{"utilization":34.0,"resets_at":"2h"}}"#,
        )
        .unwrap();
        set_file_mtime_secs_ago(&claude_cache, 120);

        let mut world = World::default();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(UsageSnapshot::default());
        world.insert_resource(UsagePersistenceState {
            state_dir: state_dir.clone(),
            claude_cache_path: claude_cache,
            openai_cache_path: state_dir.join("missing-openai.json"),
            claude_log_path: state_dir.join("claude.log"),
            openai_log_path: state_dir.join("openai.log"),
            helper_script_path: helper,
            python_program: PathBuf::from("python3"),
            last_claude_refresh_attempt_secs: None,
            last_openai_refresh_attempt_secs: None,
        });
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(10));
        world
            .run_system_once(crate::usage::sync_usage_snapshot_from_cache)
            .unwrap();
        world
            .run_system_once(refresh_usage_caches_if_needed)
            .unwrap();

        let snapshot = world.resource::<UsageSnapshot>();
        assert_eq!(snapshot.claude.session_pct, 12.0);
        assert_eq!(snapshot.claude.week_pct, 34.0);
        assert_eq!(
            world
                .resource::<UsagePersistenceState>()
                .last_claude_refresh_attempt_secs,
            Some(10.0)
        );
    }

    #[test]
    fn stale_openai_cache_triggers_one_refresh_then_throttles() {
        let state_dir = temp_dir("stale-openai");
        let helper = state_dir.join("helper.py");
        let openai_cache = state_dir.join("openai.json");
        fs::write(&helper, "import sys; raise SystemExit(0)").unwrap();
        fs::write(
            &openai_cache,
            r#"{"requests_limit":100,"requests_remaining":60,"tokens_limit":1000,"tokens_remaining":200}"#,
        )
        .unwrap();
        set_file_mtime_secs_ago(&openai_cache, 20);

        let mut world = World::default();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(UsageSnapshot::default());
        world.insert_resource(UsagePersistenceState {
            state_dir: state_dir.clone(),
            claude_cache_path: state_dir.join("missing-claude.json"),
            openai_cache_path: openai_cache,
            claude_log_path: state_dir.join("claude.log"),
            openai_log_path: state_dir.join("openai.log"),
            helper_script_path: helper,
            python_program: PathBuf::from("python3"),
            last_claude_refresh_attempt_secs: None,
            last_openai_refresh_attempt_secs: None,
        });
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(10));
        world
            .run_system_once(refresh_usage_caches_if_needed)
            .unwrap();
        assert_eq!(
            world
                .resource::<UsagePersistenceState>()
                .last_openai_refresh_attempt_secs,
            Some(10.0)
        );

        world
            .run_system_once(refresh_usage_caches_if_needed)
            .unwrap();
        assert_eq!(
            world
                .resource::<UsagePersistenceState>()
                .last_openai_refresh_attempt_secs,
            Some(10.0)
        );
    }
}
