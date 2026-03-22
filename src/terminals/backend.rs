use crate::{
    app_config::{DEFAULT_COLS, DEFAULT_ROWS},
    terminals::{
        append_debug_log, build_attach_command_argv, capture_pane_tmux_command,
        note_terminal_error, pane_state_tmux_command, send_bytes_tmux_commands, with_debug_stats,
        PtySession, RuntimeNotifier, TerminalAttachTarget, TerminalCell, TerminalCellContent,
        TerminalCommand, TerminalCursor, TerminalCursorShape, TerminalDamage, TerminalDimensions,
        TerminalFrameUpdate, TerminalRuntimeState, TerminalSurface, TerminalUpdate,
        TerminalUpdateMailbox, PTY_OUTPUT_BATCH_BYTES, PTY_OUTPUT_BATCH_WINDOW,
        PTY_OUTPUT_WAIT_TIMEOUT,
    },
};
use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    term::{cell::Flags, color::Colors, Config as TermConfig, Term},
    vte::ansi::{self, Color as AnsiColor, CursorShape, NamedColor, Rgb},
};
use bevy_egui::egui;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    io::{Read, Write},
    process::Command,
    sync::{mpsc, mpsc::Receiver, Arc, Mutex},
    thread,
    time::Duration,
};

use crate::terminals::TerminalDebugStats;

fn enqueue_terminal_update(
    update_mailbox: &Arc<TerminalUpdateMailbox>,
    update: TerminalUpdate,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    notifier: &RuntimeNotifier,
) {
    let push = update_mailbox.push(update);
    with_debug_stats(debug_stats, |stats| {
        stats.snapshots_sent += 1;
    });
    if push.should_wake {
        notifier.wake();
    }
}

fn send_terminal_status_update(
    update_mailbox: &Arc<TerminalUpdateMailbox>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    runtime: TerminalRuntimeState,
    surface: Option<TerminalSurface>,
    notifier: &RuntimeNotifier,
) {
    append_debug_log(format!("status snapshot: {}", runtime.status));
    if let Some(error) = runtime.last_error.clone() {
        note_terminal_error(debug_stats, error);
    }
    enqueue_terminal_update(
        update_mailbox,
        TerminalUpdate::Status { runtime, surface },
        debug_stats,
        notifier,
    );
}

fn send_terminal_frame_update(
    update_mailbox: &Arc<TerminalUpdateMailbox>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    notifier: &RuntimeNotifier,
    previous_surface: Option<&TerminalSurface>,
    surface: TerminalSurface,
    backend_status: &str,
) {
    let damage = compute_terminal_damage(previous_surface, &surface);
    if matches!(damage, TerminalDamage::Rows(ref rows) if rows.is_empty()) {
        return;
    }
    enqueue_terminal_update(
        update_mailbox,
        TerminalUpdate::Frame(TerminalFrameUpdate {
            surface,
            damage,
            runtime: TerminalRuntimeState::running(backend_status),
        }),
        debug_stats,
        notifier,
    );
}

const TMUX_VIEWER_POLL_INTERVAL: Duration = Duration::from_millis(33);
const TMUX_VIEWER_HISTORY_LIMIT: usize = 4096;
const PTY_BACKEND_STATUS: &str = "backend: alacritty_terminal + portable-pty";
const TMUX_VIEWER_BACKEND_STATUS: &str = "backend: tmux detached session viewer";

struct TmuxPaneSnapshot {
    cols: usize,
    rows: usize,
    cursor_x: usize,
    cursor_y: usize,
    cursor_visible: bool,
    lines: Vec<String>,
}

