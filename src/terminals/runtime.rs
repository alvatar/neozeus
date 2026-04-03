use super::{
    bridge::TerminalBridge,
    daemon::{AttachedDaemonSession, DaemonSessionInfo, TerminalDaemonClientResource},
    debug::{append_debug_log, note_terminal_error, with_debug_stats, TerminalDebugStats},
    mailbox::TerminalUpdateMailbox,
    types::{TerminalCommand, TerminalRuntimeState, TerminalSnapshot, TerminalUpdate},
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
    /// Builds a notifier backed by Bevy's Winit event-loop proxy.
    ///
    /// In windowed mode this is how background runtime threads poke the main loop to wake up and poll
    /// newly arrived terminal updates.
    pub(crate) fn new(proxy: EventLoopProxy<WinitUserEvent>) -> Self {
        Self { proxy: Some(proxy) }
    }

    /// Builds a notifier that intentionally does nothing when woken.
    ///
    /// Headless/offscreen execution uses this because there is no Winit event loop to notify.
    fn noop() -> Self {
        Self { proxy: None }
    }

    /// Requests that the main application loop wake up and process terminal work.
    ///
    /// The call is best-effort: if no proxy exists or sending fails, nothing is propagated because the
    /// next normal frame/update will eventually observe the pending state anyway.
    pub(crate) fn wake(&self) {
        if let Some(proxy) = &self.proxy {
            let _ = proxy.send_event(WinitUserEvent::WakeUp);
        }
    }
}

#[derive(Resource, Clone)]
pub(crate) struct TerminalRuntimeSpawner {
    notifier: RuntimeNotifier,
    daemon: Arc<Mutex<Option<TerminalDaemonClientResource>>>,
}

impl TerminalRuntimeSpawner {
    /// Builds a spawner that can wake the app but does not yet have a live daemon connection.
    pub(crate) fn pending(proxy: EventLoopProxy<WinitUserEvent>) -> Self {
        Self {
            notifier: RuntimeNotifier::new(proxy),
            daemon: Arc::new(Mutex::new(None)),
        }
    }

    /// Builds a headless spawner whose daemon connection will be installed later.
    pub(crate) fn pending_headless() -> Self {
        Self {
            notifier: RuntimeNotifier::noop(),
            daemon: Arc::new(Mutex::new(None)),
        }
    }

    /// Test-only constructor that installs a ready daemon into a headless spawner.
    #[cfg(test)]
    pub(crate) fn for_tests(daemon: TerminalDaemonClientResource) -> Self {
        let spawner = Self::pending_headless();
        spawner.install_daemon(daemon);
        spawner
    }

    /// Returns whether a live daemon client has been installed.
    pub(crate) fn is_ready(&self) -> bool {
        self.daemon
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .is_some()
    }

    /// Installs the live daemon client once background startup connection completes.
    pub(crate) fn install_daemon(&self, daemon: TerminalDaemonClientResource) {
        if let Ok(mut guard) = self.daemon.lock() {
            *guard = Some(daemon);
        }
        self.notifier.wake();
    }

    /// Returns the daemon client backing this runtime spawner.
    fn daemon_client(&self) -> Result<TerminalDaemonClientResource, String> {
        self.daemon
            .lock()
            .map_err(|_| "terminal runtime spawner poisoned".to_owned())?
            .clone()
            .ok_or_else(|| "terminal runtime still connecting".to_owned())
    }

    /// Returns a clone of the runtime notifier used by this spawner.
    ///
    /// Callers use this when background helpers need wake-up access without also needing full daemon
    /// spawning authority.
    pub(crate) fn notifier(&self) -> RuntimeNotifier {
        self.notifier.clone()
    }

    /// Lists the daemon's current sessions through the underlying daemon client.
    ///
    /// The spawner is the app's authority for session creation/attachment, so it also exposes session
    /// discovery instead of forcing callers to reach around it to the daemon resource.
    pub(crate) fn list_session_infos(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        self.daemon_client()?.client().list_sessions()
    }

    /// Creates a new session and optionally bootstraps it into one agent CLI.
    ///
    /// The method first creates a plain shell session, then conditionally sends one startup command
    /// such as `pi`, `claude`, or `codex`. If bootstrapping fails, the just-created daemon session is
    /// killed so the caller does not inherit a half-initialized shell.
    pub(crate) fn create_session(
        &self,
        prefix: &str,
        startup_command: Option<&str>,
    ) -> Result<String, String> {
        self.create_session_with_cwd(prefix, None, startup_command)
    }

    /// Creates a new session in the requested working directory and optionally bootstraps one agent CLI.
    pub(crate) fn create_session_with_cwd(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        startup_command: Option<&str>,
    ) -> Result<String, String> {
        self.create_session_with_cwd_and_env(prefix, cwd, startup_command, &[])
    }

