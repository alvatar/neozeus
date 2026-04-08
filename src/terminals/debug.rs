use crate::app_config::resolve_debug_log_path;
use bevy::input::keyboard::KeyboardInput;
use std::{
    env, fs,
    io::Write,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Default)]
pub(crate) struct TerminalDebugStats {
    pub(crate) key_events_seen: u64,
    pub(crate) commands_queued: u64,
    #[allow(
        dead_code,
        reason = "legacy/local backend stats retained for debug parity"
    )]
    pub(crate) pty_bytes_written: u64,
    #[allow(
        dead_code,
        reason = "legacy/local backend stats retained for debug parity"
    )]
    pub(crate) pty_bytes_read: u64,
    pub(crate) snapshots_sent: u64,
    pub(crate) snapshots_applied: u64,
    pub(crate) updates_dropped: u64,
    pub(crate) dirty_rows_uploaded: u64,
    pub(crate) compose_micros: u64,
    pub(crate) last_key: String,
    pub(crate) last_command: String,
    pub(crate) last_error: String,
}

fn debug_file_logging_enabled_with(
    explicit_enable: Option<&str>,
    explicit_path: Option<&Path>,
) -> bool {
    if let Some(explicit_enable) = explicit_enable
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return matches!(
            explicit_enable.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
    }
    explicit_path.is_some()
}

/// Returns whether file-backed debug logging is explicitly enabled for this process.
///
/// File logging is opt-in because opening and appending a log file from hot UI/terminal paths adds
/// avoidable latency. Operators can still enable it explicitly either by setting
/// `NEOZEUS_ENABLE_DEBUG_LOG=1` or by providing an explicit `NEOZEUS_DEBUG_LOG_PATH`.
pub(crate) fn debug_file_logging_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        debug_file_logging_enabled_with(
            env::var("NEOZEUS_ENABLE_DEBUG_LOG").ok().as_deref(),
            env::var_os("NEOZEUS_DEBUG_LOG_PATH")
                .filter(|value| !value.is_empty())
                .as_deref()
                .map(Path::new),
        )
    })
}

/// Appends one line to the process-wide debug log if file logging is explicitly enabled and the log
/// file can be opened.
///
/// Debug logging is intentionally best-effort: failures to create or append the file are ignored so
/// instrumentation never interferes with terminal behavior. File logging is also opt-in so normal UI
/// and terminal interaction never pays per-event filesystem cost unless debugging was explicitly
/// requested.
pub(crate) fn append_debug_log(message: impl AsRef<str>) {
    if !debug_file_logging_enabled() {
        return;
    }
    let message = message.as_ref();
    let path = resolve_debug_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

/// Mutates [`TerminalDebugStats`] through a poisoned-lock-tolerant helper.
///
/// The interesting part here is the poison handling: debug statistics are diagnostic state, so even
/// if another thread panicked while holding the mutex, this helper still recovers the inner value and
/// applies the update instead of cascading the failure.
pub(crate) fn with_debug_stats(
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    update: impl FnOnce(&mut TerminalDebugStats),
) {
    if !debug_file_logging_enabled() {
        return;
    }
    match debug_stats.lock() {
        Ok(mut stats) => update(&mut stats),
        Err(poisoned) => update(&mut poisoned.into_inner()),
    }
}

/// Records a terminal error both in the text log and in the in-memory debug counters.
///
/// The string is materialized once, appended to the debug log with a stable prefix, and then stored
/// as `last_error` inside the shared stats object so later inspection can see the most recent failure
/// without parsing the file.
pub(crate) fn note_terminal_error(
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    message: impl Into<String>,
) {
    let message = message.into();
    append_debug_log(format!("terminal error: {message}"));
    with_debug_stats(debug_stats, |stats| {
        stats.last_error = message;
    });
}

/// Records one observed keyboard event for debugging and later inspection.
///
/// The event is summarized into a compact string containing key code, text payload, and logical key,
/// then written to both the debug log and the shared debug stats. This makes it easier to diagnose
/// mismatches between Bevy keyboard events and the terminal input translation layer.
pub(crate) fn note_key_event(debug_stats: &Arc<Mutex<TerminalDebugStats>>, event: &KeyboardInput) {
    if !debug_file_logging_enabled() {
        return;
    }
    let summary = format!(
        "{:?} text={:?} logical={:?}",
        event.key_code, event.text, event.logical_key
    );
    append_debug_log(format!("key event: {summary}"));
    with_debug_stats(debug_stats, |stats| {
        stats.key_events_seen += 1;
        stats.last_key = summary;
    });
}

#[cfg(test)]
mod tests {
    use super::debug_file_logging_enabled_with;
    use std::path::Path;

    #[test]
    fn debug_file_logging_defaults_to_disabled_without_explicit_opt_in() {
        assert!(!debug_file_logging_enabled_with(None, None));
    }

    #[test]
    fn debug_file_logging_explicit_path_enables_logging() {
        assert!(debug_file_logging_enabled_with(
            None,
            Some(Path::new("/tmp/neozeus-debug.log"))
        ));
    }

    #[test]
    fn debug_file_logging_enable_flag_overrides_path_presence() {
        assert!(debug_file_logging_enabled_with(Some("1"), None));
        assert!(!debug_file_logging_enabled_with(
            Some("0"),
            Some(Path::new("/tmp/neozeus-debug.log"))
        ));
    }
}
