use super::protocol::{DaemonEvent, DaemonSessionInfo, ServerMessage};
use crate::app_config::{DEFAULT_COLS, DEFAULT_ROWS};

use super::super::{
    ansi_surface::build_surface,
    backend::{compute_terminal_damage, send_command_payload_bytes},
    pty_spawn::{spawn_pty, write_input},
    types::{
        PtySession, TerminalCommand, TerminalDamage, TerminalDimensions, TerminalFrameUpdate,
        TerminalRuntimeState, TerminalSnapshot, TerminalSurface, TerminalUpdate,
        PTY_OUTPUT_BATCH_BYTES, PTY_OUTPUT_BATCH_WINDOW, PTY_OUTPUT_WAIT_TIMEOUT,
    },
};
use alacritty_terminal::{
    event::VoidListener,
    term::{Config as TermConfig, Term},
    vte::ansi,
};
use portable_pty::PtySize;
use std::{
    collections::HashMap,
    io::Read,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) const PERSISTENT_SESSION_PREFIX: &str = "neozeus-session-";
pub(crate) const VERIFIER_SESSION_PREFIX: &str = "neozeus-verifier-";
const DAEMON_BACKEND_STATUS: &str = "backend: neozeus daemon pty";
const DAEMON_KILL_WAIT_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg(unix)]
mod unix_signals {
    use std::os::raw::c_int;

    pub(super) const SIGHUP: c_int = 1;
    pub(super) const SIGKILL: c_int = 9;

    unsafe extern "C" {
        fn kill(pid: c_int, signal: c_int) -> c_int;
    }

    pub(super) unsafe fn send(pid: c_int, signal: c_int) -> c_int {
        kill(pid, signal)
    }
}

/// Returns whether a daemon session name belongs to the ordinary persisted-session namespace.
///
/// Verifier sessions deliberately use a separate prefix and therefore do not match here.
#[cfg(test)]
pub(crate) fn is_persistent_session_name(session_name: &str) -> bool {
    session_name.starts_with(PERSISTENT_SESSION_PREFIX)
}

pub(super) struct AttachedSubscriber {
    pub(super) snapshot: TerminalSnapshot,
    pub(super) revision: u64,
    pub(super) subscriber_id: u64,
}

pub(crate) struct DaemonSession {
    session_id: String,
    created_order: u64,
    state: Arc<Mutex<DaemonSessionState>>,
    command_tx: mpsc::Sender<DaemonSessionCommand>,
    shutdown_rx: Mutex<Option<mpsc::Receiver<Result<(), String>>>>,
}

struct DaemonSessionState {
    snapshot: TerminalSnapshot,
    revision: u64,
    subscribers: HashMap<u64, mpsc::Sender<ServerMessage>>,
}

enum DaemonSessionCommand {
    Terminal(TerminalCommand),
    Resize { cols: usize, rows: usize },
    Kill,
}

