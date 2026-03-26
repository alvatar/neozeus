use crate::terminals::{
    append_debug_log, note_terminal_error, with_debug_stats, AttachedDaemonSession,
    DaemonSessionInfo, TerminalBridge, TerminalCommand, TerminalDaemonClientResource,
    TerminalDebugStats, TerminalRuntimeState, TerminalUpdate, TerminalUpdateMailbox,
    PERSISTENT_SESSION_PREFIX,
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
    proxy: Option<EventLoopProxy<WinitUserEvent>>,
}

impl RuntimeNotifier {
    /// Constructs a new value.
    pub(crate) fn new(proxy: EventLoopProxy<WinitUserEvent>) -> Self {
        Self { proxy: Some(proxy) }
    }

    /// Implements noop.
    pub(crate) fn noop() -> Self {
        Self { proxy: None }
    }

    /// Implements wake.
    pub(crate) fn wake(&self) {
        if let Some(proxy) = &self.proxy {
            let _ = proxy.send_event(WinitUserEvent::WakeUp);
        }
    }
}

#[derive(Resource, Clone)]
pub(crate) struct TerminalRuntimeSpawner {
    notifier: RuntimeNotifier,
    daemon: TerminalDaemonClientResource,
}

/// Returns whether bootstrap spawned agent.
fn should_bootstrap_spawned_agent(prefix: &str) -> bool {
    prefix == PERSISTENT_SESSION_PREFIX
}

impl TerminalRuntimeSpawner {
    /// Constructs a new value.
    pub(crate) fn new(
        proxy: EventLoopProxy<WinitUserEvent>,
        daemon: TerminalDaemonClientResource,
    ) -> Self {
        Self {
            notifier: RuntimeNotifier::new(proxy),
            daemon,
        }
    }

    /// Implements headless.
    pub(crate) fn headless(daemon: TerminalDaemonClientResource) -> Self {
        Self {
            notifier: RuntimeNotifier::noop(),
            daemon,
        }
    }

    /// Implements for tests.
    #[cfg(test)]
    pub(crate) fn for_tests(daemon: TerminalDaemonClientResource) -> Self {
        Self::headless(daemon)
    }

    /// Implements notifier.
    pub(crate) fn notifier(&self) -> RuntimeNotifier {
        self.notifier.clone()
    }

    /// Implements list session infos.
    pub(crate) fn list_session_infos(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        self.daemon.client().list_sessions()
    }

    /// Creates session.
    pub(crate) fn create_session(&self, prefix: &str) -> Result<String, String> {
        let session_id = self.create_shell_session(prefix)?;
        if should_bootstrap_spawned_agent(prefix) {
            if let Err(error) = self
                .daemon
                .client()
                .send_command(&session_id, TerminalCommand::SendCommand("pi".into()))
            {
                let _ = self.daemon.client().kill_session(&session_id);
                return Err(format!(
                    "failed to start pi in session `{session_id}`: {error}"
                ));
            }
            append_debug_log(format!("bootstrapped pi session={session_id}"));
        }
        Ok(session_id)
    }

    /// Creates shell session.
    pub(crate) fn create_shell_session(&self, prefix: &str) -> Result<String, String> {
        self.daemon.client().create_session(prefix)
    }

    /// Kills session.
    pub(crate) fn kill_session(&self, session_id: &str) -> Result<(), String> {
        self.daemon.client().kill_session(session_id)
    }

    /// Resizes session.
    pub(crate) fn resize_session(
        &self,
        session_id: &str,
        cols: usize,
        rows: usize,
    ) -> Result<(), String> {
        self.daemon.client().resize_session(session_id, cols, rows)
    }

    /// Spawns attached.
    pub(crate) fn spawn_attached(&self, session_id: &str) -> Result<TerminalBridge, String> {
        let attached = self.daemon.client().attach_session(session_id)?;
        Ok(spawn_daemon_terminal_runtime(
            self.notifier.clone(),
            self.daemon.clone(),
            session_id.to_owned(),
            attached,
        ))
    }
}

/// Spawns daemon terminal runtime.
pub(crate) fn spawn_daemon_terminal_runtime(
    notifier: RuntimeNotifier,
    daemon: TerminalDaemonClientResource,
    session_id: String,
    attached: AttachedDaemonSession,
) -> TerminalBridge {
    let (input_tx, input_rx) = mpsc::channel();
    let update_mailbox = Arc::new(TerminalUpdateMailbox::default());
    let debug_stats = Arc::new(Mutex::new(TerminalDebugStats::default()));
    let bridge = TerminalBridge::new(input_tx, update_mailbox.clone(), debug_stats.clone());

    push_initial_snapshot(
        &update_mailbox,
        &debug_stats,
        &notifier,
        attached.snapshot.clone(),
    );

    let command_mailbox = update_mailbox.clone();
    let command_debug_stats = debug_stats.clone();
    let command_notifier = notifier.clone();
    let command_daemon = daemon.clone();
    let command_session_id = session_id.clone();
    thread::spawn(move || {
        while let Ok(command) = input_rx.recv() {
            if let Err(error) = command_daemon
                .client()
                .send_command(&command_session_id, command)
            {
                publish_runtime_status(
                    &command_mailbox,
                    &command_debug_stats,
                    &command_notifier,
                    TerminalRuntimeState::failed(error.clone()),
                );
                note_terminal_error(&command_debug_stats, error);
                break;
            }
        }
    });

    let update_notifier = notifier.clone();
    let update_debug_stats = debug_stats.clone();
    let update_mailbox_thread = update_mailbox.clone();
    thread::spawn(move || {
        while let Ok(update) = attached.updates.recv() {
            let push = update_mailbox_thread.push(update);
            with_debug_stats(&update_debug_stats, |stats| {
                stats.snapshots_sent += 1;
            });
            if push.should_wake {
                update_notifier.wake();
            }
        }
        publish_runtime_status(
            &update_mailbox_thread,
            &update_debug_stats,
            &update_notifier,
            TerminalRuntimeState::disconnected("daemon update stream disconnected"),
        );
    });

    append_debug_log(format!("spawned daemon bridge session={session_id}"));
    bridge
}

/// Pushes initial snapshot.
fn push_initial_snapshot(
    update_mailbox: &Arc<TerminalUpdateMailbox>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    notifier: &RuntimeNotifier,
    snapshot: crate::terminals::TerminalSnapshot,
) {
    let push = update_mailbox.push(TerminalUpdate::Status {
        runtime: snapshot.runtime,
        surface: snapshot.surface,
    });
    with_debug_stats(debug_stats, |stats| {
        stats.snapshots_sent += 1;
    });
    if push.should_wake {
        notifier.wake();
    }
}

/// Publishes runtime status.
fn publish_runtime_status(
    update_mailbox: &Arc<TerminalUpdateMailbox>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    notifier: &RuntimeNotifier,
    runtime: TerminalRuntimeState,
) {
    let push = update_mailbox.push(TerminalUpdate::Status {
        runtime,
        surface: None,
    });
    with_debug_stats(debug_stats, |stats| {
        stats.snapshots_sent += 1;
    });
    if push.should_wake {
        notifier.wake();
    }
}
