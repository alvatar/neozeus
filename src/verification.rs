use crate::terminals::{
    append_debug_log, with_debug_stats, RuntimeNotifier, TerminalBridge, TerminalCommand,
};
use bevy::prelude::Resource;
use std::{env, thread, time::Duration};

#[derive(Resource, Clone)]
pub(crate) struct AutoVerifyConfig {
    pub(crate) command: String,
    pub(crate) delay_ms: u64,
}

impl AutoVerifyConfig {
    pub(crate) fn from_env() -> Option<Self> {
        Some(Self {
            command: env::var("NEOZEUS_AUTOVERIFY_COMMAND").ok()?,
            delay_ms: env::var("NEOZEUS_AUTOVERIFY_DELAY_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1500),
        })
    }
}

pub(crate) fn start_auto_verify_dispatcher(
    bridge: TerminalBridge,
    notifier: RuntimeNotifier,
    config: AutoVerifyConfig,
) {
    let input_tx = bridge.input_tx();
    let debug_stats = bridge.debug_stats_handle();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(config.delay_ms));
        append_debug_log(format!(
            "auto-verify command dispatched: {}",
            config.command
        ));
        match input_tx.send(TerminalCommand::SendCommand(config.command.clone())) {
            Ok(()) => {
                with_debug_stats(&debug_stats, |stats| {
                    stats.commands_queued += 1;
                    stats.last_command = "SendCommand".into();
                });
                append_debug_log("command queued: SendCommand");
                notifier.wake();
            }
            Err(_) => {
                append_debug_log("command queue failed: SendCommand");
                with_debug_stats(&debug_stats, |stats| {
                    stats.last_command = "SendCommand".into();
                    stats.last_error = "input channel disconnected".into();
                });
            }
        }
    });
}