impl DaemonSession {
    /// Starts a new daemon-backed PTY session together with its worker thread.
    ///
    /// The initial snapshot is seeded with a blank surface at the default terminal size so attaches
    /// have something coherent to render before the PTY produces output.
    pub(crate) fn start(
        session_id: String,
        created_order: u64,
        cwd: Option<&str>,
    ) -> Result<Arc<Self>, String> {
        // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
        let PtySession {
            master,
            writer,
            child,
        } = spawn_pty(DEFAULT_COLS, DEFAULT_ROWS, cwd)?;
        let state = Arc::new(Mutex::new(DaemonSessionState {
            snapshot: TerminalSnapshot {
                surface: Some(TerminalSurface::new(
                    usize::from(DEFAULT_COLS),
                    usize::from(DEFAULT_ROWS),
                )),
                runtime: TerminalRuntimeState::running(DAEMON_BACKEND_STATUS),
            },
            revision: 0,
            subscribers: HashMap::new(),
        }));
        let (command_tx, command_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let session = Arc::new(Self {
            session_id: session_id.clone(),
            created_order,
            state: state.clone(),
            command_tx,
            shutdown_rx: Mutex::new(Some(shutdown_rx)),
        });

        let worker_state = state.clone();
        thread::spawn(move || {
            let result =
                run_session_worker(session_id, worker_state, command_rx, master, writer, child);
            let _ = shutdown_tx.send(result);
        });

        Ok(session)
    }

    /// Returns this daemon session's stable string id.
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Snapshots the daemon session metadata currently exposed through `ListSessions`.
    pub(crate) fn info(&self) -> DaemonSessionInfo {
        let state = lock(&self.state);
        DaemonSessionInfo {
            session_id: self.session_id.clone(),
            runtime: state.snapshot.runtime.clone(),
            revision: state.revision,
            created_order: self.created_order,
        }
    }

    /// Adds one subscriber to the session's update fan-out set and returns the current snapshot.
    ///
    /// Attach semantics are snapshot-then-stream, so the caller needs both the current snapshot and the
    /// subscriber id used for later unsubscribe.
    pub(super) fn subscribe(
        &self,
        subscriber_id: u64,
        sender: mpsc::Sender<ServerMessage>,
    ) -> AttachedSubscriber {
        let mut state = lock(&self.state);
        state.subscribers.insert(subscriber_id, sender);
        AttachedSubscriber {
            snapshot: state.snapshot.clone(),
            revision: state.revision,
            subscriber_id,
        }
    }

    /// Removes one subscriber from the session's update fan-out set.
    pub(super) fn unsubscribe(&self, subscriber_id: u64) {
        let mut state = lock(&self.state);
        state.subscribers.remove(&subscriber_id);
    }

    /// Queues one terminal command for the daemon session worker thread.
    pub(crate) fn send_command(&self, command: TerminalCommand) -> Result<(), String> {
        self.command_tx
            .send(DaemonSessionCommand::Terminal(command))
            .map_err(|_| {
                format!(
                    "daemon session `{}` command channel disconnected",
                    self.session_id
                )
            })
    }

    /// Queues a PTY resize request for the daemon session worker thread.
    pub(crate) fn resize(&self, cols: usize, rows: usize) -> Result<(), String> {
        self.command_tx
            .send(DaemonSessionCommand::Resize { cols, rows })
            .map_err(|_| {
                format!(
                    "daemon session `{}` resize channel disconnected",
                    self.session_id
                )
            })
    }

    /// Asks the daemon session worker thread to terminate the PTY session and waits until teardown completes.
    pub(crate) fn kill(&self) -> Result<(), String> {
        let shutdown_rx = lock(&self.shutdown_rx).take();
        let _ = self.command_tx.send(DaemonSessionCommand::Kill);
        let Some(shutdown_rx) = shutdown_rx else {
            return Ok(());
        };
        shutdown_rx
            .recv_timeout(DAEMON_KILL_WAIT_TIMEOUT)
            .map_err(|_| format!("daemon session `{}` kill timed out", self.session_id))?
    }
}

pub(crate) struct SubscriberIdAllocator {
    next_id: AtomicU64,
}

impl Default for SubscriberIdAllocator {
    /// Creates a subscriber-id allocator starting at 1.
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
        }
    }
}

