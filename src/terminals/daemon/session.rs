use super::protocol::{DaemonEvent, DaemonSessionInfo, ServerMessage};
use crate::{
    app_config::{DEFAULT_COLS, DEFAULT_ROWS},
    terminals::{
        build_surface, compute_terminal_damage, send_command_payload_bytes, spawn_pty, write_input,
        TerminalAttachTarget, TerminalCommand, TerminalDamage, TerminalFrameUpdate,
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
};

pub(crate) const PERSISTENT_SESSION_PREFIX: &str = "neozeus-session-";
pub(crate) const VERIFIER_SESSION_PREFIX: &str = "neozeus-verifier-";
const DAEMON_BACKEND_STATUS: &str = "backend: neozeus daemon pty";

pub(crate) fn is_persistent_session_name(session_name: &str) -> bool {
    session_name.starts_with(PERSISTENT_SESSION_PREFIX)
}

pub(crate) struct AttachedSubscriber {
    pub(crate) snapshot: TerminalSnapshot,
    pub(crate) revision: u64,
    pub(crate) subscriber_id: u64,
}

pub(crate) struct DaemonSession {
    session_id: String,
    state: Arc<Mutex<DaemonSessionState>>,
    command_tx: mpsc::Sender<DaemonSessionCommand>,
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
    pub(crate) fn start(session_id: String) -> Result<Arc<Self>, String> {
        let crate::terminals::PtySession {
            master,
            writer,
            child,
        } = spawn_pty(DEFAULT_COLS, DEFAULT_ROWS, &TerminalAttachTarget::RawShell)?;
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
        let session = Arc::new(Self {
            session_id: session_id.clone(),
            state: state.clone(),
            command_tx,
        });

        let worker_state = state.clone();
        thread::spawn(move || {
            run_session_worker(session_id, worker_state, command_rx, master, writer, child)
        });

        Ok(session)
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn info(&self) -> DaemonSessionInfo {
        let state = lock(&self.state);
        DaemonSessionInfo {
            session_id: self.session_id.clone(),
            runtime: state.snapshot.runtime.clone(),
            revision: state.revision,
        }
    }

    pub(crate) fn subscribe(
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

    pub(crate) fn unsubscribe(&self, subscriber_id: u64) {
        let mut state = lock(&self.state);
        state.subscribers.remove(&subscriber_id);
    }

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

    pub(crate) fn kill(&self) -> Result<(), String> {
        self.command_tx
            .send(DaemonSessionCommand::Kill)
            .map_err(|_| {
                format!(
                    "daemon session `{}` kill channel disconnected",
                    self.session_id
                )
            })
    }
}

pub(crate) struct SubscriberIdAllocator {
    next_id: AtomicU64,
}

impl Default for SubscriberIdAllocator {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
        }
    }
}

impl SubscriberIdAllocator {
    pub(crate) fn next(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "daemon session worker owns PTY, parser, child lifecycle, and subscriber broadcast"
)]
fn run_session_worker(
    session_id: String,
    state: Arc<Mutex<DaemonSessionState>>,
    command_rx: mpsc::Receiver<DaemonSessionCommand>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    mut writer: Box<dyn std::io::Write + Send>,
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
) {
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
            let _ = child.kill();
            return;
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

    let dimensions = crate::terminals::TerminalDimensions {
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
                    if let Err(error) = child.kill() {
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
                    }
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

    let _ = child.kill();
    let _ = reader_thread.join();
}

fn apply_terminal_command(
    master: &(dyn portable_pty::MasterPty + Send),
    writer: &mut Box<dyn std::io::Write + Send>,
    terminal: &mut Term<VoidListener>,
    command: TerminalCommand,
) -> Result<(), String> {
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

fn resize_terminal(
    master: &(dyn portable_pty::MasterPty + Send),
    terminal: &mut Term<VoidListener>,
    cols: usize,
    rows: usize,
) -> Result<(), String> {
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
    let dimensions = crate::terminals::TerminalDimensions { cols, rows };
    terminal.resize(dimensions);
    Ok(())
}

fn publish_frame_update(
    state: &Arc<Mutex<DaemonSessionState>>,
    session_id: &str,
    previous_surface: Option<&TerminalSurface>,
    surface: &TerminalSurface,
) {
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

fn publish_update(
    state: &Arc<Mutex<DaemonSessionState>>,
    session_id: &str,
    update: TerminalUpdate,
) {
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

fn set_reader_runtime_state(
    reader_state: &Arc<Mutex<Option<TerminalRuntimeState>>>,
    runtime: TerminalRuntimeState,
) {
    *lock(reader_state) = Some(runtime);
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
