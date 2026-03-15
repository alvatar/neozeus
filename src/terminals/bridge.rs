use crate::terminals::{
    append_debug_log, note_key_event, with_debug_stats, TerminalCommand, TerminalDebugStats,
    TerminalUpdateMailbox,
};
use bevy::input::keyboard::KeyboardInput;
use std::sync::{mpsc::Sender, Arc, Mutex};

#[derive(Clone)]
pub(crate) struct TerminalBridge {
    input_tx: Sender<TerminalCommand>,
    update_mailbox: Arc<TerminalUpdateMailbox>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
}

impl TerminalBridge {
    pub(crate) fn new(
        input_tx: Sender<TerminalCommand>,
        update_mailbox: Arc<TerminalUpdateMailbox>,
        debug_stats: Arc<Mutex<TerminalDebugStats>>,
    ) -> Self {
        Self {
            input_tx,
            update_mailbox,
            debug_stats,
        }
    }

    pub(crate) fn input_tx(&self) -> Sender<TerminalCommand> {
        self.input_tx.clone()
    }

    pub(crate) fn update_mailbox(&self) -> Arc<TerminalUpdateMailbox> {
        self.update_mailbox.clone()
    }

    pub(crate) fn debug_stats_handle(&self) -> Arc<Mutex<TerminalDebugStats>> {
        self.debug_stats.clone()
    }

    pub(crate) fn send(&self, command: TerminalCommand) {
        let summary = summarize_terminal_command(&command).to_owned();
        match self.input_tx.send(command) {
            Ok(()) => {
                append_debug_log(format!("command queued: {summary}"));
                with_debug_stats(&self.debug_stats, |stats| {
                    stats.commands_queued += 1;
                    stats.last_command = summary;
                });
            }
            Err(_) => {
                append_debug_log(format!("command queue failed: {summary}"));
                with_debug_stats(&self.debug_stats, |stats| {
                    stats.last_command = summary;
                    stats.last_error = "input channel disconnected".into();
                });
            }
        }
    }

    pub(crate) fn note_key_event(&self, event: &KeyboardInput) {
        note_key_event(&self.debug_stats, event);
    }

    pub(crate) fn note_snapshot_applied(&self) {
        with_debug_stats(&self.debug_stats, |stats| {
            stats.snapshots_applied += 1;
        });
    }

    pub(crate) fn debug_stats_snapshot(&self) -> TerminalDebugStats {
        match self.debug_stats.lock() {
            Ok(stats) => stats.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl Drop for TerminalBridge {
    fn drop(&mut self) {
        let _ = self.input_tx.send(TerminalCommand::Shutdown);
    }
}

fn summarize_terminal_command(command: &TerminalCommand) -> &str {
    match command {
        TerminalCommand::InputText(_) => "InputText",
        TerminalCommand::InputEvent(_) => "InputEvent",
        TerminalCommand::SendCommand(_) => "SendCommand",
        TerminalCommand::ScrollDisplay(_) => "ScrollDisplay",
        TerminalCommand::Shutdown => "Shutdown",
    }
}
