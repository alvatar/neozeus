use crate::terminals::{
    append_debug_log, note_key_event, with_debug_stats, DrainedTerminalUpdates, TerminalCommand,
    TerminalDebugStats, TerminalUpdateMailbox,
};
use bevy::input::keyboard::KeyboardInput;
use std::sync::{mpsc::Sender, Arc, Mutex};

struct TerminalBridgeInner {
    input_tx: Sender<TerminalCommand>,
    update_mailbox: Arc<TerminalUpdateMailbox>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
}

#[derive(Clone)]
pub(crate) struct TerminalBridge {
    inner: Arc<TerminalBridgeInner>,
}

impl TerminalBridge {
    /// Creates a bridge that connects app-side systems to one terminal runtime instance.
    ///
    /// The bridge owns three shared pieces of state: the command sender used to talk to the runtime
    /// worker, the coalescing update mailbox used to receive snapshots/status, and the debug stats
    /// mutex used for instrumentation.
    pub(crate) fn new(
        input_tx: Sender<TerminalCommand>,
        update_mailbox: Arc<TerminalUpdateMailbox>,
        debug_stats: Arc<Mutex<TerminalDebugStats>>,
    ) -> Self {
        Self {
            inner: Arc::new(TerminalBridgeInner {
                input_tx,
                update_mailbox,
                debug_stats,
            }),
        }
    }

    /// Queues one terminal command for the runtime worker and updates debug instrumentation.
    ///
    /// The command itself is sent through the `mpsc` channel. On success the bridge records a queued-
    /// command count and last-command summary; on failure it records the failure in the debug stats so
    /// disconnected runtimes are visible during debugging.
    pub(crate) fn send(&self, command: TerminalCommand) {
        let summary = summarize_terminal_command(&command).to_owned();
        match self.inner.input_tx.send(command) {
            Ok(()) => {
                append_debug_log(format!("command queued: {summary}"));
                with_debug_stats(&self.inner.debug_stats, |stats| {
                    stats.commands_queued += 1;
                    stats.last_command = summary;
                });
            }
            Err(_) => {
                append_debug_log(format!("command queue failed: {summary}"));
                with_debug_stats(&self.inner.debug_stats, |stats| {
                    stats.last_command = summary;
                    stats.last_error = "input channel disconnected".into();
                });
            }
        }
    }

    /// Drains the coalesced update mailbox for this terminal.
    ///
    /// The bridge does not interpret the updates; it simply exposes the mailbox drain operation to the
    /// polling layer that will merge the newest frame and status into authoritative terminal state.
    pub(crate) fn drain_updates(&self) -> DrainedTerminalUpdates {
        self.inner.update_mailbox.drain()
    }

    /// Accumulates the number of coalesced-away frame updates observed during polling.
    ///
    /// Zero is treated as a no-op so callers can report the mailbox result blindly without causing any
    /// extra lock traffic in the common case.
    pub(crate) fn note_dropped_updates(&self, dropped_frames: u64) {
        if dropped_frames == 0 {
            return;
        }
        with_debug_stats(&self.inner.debug_stats, |stats| {
            stats.updates_dropped += dropped_frames;
        });
    }

    /// Records an input event in this terminal's debug stats.
    ///
    /// This forwards to the shared debug helper so all keyboard-event logging uses the same summary
    /// format regardless of which subsystem observed the key first.
    pub(crate) fn note_key_event(&self, event: &KeyboardInput) {
        note_key_event(&self.inner.debug_stats, event);
    }

    /// Increments the counter tracking how many snapshots from this runtime have been applied.
    ///
    /// This is purely diagnostic state, used to correlate runtime activity with renderer/poller
    /// behavior when debugging missed or delayed updates.
    pub(crate) fn note_snapshot_applied(&self) {
        with_debug_stats(&self.inner.debug_stats, |stats| {
            stats.snapshots_applied += 1;
        });
    }

    /// Accumulates rasterization cost metrics for this terminal.
    ///
    /// The bridge stores total composed microseconds and total dirty rows uploaded so debugging can
    /// answer not just "did we render" but roughly how much work each terminal caused.
    pub(crate) fn note_compose(&self, dirty_rows: usize, compose_micros: u64) {
        with_debug_stats(&self.inner.debug_stats, |stats| {
            stats.compose_micros += compose_micros;
            stats.dirty_rows_uploaded += dirty_rows as u64;
        });
    }

    /// Returns a cloned snapshot of the current debug statistics for this terminal.
    ///
    /// Poisoned locks are recovered instead of propagating an error, because inspection tooling should
    /// still be able to see the last known counters even after a panic in another holder.
    pub(crate) fn debug_stats_snapshot(&self) -> TerminalDebugStats {
        match self.inner.debug_stats.lock() {
            Ok(stats) => stats.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

/// Produces a short stable label for a terminal command variant.
///
/// The summary is used exclusively for logging and debug stats, where the variant name is more
/// useful than the full payload and avoids leaking large command strings into every instrumentation
/// entry.
fn summarize_terminal_command(command: &TerminalCommand) -> &str {
    match command {
        TerminalCommand::InputText(_) => "InputText",
        TerminalCommand::InputEvent(_) => "InputEvent",
        TerminalCommand::SendCommand(_) => "SendCommand",
        TerminalCommand::ScrollDisplay(_) => "ScrollDisplay",
    }
}