impl SubscriberIdAllocator {
    /// Returns the next unique subscriber id for daemon event fan-out.
    pub(crate) fn next(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "daemon session worker owns PTY, parser, child lifecycle, and subscriber broadcast"
)]
/// Owns the daemon session PTY loop: read PTY output, apply queued commands, rebuild surfaces, and
/// publish updates to subscribers.
///
/// Output is batched for a short window so bursts of PTY bytes coalesce into fewer surface rebuilds
/// and fewer daemon events.
fn run_session_worker(
    session_id: String,
    state: Arc<Mutex<DaemonSessionState>>,
    command_rx: mpsc::Receiver<DaemonSessionCommand>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    mut writer: Box<dyn std::io::Write + Send>,
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
) -> Result<(), String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let mut reader = match master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            publish_update(
                &state,
                &session_id,
                TerminalUpdate::Status {
                    runtime: TerminalRuntimeState::failed(format!(
                        "failed to attach daemon PTY reader: {error}"
                    )),
                    surface: None,
                },
            );
            let _ = terminate_session_processes(master.as_ref(), &mut *child);
            return Ok(());
        }
    };

    let (pty_output_tx, pty_output_rx) = mpsc::channel::<Vec<u8>>();
    let reader_state = Arc::new(Mutex::new(None::<TerminalRuntimeState>));
    let worker_reader_state = reader_state.clone();
    let reader_thread = thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    set_reader_runtime_state(
                        &worker_reader_state,
                        TerminalRuntimeState::disconnected("daemon PTY reader reached EOF"),
                    );
                    break;
                }
                Ok(read) => {
                    if pty_output_tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    set_reader_runtime_state(
                        &worker_reader_state,
                        TerminalRuntimeState::failed(format!("daemon PTY reader error: {error}")),
                    );
                    break;
                }
            }
        }
    });

    let dimensions = TerminalDimensions {
        cols: usize::from(DEFAULT_COLS),
        rows: usize::from(DEFAULT_ROWS),
    };
    let config = TermConfig {
        scrolling_history: 5000,
        ..TermConfig::default()
    };
    let mut terminal = Term::new(config, &dimensions, VoidListener);
    let mut parser = ansi::Processor::<ansi::StdSyncHandler>::new();
    let mut previous_surface: Option<TerminalSurface> = None;
    let mut running = true;
    let mut child_reaped = false;

    while running {
        let mut received_output = false;
        let mut batched_output_bytes = 0usize;
        match pty_output_rx.recv_timeout(PTY_OUTPUT_WAIT_TIMEOUT) {
            Ok(bytes) => {
                batched_output_bytes += bytes.len();
                parser.advance(&mut terminal, &bytes);
                received_output = true;

                let batch_deadline = std::time::Instant::now() + PTY_OUTPUT_BATCH_WINDOW;
                loop {
                    while batched_output_bytes < PTY_OUTPUT_BATCH_BYTES {
                        let Ok(bytes) = pty_output_rx.try_recv() else {
                            break;
                        };
                        batched_output_bytes += bytes.len();
                        parser.advance(&mut terminal, &bytes);
                    }

                    if batched_output_bytes >= PTY_OUTPUT_BATCH_BYTES {
                        break;
                    }

                    let Some(remaining) =
                        batch_deadline.checked_duration_since(std::time::Instant::now())
                    else {
                        break;
                    };
                    if remaining.is_zero() {
                        break;
                    }

                    match pty_output_rx.recv_timeout(remaining) {
                        Ok(bytes) => {
                            batched_output_bytes += bytes.len();
                            parser.advance(&mut terminal, &bytes);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            publish_update(
                                &state,
                                &session_id,
                                TerminalUpdate::Status {
                                    runtime: TerminalRuntimeState::disconnected(
                                        "daemon PTY reader channel disconnected",
                                    ),
                                    surface: Some(build_surface(&terminal)),
                                },
                            );
                            running = false;
                            break;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                publish_update(
                    &state,
                    &session_id,
                    TerminalUpdate::Status {
                        runtime: TerminalRuntimeState::disconnected(
                            "daemon PTY reader channel disconnected",
                        ),
                        surface: Some(build_surface(&terminal)),
                    },
                );
                running = false;
            }
        }

        while let Ok(command) = command_rx.try_recv() {
            match command {
                DaemonSessionCommand::Terminal(command) => {
                    let is_scroll = matches!(command, TerminalCommand::ScrollDisplay(_));
                    if let Err(error) =
                        apply_terminal_command(master.as_ref(), &mut writer, &mut terminal, command)
                    {
                        publish_update(
                            &state,
                            &session_id,
                            TerminalUpdate::Status {
                                runtime: TerminalRuntimeState::failed(error),
                                surface: Some(build_surface(&terminal)),
                            },
                        );
                        running = false;
                    } else if is_scroll {
                        let surface = build_surface(&terminal);
                        publish_frame_update(
                            &state,
                            &session_id,
                            previous_surface.as_ref(),
                            &surface,
                        );
                        previous_surface = Some(surface);
                    }
                }
                DaemonSessionCommand::Resize { cols, rows } => {
                    if let Err(error) = resize_terminal(master.as_ref(), &mut terminal, cols, rows)
                    {
                        publish_update(
                            &state,
                            &session_id,
                            TerminalUpdate::Status {
                                runtime: TerminalRuntimeState::failed(error),
                                surface: Some(build_surface(&terminal)),
                            },
                        );
                        running = false;
                    } else {
                        let surface = build_surface(&terminal);
                        publish_frame_update(
                            &state,
                            &session_id,
                            previous_surface.as_ref(),
                            &surface,
                        );
                        previous_surface = Some(surface);
                    }
                }
                DaemonSessionCommand::Kill => {
                    if let Err(error) = terminate_session_processes(master.as_ref(), &mut *child) {
                        publish_update(
                            &state,
                            &session_id,
                            TerminalUpdate::Status {
                                runtime: TerminalRuntimeState::failed(format!(
                                    "daemon session kill failed: {error}"
                                )),
                                surface: Some(build_surface(&terminal)),
                            },
                        );
                        return Err(format!(
                            "daemon session `{session_id}` kill failed: {error}"
                        ));
                    }
                    child_reaped = true;
                    running = false;
                }
            }
        }

        let reader_runtime = lock(&reader_state).clone();
        if let Some(runtime) = reader_runtime {
            publish_update(
                &state,
                &session_id,
                TerminalUpdate::Status {
                    runtime,
                    surface: Some(build_surface(&terminal)),
                },
            );
            running = false;
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                publish_update(
                    &state,
                    &session_id,
                    TerminalUpdate::Status {
                        runtime: TerminalRuntimeState::exited(
                            format!(
                                "daemon PTY child exited: code={} signal={:?}",
                                status.exit_code(),
                                status.signal()
                            ),
                            Some(status.exit_code()),
                            status.signal().map(str::to_owned),
                        ),
                        surface: Some(build_surface(&terminal)),
                    },
                );
                child_reaped = true;
                running = false;
            }
            Ok(None) => {}
            Err(error) => {
                publish_update(
                    &state,
                    &session_id,
                    TerminalUpdate::Status {
                        runtime: TerminalRuntimeState::failed(format!(
                            "daemon PTY child wait failed: {error}"
                        )),
                        surface: Some(build_surface(&terminal)),
                    },
                );
                running = false;
            }
        }

        if received_output && running {
            let surface = build_surface(&terminal);
            publish_frame_update(&state, &session_id, previous_surface.as_ref(), &surface);
            previous_surface = Some(surface);
        }
    }

    if !child_reaped {
        let _ = terminate_session_processes(master.as_ref(), &mut *child);
    }
    drop(writer);
    drop(master);
    reader_thread
        .join()
        .map_err(|_| format!("daemon session `{session_id}` reader thread panicked"))?;
    Ok(())
}

/// Waits until the PTY child has exited or a timeout elapses.
fn wait_for_child_exit(
    child: &mut (dyn portable_pty::Child + Send + Sync),
    timeout: Duration,
) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("daemon PTY child wait failed: {error}"))?
            .is_some()
        {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Hard-stops the daemon PTY child, escalating from hangup to a group kill when needed, and waits
/// until the child has exited.
fn terminate_session_processes(
    master: &(dyn portable_pty::MasterPty + Send),
    child: &mut (dyn portable_pty::Child + Send + Sync),
) -> Result<(), String> {
    #[cfg(unix)]
    {
        if let Some(pgid) = master.process_group_leader().filter(|pgid| *pgid > 0) {
            let hup_result = unsafe { unix_signals::send(-pgid, unix_signals::SIGHUP) };
            if hup_result != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!(
                        "failed to send SIGHUP to daemon session process group {pgid}: {error}"
                    ));
                }
            }
            if wait_for_child_exit(child, Duration::from_millis(250))? {
                return Ok(());
            }

            let kill_result = unsafe { unix_signals::send(-pgid, unix_signals::SIGKILL) };
            if kill_result != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!(
                        "failed to send SIGKILL to daemon session process group {pgid}: {error}"
                    ));
                }
            }
            if wait_for_child_exit(child, Duration::from_secs(1))? {
                return Ok(());
            }
        }
    }

    child
        .kill()
        .map_err(|error| format!("daemon PTY child kill failed: {error}"))?;
    if wait_for_child_exit(child, Duration::from_secs(1))? {
        return Ok(());
    }
    Err("daemon PTY child did not exit after hard kill".to_owned())
}

