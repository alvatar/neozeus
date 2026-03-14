use super::*;
use crate::*;

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

fn send_terminal_frame_update(
    update_tx: &Sender<TerminalUpdate>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    event_loop_proxy: &EventLoopProxy<WinitUserEvent>,
    previous_surface: Option<&TerminalSurface>,
    surface: TerminalSurface,
    status: String,
) {
    let damage = compute_terminal_damage(previous_surface, &surface);
    if matches!(damage, TerminalDamage::Rows(ref rows) if rows.is_empty()) {
        return;
    }
    if update_tx
        .send(TerminalUpdate::Frame(TerminalFrameUpdate {
            surface,
            damage,
            status,
        }))
        .is_ok()
    {
        with_debug_stats(debug_stats, |stats| {
            stats.snapshots_sent += 1;
        });
        let _ = event_loop_proxy.send_event(WinitUserEvent::WakeUp);
    }
}

pub(crate) fn terminal_worker(
    input_rx: Receiver<TerminalCommand>,
    update_tx: Sender<TerminalUpdate>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
    event_loop_proxy: EventLoopProxy<WinitUserEvent>,
) {
    let PtySession {
        master,
        writer,
        mut child,
    } = match spawn_pty(DEFAULT_COLS, DEFAULT_ROWS) {
        Ok(session) => session,
        Err(error) => {
            let status = format!("failed to start PTY backend: {error}");
            let _ = update_tx.send(TerminalUpdate::Status {
                surface: None,
                status: status.clone(),
            });
            set_terminal_error(&debug_stats, status);
            return;
        }
    };
    append_debug_log("pty spawned successfully");

    let mut reader = match master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let status = format!("failed to attach PTY reader: {error}");
            let _ = update_tx.send(TerminalUpdate::Status {
                surface: None,
                status: status.clone(),
            });
            set_terminal_error(&debug_stats, status);
            let _ = child.kill();
            return;
        }
    };

    let (pty_output_tx, pty_output_rx) = mpsc::channel::<Vec<u8>>();
    let reader_state = Arc::new(Mutex::new(None::<String>));
    let worker_reader_state = reader_state.clone();
    let reader_thread = thread::spawn(move || {
        append_debug_log("pty reader thread start");
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    match worker_reader_state.lock() {
                        Ok(mut state) => *state = Some("PTY reader reached EOF".into()),
                        Err(poisoned) => {
                            *poisoned.into_inner() = Some("PTY reader reached EOF".into())
                        }
                    }
                    break;
                }
                Ok(read) => {
                    if pty_output_tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    match worker_reader_state.lock() {
                        Ok(mut state) => *state = Some(format!("PTY reader error: {error}")),
                        Err(poisoned) => {
                            *poisoned.into_inner() = Some(format!("PTY reader error: {error}"))
                        }
                    }
                    break;
                }
            }
        }
    });

    enum InputThreadEvent {
        WriteResult(Result<(), String>),
        ScrollDisplay(i32),
        Shutdown,
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
                TerminalCommand::Shutdown => {
                    let _ = input_status_tx.send(InputThreadEvent::Shutdown);
                    break;
                }
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
                                &update_tx,
                                &debug_stats,
                                &terminal,
                                &event_loop_proxy,
                                "PTY reader channel disconnected",
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
                    &update_tx,
                    &debug_stats,
                    &terminal,
                    &event_loop_proxy,
                    "PTY reader channel disconnected",
                );
                running = false;
            }
        }

        while let Ok(event) = input_status_rx.try_recv() {
            match event {
                InputThreadEvent::WriteResult(Ok(())) => {}
                InputThreadEvent::WriteResult(Err(status)) => {
                    send_terminal_status_update(
                        &update_tx,
                        &debug_stats,
                        &terminal,
                        &event_loop_proxy,
                        status,
                    );
                    running = false;
                }
                InputThreadEvent::ScrollDisplay(lines) => {
                    append_debug_log(format!("terminal scroll display: {lines}"));
                    terminal.scroll_display(Scroll::Delta(lines));
                    let surface = build_surface(&terminal);
                    send_terminal_frame_update(
                        &update_tx,
                        &debug_stats,
                        &event_loop_proxy,
                        previous_surface.as_ref(),
                        surface.clone(),
                        "backend: alacritty_terminal + portable-pty".into(),
                    );
                    previous_surface = Some(surface);
                }
                InputThreadEvent::Shutdown => {
                    running = false;
                }
            }
        }

        let reader_status = match reader_state.lock() {
            Ok(state) => state.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(status) = reader_status {
            send_terminal_status_update(
                &update_tx,
                &debug_stats,
                &terminal,
                &event_loop_proxy,
                status,
            );
            running = false;
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                send_terminal_status_update(
                    &update_tx,
                    &debug_stats,
                    &terminal,
                    &event_loop_proxy,
                    format!(
                        "PTY child exited: code={} signal={:?}",
                        status.exit_code(),
                        status.signal()
                    ),
                );
                running = false;
            }
            Ok(None) => {}
            Err(error) => {
                send_terminal_status_update(
                    &update_tx,
                    &debug_stats,
                    &terminal,
                    &event_loop_proxy,
                    format!("PTY child wait failed: {error}"),
                );
                running = false;
            }
        }

        if received_output && running {
            let surface = build_surface(&terminal);
            send_terminal_frame_update(
                &update_tx,
                &debug_stats,
                &event_loop_proxy,
                previous_surface.as_ref(),
                surface.clone(),
                "backend: alacritty_terminal + portable-pty".into(),
            );
            previous_surface = Some(surface);
        }
    }

    let _ = child.kill();
    let _ = reader_thread.join();
    let _ = input_thread.join();
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

fn spawn_pty(cols: u16, rows: u16) -> Result<PtySession, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("openpty failed: {error}"))?;

    let shell = shell_path();
    let mut command = CommandBuilder::new(shell);
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

fn shell_path() -> OsString {
    match env::var_os("SHELL") {
        Some(shell) => shell,
        None => OsString::from("bash"),
    }
}

fn write_input(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}