    /// Creates a new session in the requested working directory with explicit env overrides and
    /// optionally bootstraps one agent CLI.
    pub(crate) fn create_session_with_cwd_and_env(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        startup_command: Option<&str>,
        env_overrides: &[(String, String)],
    ) -> Result<String, String> {
        let daemon = self.daemon_client()?;
        let session_id = self.create_shell_session_with_cwd_and_env(prefix, cwd, env_overrides)?;
        if let Some(startup_command) = startup_command {
            if let Err(error) = daemon.client().send_command(
                &session_id,
                TerminalCommand::SendCommand(startup_command.into()),
            ) {
                let _ = daemon.client().kill_session(&session_id);
                return Err(format!(
                    "failed to start {startup_command} in session `{session_id}`: {error}"
                ));
            }
            append_debug_log(format!(
                "bootstrapped {startup_command} session={session_id}"
            ));
        }
        Ok(session_id)
    }

    /// Creates a raw shell session with explicit env overrides.
    pub(crate) fn create_shell_session_with_cwd_and_env(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        env_overrides: &[(String, String)],
    ) -> Result<String, String> {
        self.daemon_client()?
            .client()
            .create_session_with_env(prefix, cwd, env_overrides)
    }

    /// Asks the daemon to resize one named session to the requested terminal grid.
    pub(crate) fn resize_session(
        &self,
        session_id: &str,
        cols: usize,
        rows: usize,
    ) -> Result<(), String> {
        self.daemon_client()?
            .client()
            .resize_session(session_id, cols, rows)
    }

    /// Asks the daemon to kill one named session.
    ///
    /// This is a thin delegation method kept on the spawner so the rest of the app can treat the
    /// spawner as the single runtime/session façade.
    pub(crate) fn kill_session(&self, session_id: &str) -> Result<(), String> {
        self.daemon_client()?.client().kill_session(session_id)
    }

    /// Attaches to an existing daemon session and launches the local runtime bridge threads for it.
    ///
    /// The daemon supplies the initial snapshot plus the streamed update receiver; this method wraps
    /// both in the app-side [`TerminalBridge`] abstraction.
    pub(crate) fn spawn_attached(&self, session_id: &str) -> Result<TerminalBridge, String> {
        let daemon = self.daemon_client()?;
        let attached = daemon.client().attach_session(session_id)?;
        Ok(spawn_daemon_terminal_runtime(
            self.notifier.clone(),
            daemon,
            session_id.to_owned(),
            attached,
        ))
    }
}

/// Creates the app-side runtime bridge for one attached daemon session.
///
/// The function sets up the command channel, the coalescing update mailbox, and the debug stats, then
/// spawns two background threads: one forwards outgoing commands to the daemon, and the other drains
/// streamed daemon updates into the mailbox. The returned [`TerminalBridge`] is what the rest of the
/// app talks to.
fn spawn_daemon_terminal_runtime(
    notifier: RuntimeNotifier,
    daemon: TerminalDaemonClientResource,
    session_id: String,
    attached: AttachedDaemonSession,
) -> TerminalBridge {
    let (input_tx, input_rx) = mpsc::channel();
    let update_mailbox = Arc::new(TerminalUpdateMailbox::default());
    let debug_stats = Arc::new(Mutex::new(TerminalDebugStats::default()));
    let bridge = TerminalBridge::new(input_tx, update_mailbox.clone(), debug_stats.clone());

    // Seed the mailbox with the daemon's initial snapshot before the background threads start so the
    // app can present something immediately after attachment.
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
            let should_wake = update_mailbox_thread.push(update);
            with_debug_stats(&update_debug_stats, |stats| {
                stats.snapshots_sent += 1;
            });
            if should_wake {
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

/// Pushes the daemon's initial snapshot into the update mailbox and wakes the app if needed.
///
/// The snapshot is converted into a `Status` update because it contains both runtime state and an
/// optional full surface. Debug stats track it as one sent snapshot so startup attachment has the same
/// accounting shape as streamed updates.
fn push_initial_snapshot(
    update_mailbox: &Arc<TerminalUpdateMailbox>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    notifier: &RuntimeNotifier,
    snapshot: TerminalSnapshot,
) {
    let should_wake = update_mailbox.push(TerminalUpdate::Status {
        runtime: snapshot.runtime,
        surface: snapshot.surface,
    });
    with_debug_stats(debug_stats, |stats| {
        stats.snapshots_sent += 1;
    });
    if should_wake {
        notifier.wake();
    }
}

/// Publishes a status-only runtime update into the mailbox and wakes the app if necessary.
///
/// This is used for important runtime state transitions such as command-send failure or daemon-stream
/// disconnection, where there may be no new surface but the UI still needs to reflect a new runtime
/// status immediately.
fn publish_runtime_status(
    update_mailbox: &Arc<TerminalUpdateMailbox>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    notifier: &RuntimeNotifier,
    runtime: TerminalRuntimeState,
) {
    let should_wake = update_mailbox.push(TerminalUpdate::Status {
        runtime,
        surface: None,
    });
    with_debug_stats(debug_stats, |stats| {
        stats.snapshots_sent += 1;
    });
    if should_wake {
        notifier.wake();
    }
}
