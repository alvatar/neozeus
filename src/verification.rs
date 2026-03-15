use crate::terminals::{append_debug_log, RuntimeNotifier, TerminalBridge, TerminalCommand};
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
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(config.delay_ms));
        append_debug_log(format!(
            "auto-verify command dispatched: {}",
            config.command
        ));
        bridge.send(TerminalCommand::SendCommand(config.command));
        notifier.wake();
    });
}