/// Applies one queued terminal command to the daemon PTY session.
///
/// Text/event/command payloads are written to the PTY, while scrollback is handled locally against the
/// in-memory terminal model because it is a view-only operation.
fn apply_terminal_command(
    master: &(dyn portable_pty::MasterPty + Send),
    writer: &mut Box<dyn std::io::Write + Send>,
    terminal: &mut Term<VoidListener>,
    command: TerminalCommand,
) -> Result<(), String> {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    match command {
        TerminalCommand::InputText(text) => {
            let bytes = text.into_bytes();
            write_input(&mut **writer, &bytes)
                .map_err(|error| format!("daemon PTY write failed for text input: {error}"))
        }
        TerminalCommand::InputEvent(event) => {
            let bytes = event.into_bytes();
            write_input(&mut **writer, &bytes)
                .map_err(|error| format!("daemon PTY write failed for input event: {error}"))
        }
        TerminalCommand::SendCommand(command) => {
            let bytes = send_command_payload_bytes(&command);
            write_input(&mut **writer, &bytes).map_err(|error| {
                format!("daemon PTY write failed for command `{command}`: {error}")
            })
        }
        TerminalCommand::ScrollDisplay(lines) => {
            let _ = master;
            terminal.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
            Ok(())
        }
    }
}

