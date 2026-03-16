use super::backend::terminal_worker;
use crate::terminals::{
    append_debug_log, note_terminal_error, TerminalAttachTarget, TerminalBridge,
    TerminalDebugStats, TerminalRuntimeState, TerminalUpdate, TerminalUpdateMailbox,
};
use bevy::{
    prelude::Resource,
    winit::{EventLoopProxy, WinitUserEvent},
};
use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
};

#[derive(Clone)]
pub(crate) struct RuntimeNotifier {
    proxy: EventLoopProxy<WinitUserEvent>,
}

impl RuntimeNotifier {
    pub(crate) fn new(proxy: EventLoopProxy<WinitUserEvent>) -> Self {
        Self { proxy }
    }

    pub(crate) fn wake(&self) {
        let _ = self.proxy.send_event(WinitUserEvent::WakeUp);
    }
}

#[derive(Resource, Clone)]
pub(crate) struct TerminalRuntimeSpawner {
    notifier: RuntimeNotifier,
}

impl TerminalRuntimeSpawner {
    pub(crate) fn new(proxy: EventLoopProxy<WinitUserEvent>) -> Self {
        Self {
            notifier: RuntimeNotifier::new(proxy),
        }
    }

    pub(crate) fn notifier(&self) -> RuntimeNotifier {
        self.notifier.clone()
    }

    pub(crate) fn spawn_attached(&self, attach_target: TerminalAttachTarget) -> TerminalBridge {
        spawn_terminal_runtime(self.notifier.clone(), attach_target)
    }
}

pub(crate) fn spawn_terminal_runtime(
    notifier: RuntimeNotifier,
    attach_target: TerminalAttachTarget,
) -> TerminalBridge {
    let (input_tx, input_rx) = mpsc::channel();
    let update_mailbox = Arc::new(TerminalUpdateMailbox::default());
    let debug_stats = Arc::new(Mutex::new(TerminalDebugStats::default()));
    let bridge = TerminalBridge::new(input_tx, update_mailbox.clone(), debug_stats.clone());

    let worker_mailbox = update_mailbox.clone();
    let worker_debug_stats = debug_stats.clone();
    let worker_notifier = notifier.clone();
    let worker_attach_target = attach_target.clone();
    thread::spawn(move || {
        append_debug_log("terminal worker thread spawn");
        let panic_mailbox = worker_mailbox.clone();
        let panic_debug_stats = worker_debug_stats.clone();
        let panic_notifier = worker_notifier.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            terminal_worker(
                input_rx,
                worker_mailbox,
                worker_debug_stats,
                worker_notifier,
                worker_attach_target,
            )
        }));
        if let Err(payload) = result {
            let message = panic_payload_to_string(payload);
            append_debug_log(format!("terminal worker panic: {message}"));
            let push = panic_mailbox.push(TerminalUpdate::Status {
                runtime: TerminalRuntimeState::failed(format!(
                    "terminal worker panicked: {message}"
                )),
                surface: None,
            });
            crate::terminals::with_debug_stats(&panic_debug_stats, |stats| {
                stats.snapshots_sent += 1;
            });
            note_terminal_error(
                &panic_debug_stats,
                format!("terminal worker panicked: {message}"),
            );
            if push.should_wake {
                panic_notifier.wake();
            }
        }
    });

    bridge
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_owned(),
            Err(_) => "unknown panic payload".to_owned(),
        },
    }
}
