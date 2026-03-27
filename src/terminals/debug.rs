use crate::app_config::DEBUG_LOG_PATH;
use bevy::input::keyboard::KeyboardInput;
use std::{
    fs,
    io::Write,
    sync::{Arc, Mutex},
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

/// Appends one line to the process-wide debug log if the log file can be opened.
///
/// Debug logging is intentionally best-effort: failures to create or append the file are ignored so
/// instrumentation never interferes with terminal behavior. Callers therefore use this freely in hot
/// paths and error paths alike.
pub(crate) fn append_debug_log(message: impl AsRef<str>) {
    let message = message.as_ref();
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(DEBUG_LOG_PATH)
    {
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