/// Applies a PTY resize to both the real PTY master and the in-memory terminal model.
///
/// Dimensions are clamped to at least 1x1 so bad callers cannot request zero-sized terminals.
fn resize_terminal(
    master: &(dyn portable_pty::MasterPty + Send),
    terminal: &mut Term<VoidListener>,
    cols: usize,
    rows: usize,
) -> Result<(), String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let cols = cols.max(1);
    let rows = rows.max(1);
    master
        .resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("daemon PTY resize failed: {error}"))?;
    let dimensions = TerminalDimensions { cols, rows };
    terminal.resize(dimensions);
    Ok(())
}

/// Computes visual damage from the previous surface and publishes a frame update when anything
/// changed.
///
/// Empty row-damage updates are suppressed so idle redraws do not spam subscribers.
fn publish_frame_update(
    state: &Arc<Mutex<DaemonSessionState>>,
    session_id: &str,
    previous_surface: Option<&TerminalSurface>,
    surface: &TerminalSurface,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let damage = compute_terminal_damage(previous_surface, surface);
    if matches!(damage, TerminalDamage::Rows(ref rows) if rows.is_empty()) {
        return;
    }
    publish_update(
        state,
        session_id,
        TerminalUpdate::Frame(TerminalFrameUpdate {
            surface: surface.clone(),
            damage,
            runtime: TerminalRuntimeState::running(DAEMON_BACKEND_STATUS),
        }),
    );
}

/// Updates the session snapshot/revision and broadcasts one daemon event to all subscribers.
///
/// Dead subscriber channels are pruned opportunistically during broadcast.
fn publish_update(
    state: &Arc<Mutex<DaemonSessionState>>,
    session_id: &str,
    update: TerminalUpdate,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let mut state = lock(state);
    match &update {
        TerminalUpdate::Frame(frame) => {
            state.snapshot.runtime = frame.runtime.clone();
            state.snapshot.surface = Some(frame.surface.clone());
        }
        TerminalUpdate::Status { runtime, surface } => {
            state.snapshot.runtime = runtime.clone();
            if let Some(surface) = surface {
                state.snapshot.surface = Some(surface.clone());
            }
        }
    }
    state.revision += 1;
    let revision = state.revision;
    let event = ServerMessage::Event(DaemonEvent::SessionUpdated {
        session_id: session_id.to_owned(),
        update,
        revision,
    });
    state
        .subscribers
        .retain(|_, subscriber| subscriber.send(event.clone()).is_ok());
}

/// Stores the reader thread's terminal runtime outcome for pickup by the main worker loop.
///
/// The separate reader thread cannot publish directly because the main worker owns surface building
/// and subscriber broadcast policy.
fn set_reader_runtime_state(
    reader_state: &Arc<Mutex<Option<TerminalRuntimeState>>>,
    runtime: TerminalRuntimeState,
) {
    *lock(reader_state) = Some(runtime);
}

/// Locks this value.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
