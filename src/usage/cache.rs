use super::models::{ClaudeUsageData, OpenAiUsageData, UsagePersistenceState, UsageSnapshot};
use bevy::prelude::*;
use std::{env, fs, path::PathBuf};

const CLAUDE_CACHE_FILENAME: &str = "claude-usage-cache.json";
const OPENAI_CACHE_FILENAME: &str = "openai-usage-cache.json";
const CLAUDE_LOG_FILENAME: &str = "claude-usage.log";
const OPENAI_LOG_FILENAME: &str = "openai-usage.log";
const CLAUDE_BACKOFF_FILENAME: &str = "claude-usage-backoff-until.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheReadState {
    Missing,
    Malformed,
    Parsed,
}

/// Resolves the NeoZeus state directory used by usage caches and helper logs.
pub(crate) fn resolve_usage_state_dir() -> PathBuf {
    if let Some(explicit) = env::var_os("NEOZEUS_STATE_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(explicit);
    }
    if let Some(xdg_state_home) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(xdg_state_home).join("neozeus");
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".local/state/neozeus");
    }
    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty())
    {
        return PathBuf::from(xdg_config_home).join("neozeus");
    }
    PathBuf::from("/tmp/neozeus")
}

/// Builds the default usage persistence resource from the current process environment.
pub(crate) fn default_usage_persistence_state() -> UsagePersistenceState {
    let state_dir = resolve_usage_state_dir();
    UsagePersistenceState {
        claude_cache_path: env::var_os("NEOZEUS_CLAUDE_USAGE_CACHE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join(CLAUDE_CACHE_FILENAME)),
        openai_cache_path: env::var_os("NEOZEUS_OPENAI_USAGE_CACHE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join(OPENAI_CACHE_FILENAME)),
        claude_log_path: env::var_os("NEOZEUS_CLAUDE_USAGE_LOG")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join(CLAUDE_LOG_FILENAME)),
        openai_log_path: env::var_os("NEOZEUS_OPENAI_USAGE_LOG")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join(OPENAI_LOG_FILENAME)),
        claude_backoff_until_path: env::var_os("NEOZEUS_CLAUDE_USAGE_BACKOFF")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join(CLAUDE_BACKOFF_FILENAME)),
        helper_script_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/usage_fetch.py"),
        python_program: PathBuf::from(env::var_os("PYTHON").unwrap_or_else(|| "python3".into())),
        state_dir,
        last_claude_refresh_attempt_secs: None,
        last_openai_refresh_attempt_secs: None,
    }
}

/// Refreshes the in-memory usage snapshot from the current cache files.
pub(crate) fn sync_usage_snapshot_from_cache(
    persistence_state: Res<UsagePersistenceState>,
    mut usage_snapshot: ResMut<UsageSnapshot>,
) {
    let (claude_state, claude_usage) =
        read_claude_usage_from_path(&persistence_state.claude_cache_path);
    match claude_state {
        CacheReadState::Parsed => usage_snapshot.claude = claude_usage,
        CacheReadState::Missing => usage_snapshot.claude = ClaudeUsageData::default(),
        CacheReadState::Malformed => {}
    }

    let (openai_state, openai_usage) =
        read_openai_usage_from_path(&persistence_state.openai_cache_path);
    match openai_state {
        CacheReadState::Parsed => usage_snapshot.openai = openai_usage,
        CacheReadState::Missing => usage_snapshot.openai = OpenAiUsageData::default(),
        CacheReadState::Malformed => {}
    }
}

