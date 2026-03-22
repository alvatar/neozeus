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

    pub(crate) fn drain_updates(&self) -> DrainedTerminalUpdates {
        self.inner.update_mailbox.drain()
    }

    pub(crate) fn note_dropped_updates(&self, dropped_frames: u64) {
        if dropped_frames == 0 {
            return;
        }
        with_debug_stats(&self.inner.debug_stats, |stats| {
            stats.updates_dropped += dropped_frames;
        });
    }

    pub(crate) fn note_key_event(&self, event: &KeyboardInput) {
        note_key_event(&self.inner.debug_stats, event);
    }

    pub(crate) fn note_snapshot_applied(&self) {
        with_debug_stats(&self.inner.debug_stats, |stats| {
            stats.snapshots_applied += 1;
        });
    }

    pub(crate) fn note_compose(&self, dirty_rows: usize, compose_micros: u64) {
        with_debug_stats(&self.inner.debug_stats, |stats| {
            stats.compose_micros += compose_micros;
            stats.dirty_rows_uploaded += dirty_rows as u64;
        });
    }

    pub(crate) fn debug_stats_snapshot(&self) -> TerminalDebugStats {
        match self.inner.debug_stats.lock() {
            Ok(stats) => stats.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

fn summarize_terminal_command(command: &TerminalCommand) -> &str {
    match command {
        TerminalCommand::InputText(_) => "InputText",
        TerminalCommand::InputEvent(_) => "InputEvent",
        TerminalCommand::SendCommand(_) => "SendCommand",
        TerminalCommand::ScrollDisplay(_) => "ScrollDisplay",
    }
}
