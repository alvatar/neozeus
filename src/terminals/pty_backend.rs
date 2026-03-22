use crate::{
    app_config::{DEFAULT_COLS, DEFAULT_ROWS},
    terminals::{
        backend::{send_terminal_frame_update, send_terminal_status_update},
        build_surface, with_debug_stats, RuntimeNotifier, TerminalAttachTarget, TerminalCommand,
        TerminalDebugStats, TerminalDimensions, TerminalRuntimeState, TerminalSurface,
        TerminalUpdateMailbox, PTY_OUTPUT_BATCH_BYTES, PTY_OUTPUT_BATCH_WINDOW,
        PTY_OUTPUT_WAIT_TIMEOUT,
    },
};
use alacritty_terminal::{
    event::VoidListener,
    term::{Config as TermConfig, Term},
    vte::ansi,
};
use std::{
    io::Read,
    sync::{mpsc, mpsc::Receiver, Arc, Mutex},
    thread,
};

use super::pty_spawn::{spawn_pty, write_input};

const PTY_BACKEND_STATUS: &str = "backend: alacritty_terminal + portable-pty";

pub(crate) fn run_pty_worker(
    input_rx: Receiver<TerminalCommand>,
    update_mailbox: Arc<TerminalUpdateMailbox>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
    notifier: RuntimeNotifier,
    attach_target: TerminalAttachTarget,
) {
    let crate::terminals::PtySession {
        master,
        writer,
        mut child,
    } = match spawn_pty(DEFAULT_COLS, DEFAULT_ROWS, &attach_target) {
        Ok(session) => session,
        Err(error) => {
            send_terminal_status_update(
                &update_mailbox,
                &debug_stats,
                TerminalRuntimeState::failed(format!("failed to start PTY backend: {error}")),
                None,
                &notifier,
            );
            return;
        }
    };
    crate::terminals::append_debug_log("pty spawned successfully");

    let mut reader = match master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            send_terminal_status_update(
                &update_mailbox,
                &debug_stats,
                TerminalRuntimeState::failed(format!("failed to attach PTY reader: {error}")),
                None,
                &notifier,
            );
            let _ = child.kill();
            return;
        }
    };

    let (pty_output_tx, pty_output_rx) = mpsc::channel::<Vec<u8>>();
    let reader_state = Arc::new(Mutex::new(None::<TerminalRuntimeState>));
    let worker_reader_state = reader_state.clone();
    let reader_thread = thread::spawn(move || {
        crate::terminals::append_debug_log("pty reader thread start");
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    set_reader_runtime_state(
                        &worker_reader_state,
                        TerminalRuntimeState::disconnected("PTY reader reached EOF"),
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
                        TerminalRuntimeState::failed(format!("PTY reader error: {error}")),
                    );
                    break;
                }
            }
        }
    });

    enum InputThreadEvent {
        WriteResult(Result<(), String>),
        ScrollDisplay(i32),
    }

    let (input_status_tx, input_status_rx) = mpsc::channel::<InputThreadEvent>();
    let input_debug_stats = debug_stats.clone();
    let input_thread = thread::spawn(move || {
        let mut writer = writer;
        while let Ok(command) = input_rx.recv() {
            let event = match command {
                TerminalCommand::InputText(text) => {
                    let bytes = text.into_bytes();
                    crate::terminals::append_debug_log(format!(
                        "pty write text: {} bytes",
                        bytes.len()
                    ));
                    let result = match write_input(&mut *writer, &bytes) {
                        Ok(()) => {
                            with_debug_stats(&input_debug_stats, |stats| {
                                stats.pty_bytes_written += bytes.len() as u64;
                            });
                            Ok(())
                        }
                        Err(error) => Err(format!("PTY write failed for text input: {error}")),
                    };
                    InputThreadEvent::WriteResult(result)
                }
                TerminalCommand::InputEvent(event) => {
                    let bytes = event.into_bytes();
                    crate::terminals::append_debug_log(format!(
                        "pty write input event: {} bytes",
                        bytes.len()
                    ));
                    let result = match write_input(&mut *writer, &bytes) {
                        Ok(()) => {
                            with_debug_stats(&input_debug_stats, |stats| {
                                stats.pty_bytes_written += bytes.len() as u64;
                            });
                            Ok(())
                        }
                        Err(error) => Err(format!("PTY write failed for input event: {error}")),
                    };
                    InputThreadEvent::WriteResult(result)
                }
                TerminalCommand::SendCommand(command) => {
                    let bytes = crate::terminals::send_command_payload_bytes(&command);
                    crate::terminals::append_debug_log(format!(
                        "pty write command `{command}`: {} bytes",
                        bytes.len()
                    ));
                    let result = match write_input(&mut *writer, &bytes) {
                        Ok(()) => {
                            with_debug_stats(&input_debug_stats, |stats| {
                                stats.pty_bytes_written += bytes.len() as u64;
                            });
                            Ok(())
                        }
                        Err(error) => {
                            Err(format!("PTY write failed for command `{command}`: {error}"))
                        }
                    };
                    InputThreadEvent::WriteResult(result)
                }
                TerminalCommand::ScrollDisplay(lines) => InputThreadEvent::ScrollDisplay(lines),
            };

            if input_status_tx.send(event).is_err() {
                break;
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

    while running {
        let mut received_output = false;
        let mut batched_output_bytes = 0usize;
        match pty_output_rx.recv_timeout(PTY_OUTPUT_WAIT_TIMEOUT) {
            Ok(bytes) => {
                batched_output_bytes += bytes.len();
                apply_pty_bytes(&mut parser, &mut terminal, &debug_stats, &bytes);
                received_output = true;

                let batch_deadline = std::time::Instant::now() + PTY_OUTPUT_BATCH_WINDOW;
                loop {
                    while batched_output_bytes < PTY_OUTPUT_BATCH_BYTES {
                        let Ok(bytes) = pty_output_rx.try_recv() else {
                            break;
                        };
                        batched_output_bytes += bytes.len();
                        apply_pty_bytes(&mut parser, &mut terminal, &debug_stats, &bytes);
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
                            apply_pty_bytes(&mut parser, &mut terminal, &debug_stats, &bytes);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            send_terminal_status_update(
                                &update_mailbox,
                                &debug_stats,
                                TerminalRuntimeState::disconnected(
                                    "PTY reader channel disconnected",
                                ),
                                Some(build_surface(&terminal)),
                                &notifier,
                            );
                            running = false;
                            break;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                send_terminal_status_update(
                    &update_mailbox,
                    &debug_stats,
                    TerminalRuntimeState::disconnected("PTY reader channel disconnected"),
                    Some(build_surface(&terminal)),
                    &notifier,
                );
                running = false;
            }
        }

        while let Ok(event) = input_status_rx.try_recv() {
            match event {
                InputThreadEvent::WriteResult(Ok(())) => {}
                InputThreadEvent::WriteResult(Err(status)) => {
                    send_terminal_status_update(
                        &update_mailbox,
                        &debug_stats,
                        TerminalRuntimeState::failed(status),
                        Some(build_surface(&terminal)),
                        &notifier,
                    );
                    running = false;
                }
                InputThreadEvent::ScrollDisplay(lines) => {
                    crate::terminals::append_debug_log(format!("terminal scroll display: {lines}"));
                    terminal.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
                    let surface = build_surface(&terminal);
                    send_terminal_frame_update(
                        &update_mailbox,
                        &debug_stats,
                        &notifier,
                        previous_surface.as_ref(),
                        surface.clone(),
                        PTY_BACKEND_STATUS,
                    );
                    previous_surface = Some(surface);
                }
            }
        }

        let reader_runtime = match reader_state.lock() {
            Ok(state) => state.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(runtime) = reader_runtime {
            send_terminal_status_update(
                &update_mailbox,
                &debug_stats,
                runtime,
                Some(build_surface(&terminal)),
                &notifier,
            );
            running = false;
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                send_terminal_status_update(
                    &update_mailbox,
                    &debug_stats,
                    TerminalRuntimeState::exited(
                        format!(
                            "PTY child exited: code={} signal={:?}",
                            status.exit_code(),
                            status.signal()
                        ),
                        Some(status.exit_code()),
                        status.signal().map(str::to_owned),
                    ),
                    Some(build_surface(&terminal)),
                    &notifier,
                );
                running = false;
            }
            Ok(None) => {}
            Err(error) => {
                send_terminal_status_update(
                    &update_mailbox,
                    &debug_stats,
                    TerminalRuntimeState::failed(format!("PTY child wait failed: {error}")),
                    Some(build_surface(&terminal)),
                    &notifier,
                );
                running = false;
            }
        }

        if received_output && running {
            let surface = build_surface(&terminal);
            send_terminal_frame_update(
                &update_mailbox,
                &debug_stats,
                &notifier,
                previous_surface.as_ref(),
                surface.clone(),
                PTY_BACKEND_STATUS,
            );
            previous_surface = Some(surface);
        }
    }

    let _ = child.kill();
    let _ = reader_thread.join();
    let _ = input_thread.join();
}

fn set_reader_runtime_state(
    reader_state: &Arc<Mutex<Option<TerminalRuntimeState>>>,
    runtime: TerminalRuntimeState,
) {
    match reader_state.lock() {
        Ok(mut state) => *state = Some(runtime),
        Err(poisoned) => *poisoned.into_inner() = Some(runtime),
    }
}

fn apply_pty_bytes(
    parser: &mut ansi::Processor<ansi::StdSyncHandler>,
    terminal: &mut Term<VoidListener>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    bytes: &[u8],
) {
    with_debug_stats(debug_stats, |stats| {
        stats.pty_bytes_read += bytes.len() as u64;
    });
    parser.advance(terminal, bytes);
}