pub(crate) fn terminal_worker(
    input_rx: Receiver<TerminalCommand>,
    update_mailbox: Arc<TerminalUpdateMailbox>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
    notifier: RuntimeNotifier,
    attach_target: TerminalAttachTarget,
) {
    if let TerminalAttachTarget::TmuxViewer { session_name } = &attach_target {
        tmux_viewer_worker(
            input_rx,
            update_mailbox,
            debug_stats,
            notifier,
            session_name,
        );
        return;
    }

    let PtySession {
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
    append_debug_log("pty spawned successfully");

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
        append_debug_log("pty reader thread start");
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
                    append_debug_log(format!("pty write text: {} bytes", bytes.len()));
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
                    append_debug_log(format!("pty write input event: {} bytes", bytes.len()));
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
                    let payload = format!("{command}\r");
                    let bytes = payload.into_bytes();
                    append_debug_log(format!(
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
                    append_debug_log(format!("terminal scroll display: {lines}"));
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

fn tmux_viewer_worker(
    input_rx: Receiver<TerminalCommand>,
    update_mailbox: Arc<TerminalUpdateMailbox>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
    notifier: RuntimeNotifier,
    session_name: &str,
) {
    append_debug_log(format!("tmux viewer worker start session={session_name}"));
    let mut previous_surface: Option<TerminalSurface> = None;
    let mut scroll_offset_lines = 0usize;

    loop {
        match input_rx.recv_timeout(TMUX_VIEWER_POLL_INTERVAL) {
            Ok(command) => {
                if let Err(error) = apply_tmux_viewer_command(
                    session_name,
                    command,
                    &debug_stats,
                    &mut scroll_offset_lines,
                ) {
                    send_terminal_status_update(
                        &update_mailbox,
                        &debug_stats,
                        TerminalRuntimeState::failed(error),
                        previous_surface.clone(),
                        &notifier,
                    );
                    return;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                send_terminal_status_update(
                    &update_mailbox,
                    &debug_stats,
                    TerminalRuntimeState::disconnected("tmux viewer input channel disconnected"),
                    previous_surface.clone(),
                    &notifier,
                );
                return;
            }
        }

        while let Ok(command) = input_rx.try_recv() {
            if let Err(error) = apply_tmux_viewer_command(
                session_name,
                command,
                &debug_stats,
                &mut scroll_offset_lines,
            ) {
                send_terminal_status_update(
                    &update_mailbox,
                    &debug_stats,
                    TerminalRuntimeState::failed(error),
                    previous_surface.clone(),
                    &notifier,
                );
                return;
            }
        }

        if !tmux_session_exists(session_name) {
            send_terminal_status_update(
                &update_mailbox,
                &debug_stats,
                TerminalRuntimeState::disconnected(format!(
                    "tmux session `{session_name}` no longer exists"
                )),
                previous_surface.clone(),
                &notifier,
            );
            return;
        }

        match capture_tmux_surface(session_name, scroll_offset_lines) {
            Ok((surface, max_scroll_lines)) => {
                scroll_offset_lines = scroll_offset_lines.min(max_scroll_lines);
                send_terminal_frame_update(
                    &update_mailbox,
                    &debug_stats,
                    &notifier,
                    previous_surface.as_ref(),
                    surface.clone(),
                    TMUX_VIEWER_BACKEND_STATUS,
                );
                previous_surface = Some(surface);
            }
            Err(error) => {
                send_terminal_status_update(
                    &update_mailbox,
                    &debug_stats,
                    TerminalRuntimeState::failed(error),
                    previous_surface.clone(),
                    &notifier,
                );
                return;
            }
        }
    }
}

fn apply_tmux_viewer_command(
    session_name: &str,
    command: TerminalCommand,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    scroll_offset_lines: &mut usize,
) -> Result<(), String> {
    match command {
        TerminalCommand::InputText(text) => {
            *scroll_offset_lines = 0;
            send_tmux_bytes(session_name, text.as_bytes(), debug_stats)
        }
        TerminalCommand::InputEvent(event) => {
            *scroll_offset_lines = 0;
            send_tmux_bytes(session_name, event.as_bytes(), debug_stats)
        }
        TerminalCommand::SendCommand(command) => {
            *scroll_offset_lines = 0;
            let payload = format!("{command}\r");
            send_tmux_bytes(session_name, payload.as_bytes(), debug_stats)
        }
        TerminalCommand::ScrollDisplay(lines) => {
            if lines >= 0 {
                *scroll_offset_lines = scroll_offset_lines.saturating_add(lines as usize);
            } else {
                *scroll_offset_lines = scroll_offset_lines.saturating_sub((-lines) as usize);
            }
            Ok(())
        }
    }
}

fn send_tmux_bytes(
    session_name: &str,
    bytes: &[u8],
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
) -> Result<(), String> {
    for args in send_bytes_tmux_commands(session_name, bytes) {
        run_tmux_command(&args)?;
    }
    with_debug_stats(debug_stats, |stats| {
        stats.pty_bytes_written += bytes.len() as u64;
    });
    Ok(())
}

fn capture_tmux_surface(
    session_name: &str,
    scroll_offset_lines: usize,
) -> Result<(TerminalSurface, usize), String> {
    let snapshot = read_tmux_pane_snapshot(session_name)?;
    let max_scroll_lines = snapshot.lines.len().saturating_sub(snapshot.rows);
    let surface = build_surface_from_tmux_snapshot(&snapshot, scroll_offset_lines);
    Ok((surface, max_scroll_lines))
}

fn read_tmux_pane_snapshot(session_name: &str) -> Result<TmuxPaneSnapshot, String> {
    let state_output = run_tmux_command(&pane_state_tmux_command(session_name))?;
    let mut state_parts = state_output.trim().split('\t');
    let cols = state_parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid tmux pane width for `{session_name}`"))?;
    let rows = state_parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid tmux pane height for `{session_name}`"))?;
    let cursor_x = state_parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid tmux cursor_x for `{session_name}`"))?;
    let cursor_y = state_parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid tmux cursor_y for `{session_name}`"))?;
    let cursor_visible = state_parts
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .is_some_and(|flag| flag != 0);

    let capture_output = run_tmux_command(&capture_pane_tmux_command(
        session_name,
        TMUX_VIEWER_HISTORY_LIMIT,
    ))?;
    let mut lines = capture_output
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect::<Vec<_>>();
    if capture_output.ends_with('\n') {
        let _ = lines.pop();
    }

    Ok(TmuxPaneSnapshot {
        cols: cols.max(1),
        rows: rows.max(1),
        cursor_x,
        cursor_y,
        cursor_visible,
        lines,
    })
}

fn build_surface_from_tmux_snapshot(
    snapshot: &TmuxPaneSnapshot,
    scroll_offset_lines: usize,
) -> TerminalSurface {
    let dimensions = TerminalDimensions {
        cols: snapshot.cols.max(1),
        rows: snapshot.rows.max(1),
    };
    let config = TermConfig {
        scrolling_history: TMUX_VIEWER_HISTORY_LIMIT,
        ..TermConfig::default()
    };
    let mut terminal = Term::new(config, &dimensions, VoidListener);
    let mut parser = ansi::Processor::<ansi::StdSyncHandler>::new();
    parser.advance(&mut terminal, b"\x1b[2J\x1b[H");

    let max_scroll_lines = snapshot.lines.len().saturating_sub(snapshot.rows);
    let start = max_scroll_lines.saturating_sub(scroll_offset_lines.min(max_scroll_lines));
    for (row_index, line) in snapshot
        .lines
        .iter()
        .skip(start)
        .take(snapshot.rows)
        .enumerate()
    {
        let move_sequence = format!("\x1b[{};1H\x1b[0m", row_index + 1);
        parser.advance(&mut terminal, move_sequence.as_bytes());
        parser.advance(&mut terminal, line.as_bytes());
        parser.advance(&mut terminal, b"\x1b[K");
    }

    let mut surface = build_surface(&terminal);
    if let Some(cursor) = surface.cursor.as_mut() {
        cursor.x = snapshot.cursor_x.min(snapshot.cols.saturating_sub(1));
        cursor.y = snapshot.cursor_y.min(snapshot.rows.saturating_sub(1));
        cursor.visible = snapshot.cursor_visible && scroll_offset_lines == 0;
    }
    surface
}

fn tmux_session_exists(session_name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_tmux_command(args: &[std::ffi::OsString]) -> Result<String, String> {
    let output = Command::new("tmux").args(args).output().map_err(|error| {
        format!(
            "failed to execute tmux {}: {error}",
            args.iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        )
    })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
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

pub(crate) fn compute_terminal_damage(
    previous_surface: Option<&TerminalSurface>,
    surface: &TerminalSurface,
) -> TerminalDamage {
    let Some(previous_surface) = previous_surface else {
        return TerminalDamage::Full;
    };
    if previous_surface.cols != surface.cols || previous_surface.rows != surface.rows {
        return TerminalDamage::Full;
    }

    let mut dirty_rows = Vec::new();
    for y in 0..surface.rows {
        let start = y * surface.cols;
        let end = start + surface.cols;
        if previous_surface.cells[start..end] != surface.cells[start..end] {
            dirty_rows.push(y);
        }
    }

    if previous_surface.cursor != surface.cursor {
        if let Some(cursor) = previous_surface.cursor.as_ref() {
            if cursor.visible && cursor.y < surface.rows && !dirty_rows.contains(&cursor.y) {
                dirty_rows.push(cursor.y);
            }
        }
        if let Some(cursor) = surface.cursor.as_ref() {
            if cursor.visible && cursor.y < surface.rows && !dirty_rows.contains(&cursor.y) {
                dirty_rows.push(cursor.y);
            }
        }
    }

    if dirty_rows.len() >= surface.rows {
        TerminalDamage::Full
    } else {
        dirty_rows.sort_unstable();
        TerminalDamage::Rows(dirty_rows)
    }
}

pub(crate) fn build_surface(term: &Term<VoidListener>) -> TerminalSurface {
    let content = term.renderable_content();
    let cols = term.columns();
    let rows = term.screen_lines();
    let mut surface = TerminalSurface::new(cols, rows);

    for indexed in content.display_iter {
        let x = indexed.point.column.0;
        let y_i32 = indexed.point.line.0;
        if y_i32 < 0 {
            continue;
        }
        let y = y_i32 as usize;
        if x >= cols || y >= rows {
            continue;
        }

        let mut fg = resolve_alacritty_color(indexed.cell.fg, content.colors, true);
        let mut bg = resolve_alacritty_color(indexed.cell.bg, content.colors, false);
        if indexed.cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }

        let content = if indexed.cell.flags.contains(Flags::HIDDEN)
            || indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            || indexed.cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            TerminalCellContent::Empty
        } else {
            TerminalCellContent::from_parts(indexed.cell.c, indexed.cell.zerowidth())
        };

        let width = if indexed.cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else if indexed
            .cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            0
        } else {
            1
        };

        surface.set_cell(
            x,
            y,
            TerminalCell {
                content,
                fg,
                bg,
                width,
            },
        );
    }

    surface.cursor = Some(TerminalCursor {
        x: content.cursor.point.column.0.min(cols.saturating_sub(1)),
        y: content.cursor.point.line.0.max(0) as usize,
        shape: map_cursor_shape(content.cursor.shape),
        visible: content.cursor.shape != CursorShape::Hidden,
        color: resolve_alacritty_color(AnsiColor::Named(NamedColor::Cursor), content.colors, true),
    });
    surface
}

fn map_cursor_shape(shape: CursorShape) -> TerminalCursorShape {
    match shape {
        CursorShape::Underline => TerminalCursorShape::Underline,
        CursorShape::Beam => TerminalCursorShape::Beam,
        CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => {
            TerminalCursorShape::Block
        }
    }
}

pub(crate) fn resolve_alacritty_color(
    color: AnsiColor,
    colors: &Colors,
    is_foreground: bool,
) -> egui::Color32 {
    let rgb = match color {
        AnsiColor::Spec(rgb) => rgb,
        AnsiColor::Indexed(index) => xterm_indexed_rgb(index),
        AnsiColor::Named(named) => match colors[named] {
            Some(rgb) => rgb,
            None => fallback_named_rgb(named, is_foreground),
        },
    };
    egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

fn fallback_named_rgb(named: NamedColor, is_foreground: bool) -> Rgb {
    match named {
        NamedColor::Black => Rgb { r: 0, g: 0, b: 0 },
        NamedColor::Red => Rgb {
            r: 204,
            g: 85,
            b: 85,
        },
        NamedColor::Green => Rgb {
            r: 85,
            g: 204,
            b: 85,
        },
        NamedColor::Yellow => Rgb {
            r: 205,
            g: 205,
            b: 85,
        },
        NamedColor::Blue => Rgb {
            r: 84,
            g: 85,
            b: 203,
        },
        NamedColor::Magenta => Rgb {
            r: 204,
            g: 85,
            b: 204,
        },
        NamedColor::Cyan => Rgb {
            r: 122,
            g: 202,
            b: 202,
        },
        NamedColor::White => Rgb {
            r: 204,
            g: 204,
            b: 204,
        },
        NamedColor::BrightBlack => Rgb {
            r: 85,
            g: 85,
            b: 85,
        },
        NamedColor::BrightRed => Rgb {
            r: 255,
            g: 85,
            b: 85,
        },
        NamedColor::BrightGreen => Rgb {
            r: 85,
            g: 255,
            b: 85,
        },
        NamedColor::BrightYellow => Rgb {
            r: 255,
            g: 255,
            b: 85,
        },
        NamedColor::BrightBlue => Rgb {
            r: 85,
            g: 85,
            b: 255,
        },
        NamedColor::BrightMagenta => Rgb {
            r: 255,
            g: 85,
            b: 255,
        },
        NamedColor::BrightCyan => Rgb {
            r: 85,
            g: 255,
            b: 255,
        },
        NamedColor::BrightWhite => Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        NamedColor::Foreground | NamedColor::BrightForeground => Rgb {
            r: 190,
            g: 190,
            b: 190,
        },
        NamedColor::Background => Rgb {
            r: 10,
            g: 10,
            b: 10,
        },
        NamedColor::Cursor => Rgb {
            r: 82,
            g: 173,
            b: 112,
        },
        NamedColor::DimBlack => Rgb {
            r: 40,
            g: 40,
            b: 40,
        },
        NamedColor::DimRed => Rgb {
            r: 120,
            g: 50,
            b: 50,
        },
        NamedColor::DimGreen => Rgb {
            r: 50,
            g: 120,
            b: 50,
        },
        NamedColor::DimYellow => Rgb {
            r: 120,
            g: 120,
            b: 50,
        },
        NamedColor::DimBlue => Rgb {
            r: 50,
            g: 50,
            b: 120,
        },
        NamedColor::DimMagenta => Rgb {
            r: 120,
            g: 50,
            b: 120,
        },
        NamedColor::DimCyan => Rgb {
            r: 50,
            g: 120,
            b: 120,
        },
        NamedColor::DimWhite | NamedColor::DimForeground => {
            if is_foreground {
                Rgb {
                    r: 120,
                    g: 120,
                    b: 120,
                }
            } else {
                Rgb {
                    r: 10,
                    g: 10,
                    b: 10,
                }
            }
        }
    }
}

pub(crate) fn xterm_indexed_rgb(index: u8) -> Rgb {
    const ANSI: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcc, 0x55, 0x55),
        (0x55, 0xcc, 0x55),
        (0xcd, 0xcd, 0x55),
        (0x54, 0x55, 0xcb),
        (0xcc, 0x55, 0xcc),
        (0x7a, 0xca, 0xca),
        (0xcc, 0xcc, 0xcc),
        (0x55, 0x55, 0x55),
        (0xff, 0x55, 0x55),
        (0x55, 0xff, 0x55),
        (0xff, 0xff, 0x55),
        (0x55, 0x55, 0xff),
        (0xff, 0x55, 0xff),
        (0x55, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];

    if index < 16 {
        let (r, g, b) = ANSI[index as usize];
        return Rgb { r, g, b };
    }

    if index < 232 {
        const RAMP6: [u8; 6] = [0, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
        let idx = index - 16;
        let blue = RAMP6[(idx % 6) as usize];
        let green = RAMP6[((idx / 6) % 6) as usize];
        let red = RAMP6[((idx / 36) % 6) as usize];
        return Rgb {
            r: red,
            g: green,
            b: blue,
        };
    }

    let grey = 0x08 + (index - 232) * 10;
    Rgb {
        r: grey,
        g: grey,
        b: grey,
    }
}

fn spawn_pty(cols: u16, rows: u16, target: &TerminalAttachTarget) -> Result<PtySession, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("openpty failed: {error}"))?;

    let mut command = build_attach_command(target);
    command.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("spawn_command failed: {error}"))?;

    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("take_writer failed: {error}"))?;

    Ok(PtySession {
        master: pair.master,
        writer,
        child,
    })
}

fn build_attach_command(target: &TerminalAttachTarget) -> CommandBuilder {
    let (program, args) = build_attach_command_argv(target);
    let mut command = CommandBuilder::new(program);
    for arg in args {
        command.arg(arg);
    }
    command
}

fn write_input(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}
