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

/// Appends debug log.
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

/// Implements with debug stats.
pub(crate) fn with_debug_stats(
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    update: impl FnOnce(&mut TerminalDebugStats),
) {
    match debug_stats.lock() {
        Ok(mut stats) => update(&mut stats),
        Err(poisoned) => update(&mut poisoned.into_inner()),
    }
}

/// Notes terminal error.
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

/// Notes key event.
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