/// Reads one Claude usage cache from disk into the normalized app model.
pub(crate) fn read_claude_usage_from_path(path: &PathBuf) -> (CacheReadState, ClaudeUsageData) {
    let Ok(text) = fs::read_to_string(path) else {
        return (CacheReadState::Missing, ClaudeUsageData::default());
    };
    let Some(five_hour) = find_json_object(&text, "five_hour") else {
        return (CacheReadState::Malformed, ClaudeUsageData::default());
    };
    let seven_day = find_json_object(&text, "seven_day");
    let extra_usage = find_json_object(&text, "extra_usage");
    (
        CacheReadState::Parsed,
        ClaudeUsageData {
            session_pct: find_json_number(five_hour, "utilization").unwrap_or(0.0) as f32,
            week_pct: seven_day
                .and_then(|bucket| find_json_number(bucket, "utilization"))
                .unwrap_or(0.0) as f32,
            extra_pct: extra_usage
                .and_then(|bucket| find_json_number(bucket, "utilization"))
                .unwrap_or(0.0) as f32,
            extra_used: extra_usage
                .and_then(|bucket| find_json_number(bucket, "used_credits"))
                .unwrap_or(0.0) as f32,
            extra_limit: extra_usage
                .and_then(|bucket| find_json_number(bucket, "monthly_limit"))
                .unwrap_or(0.0) as f32,
            session_resets_at: find_json_string(five_hour, "resets_at").unwrap_or_default(),
            week_resets_at: seven_day
                .and_then(|bucket| find_json_string(bucket, "resets_at"))
                .unwrap_or_default(),
            available: true,
        },
    )
}

/// Reads one OpenAI usage cache from disk into the normalized app model.
pub(crate) fn read_openai_usage_from_path(path: &PathBuf) -> (CacheReadState, OpenAiUsageData) {
    let Ok(text) = fs::read_to_string(path) else {
        return (CacheReadState::Missing, OpenAiUsageData::default());
    };

    let requests_limit = find_json_i32(&text, "requests_limit").unwrap_or(0);
    let requests_remaining = find_json_i32(&text, "requests_remaining").unwrap_or(0);
    let tokens_limit = find_json_i32(&text, "tokens_limit").unwrap_or(0);
    let tokens_remaining = find_json_i32(&text, "tokens_remaining").unwrap_or(0);
    let requests_pct = find_json_number(&text, "requests_pct")
        .map(float_to_milli_percent)
        .unwrap_or_else(|| utilization_milli_percent(requests_limit, requests_remaining));
    let tokens_pct = find_json_number(&text, "tokens_pct")
        .map(float_to_milli_percent)
        .unwrap_or_else(|| utilization_milli_percent(tokens_limit, tokens_remaining));
    let requests_resets_at = find_json_string(&text, "requests_resets_at").unwrap_or_default();
    let tokens_resets_at = find_json_string(&text, "tokens_resets_at").unwrap_or_default();
    let available = requests_limit > 0 || tokens_limit > 0 || requests_pct > 0 || tokens_pct > 0;

    if !text.contains('"') {
        return (CacheReadState::Malformed, OpenAiUsageData::default());
    }

    (
        CacheReadState::Parsed,
        OpenAiUsageData {
            requests_pct_milli: requests_pct,
            tokens_pct_milli: tokens_pct,
            requests_limit,
            requests_remaining,
            tokens_limit,
            tokens_remaining,
            requests_resets_at,
            tokens_resets_at,
            available,
        },
    )
}

fn utilization_milli_percent(limit: i32, remaining: i32) -> i32 {
    if limit <= 0 {
        return 0;
    }
    (((limit - remaining) as f64 / limit as f64) * 100_000.0).round() as i32
}

fn float_to_milli_percent(value: f64) -> i32 {
    (value.clamp(0.0, 100.0) * 1000.0).round() as i32
}

fn find_json_object<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let value_start = text[key_start..].find(':')? + key_start + 1;
    let trimmed_start = skip_json_whitespace(text, value_start);
    if text[trimmed_start..].starts_with("null") {
        return None;
    }
    let object_start = text[trimmed_start..].find('{')? + trimmed_start;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[object_start..].char_indices() {
        match ch {
            '"' if !escaped => in_string = !in_string,
            '\\' if in_string => {
                escaped = !escaped;
                continue;
            }
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = object_start + offset + 1;
                    return Some(&text[object_start..end]);
                }
            }
            _ => {}
        }
        escaped = false;
    }
    None
}

fn find_json_number(text: &str, key: &str) -> Option<f64> {
    let value_start = find_json_value_start(text, key)?;
    let value_end = text[value_start..]
        .find(|ch: char| !matches!(ch, '0'..='9' | '-' | '+' | '.' | 'e' | 'E'))
        .map(|offset| value_start + offset)
        .unwrap_or(text.len());
    text[value_start..value_end].parse::<f64>().ok()
}

fn find_json_i32(text: &str, key: &str) -> Option<i32> {
    find_json_number(text, key).map(|value| value.round() as i32)
}

fn find_json_string(text: &str, key: &str) -> Option<String> {
    let value_start = find_json_value_start(text, key)?;
    if !text[value_start..].starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in text[value_start + 1..].chars() {
        if escaped {
            match ch {
                '"' | '\\' | '/' => out.push(ch),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                _ => return None,
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return Some(out),
            _ => out.push(ch),
        }
    }
    None
}

fn find_json_value_start(text: &str, key: &str) -> Option<usize> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let colon = text[key_start..].find(':')? + key_start + 1;
    Some(skip_json_whitespace(text, colon))
}

fn skip_json_whitespace(text: &str, mut index: usize) -> usize {
    while index < text.len() && text.as_bytes()[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::{
        default_usage_persistence_state, read_claude_usage_from_path, read_openai_usage_from_path,
        resolve_usage_state_dir, sync_usage_snapshot_from_cache, CacheReadState,
    };
    use crate::usage::{ClaudeUsageData, OpenAiUsageData, UsagePersistenceState, UsageSnapshot};
    use bevy::{ecs::system::RunSystemOnce, prelude::*};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("neozeus-usage-{name}-{unique}.json"))
    }

    #[test]
    fn resolve_usage_state_dir_prefers_xdg_state_then_home_state_then_config() {
        let original_xdg_state = std::env::var_os("XDG_STATE_HOME");
        let original_home = std::env::var_os("HOME");
        let original_xdg_config = std::env::var_os("XDG_CONFIG_HOME");
        let original_explicit = std::env::var_os("NEOZEUS_STATE_DIR");

        std::env::set_var("NEOZEUS_STATE_DIR", "/tmp/neozeus-explicit");
        assert_eq!(
            resolve_usage_state_dir(),
            PathBuf::from("/tmp/neozeus-explicit")
        );
        std::env::remove_var("NEOZEUS_STATE_DIR");
        std::env::set_var("XDG_STATE_HOME", "/tmp/xdg-state-home");
        assert_eq!(
            resolve_usage_state_dir(),
            PathBuf::from("/tmp/xdg-state-home/neozeus")
        );
        std::env::remove_var("XDG_STATE_HOME");
        std::env::set_var("HOME", "/tmp/test-home");
        assert_eq!(
            resolve_usage_state_dir(),
            PathBuf::from("/tmp/test-home/.local/state/neozeus")
        );
        std::env::remove_var("HOME");
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-config-home");
        assert_eq!(
            resolve_usage_state_dir(),
            PathBuf::from("/tmp/xdg-config-home/neozeus")
        );

        match original_explicit {
            Some(value) => std::env::set_var("NEOZEUS_STATE_DIR", value),
            None => std::env::remove_var("NEOZEUS_STATE_DIR"),
        }
        match original_xdg_state {
            Some(value) => std::env::set_var("XDG_STATE_HOME", value),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        match original_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match original_xdg_config {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn default_usage_persistence_state_uses_expected_cache_filenames() {
        let state = default_usage_persistence_state();
        assert!(state.claude_cache_path.ends_with("claude-usage-cache.json"));
        assert!(state.openai_cache_path.ends_with("openai-usage-cache.json"));
        assert!(state.helper_script_path.ends_with("scripts/usage_fetch.py"));
    }

    #[test]
    fn read_claude_usage_missing_cache_returns_unavailable_state() {
        let (state, usage) = read_claude_usage_from_path(&temp_path("missing-claude"));
        assert_eq!(state, CacheReadState::Missing);
        assert_eq!(usage, ClaudeUsageData::default());
    }

    #[test]
    fn read_claude_usage_valid_cache_parses_usage_fields() {
        let path = temp_path("valid-claude");
        fs::write(&path, r#"{"five_hour":{"utilization":42.0,"resets_at":"5m"},"seven_day":{"utilization":10.0,"resets_at":"2026-03-29T16:00:00Z"},"extra_usage":{"utilization":5.0,"used_credits":100,"monthly_limit":5000}}"#).unwrap();
        let (state, usage) = read_claude_usage_from_path(&path);
        assert_eq!(state, CacheReadState::Parsed);
        assert!(usage.available);
        assert_eq!(usage.session_pct, 42.0);
        assert_eq!(usage.week_pct, 10.0);
        assert_eq!(usage.extra_pct, 5.0);
        assert_eq!(usage.extra_used, 100.0);
        assert_eq!(usage.extra_limit, 5000.0);
        assert_eq!(usage.session_resets_at, "5m");
    }

    #[test]
    fn read_claude_usage_malformed_cache_does_not_panic() {
        let path = temp_path("malformed-claude");
        fs::write(&path, r#"{"seven_day":{"utilization":24.0}}"#).unwrap();
        let (state, usage) = read_claude_usage_from_path(&path);
        assert_eq!(state, CacheReadState::Malformed);
        assert_eq!(usage, ClaudeUsageData::default());
    }

    #[test]
    fn read_openai_usage_missing_cache_returns_unavailable_state() {
        let (state, usage) = read_openai_usage_from_path(&temp_path("missing-openai"));
        assert_eq!(state, CacheReadState::Missing);
        assert_eq!(usage, OpenAiUsageData::default());
    }

    #[test]
    fn read_openai_usage_valid_cache_parses_normalized_values() {
        let path = temp_path("valid-openai");
        fs::write(&path, r#"{"requests_limit":100,"requests_remaining":60,"tokens_limit":50000,"tokens_remaining":12500,"requests_pct":40.0,"tokens_pct":75.0,"requests_resets_at":"5m","tokens_resets_at":"2h"}"#).unwrap();
        let (state, usage) = read_openai_usage_from_path(&path);
        assert_eq!(state, CacheReadState::Parsed);
        assert!(usage.available);
        assert_eq!(usage.requests_pct_milli, 40_000);
        assert_eq!(usage.tokens_pct_milli, 75_000);
        assert_eq!(usage.requests_limit, 100);
        assert_eq!(usage.tokens_remaining, 12500);
        assert_eq!(usage.requests_resets_at, "5m");
    }

    #[test]
    fn sync_usage_snapshot_from_cache_keeps_last_good_snapshot_on_malformed_cache() {
        let claude_path = temp_path("sync-claude");
        let openai_path = temp_path("sync-openai");
        fs::write(
            &claude_path,
            r#"{"five_hour":{"utilization":12.0,"resets_at":"5m"}}"#,
        )
        .unwrap();
        fs::write(&openai_path, r#"{"requests_limit":100,"requests_remaining":60,"tokens_limit":1000,"tokens_remaining":200}"#).unwrap();

        let mut world = World::default();
        world.insert_resource(UsagePersistenceState {
            state_dir: PathBuf::from("/tmp/neozeus"),
            claude_cache_path: claude_path.clone(),
            openai_cache_path: openai_path.clone(),
            claude_log_path: PathBuf::from("/tmp/neozeus/claude.log"),
            openai_log_path: PathBuf::from("/tmp/neozeus/openai.log"),
            claude_backoff_until_path: PathBuf::from("/tmp/neozeus/claude-backoff.txt"),
            helper_script_path: PathBuf::from("scripts/usage_fetch.py"),
            python_program: PathBuf::from("python3"),
            last_claude_refresh_attempt_secs: None,
            last_openai_refresh_attempt_secs: None,
        });
        world.insert_resource(UsageSnapshot::default());
        world
            .run_system_once(sync_usage_snapshot_from_cache)
            .unwrap();
        assert_eq!(world.resource::<UsageSnapshot>().claude.session_pct, 12.0);

        fs::write(&claude_path, r#"{"seven_day":{"utilization":99.0}}"#).unwrap();
        world
            .run_system_once(sync_usage_snapshot_from_cache)
            .unwrap();
        assert_eq!(world.resource::<UsageSnapshot>().claude.session_pct, 12.0);
    }
}
