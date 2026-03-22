use crate::terminals::tmux::TmuxPaneState;
use crate::terminals::{
    backend::{send_terminal_frame_update, send_terminal_status_update},
    build_surface, resolve_tmux_active_pane_target, send_command_payload_bytes, with_debug_stats,
    RuntimeNotifier, TerminalCommand, TerminalDebugStats, TerminalDimensions, TerminalRuntimeState,
    TerminalSurface, TerminalUpdateMailbox, TmuxPaneClient,
};
use alacritty_terminal::{event::VoidListener, term::Config as TermConfig, term::Term, vte::ansi};
use std::sync::{mpsc, mpsc::Receiver, Arc, Mutex};
use std::time::Duration;

const TMUX_VIEWER_POLL_INTERVAL: Duration = Duration::from_millis(33);
const TMUX_VIEWER_HISTORY_LIMIT: usize = 4096;
const TMUX_VIEWER_BACKEND_STATUS: &str = "backend: tmux detached session viewer";

struct TmuxPaneSnapshot {
    state: TmuxPaneState,
    lines: Vec<String>,
}

pub(crate) fn run_tmux_viewer_worker(
    input_rx: Receiver<TerminalCommand>,
    update_mailbox: Arc<TerminalUpdateMailbox>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
    notifier: RuntimeNotifier,
    session_name: &str,
    tmux_client: Arc<dyn TmuxPaneClient>,
) {
    crate::terminals::append_debug_log(format!("tmux viewer worker start session={session_name}"));
    let mut previous_surface: Option<TerminalSurface> = None;
    let mut scroll_offset_lines = 0usize;

    loop {
        match input_rx.recv_timeout(TMUX_VIEWER_POLL_INTERVAL) {
            Ok(command) => {
                if let Err(error) = apply_tmux_viewer_command(
                    tmux_client.as_ref(),
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
                tmux_client.as_ref(),
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

        match capture_tmux_surface(tmux_client.as_ref(), session_name, scroll_offset_lines) {
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
    client: &dyn TmuxPaneClient,
    session_name: &str,
    command: TerminalCommand,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    scroll_offset_lines: &mut usize,
) -> Result<(), String> {
    match command {
        TerminalCommand::InputText(text) => {
            *scroll_offset_lines = 0;
            send_tmux_bytes(client, session_name, text.as_bytes(), debug_stats)
        }
        TerminalCommand::InputEvent(event) => {
            *scroll_offset_lines = 0;
            send_tmux_bytes(client, session_name, event.as_bytes(), debug_stats)
        }
        TerminalCommand::SendCommand(command) => {
            *scroll_offset_lines = 0;
            let payload = send_command_payload_bytes(&command);
            send_tmux_bytes(client, session_name, &payload, debug_stats)
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
    client: &dyn TmuxPaneClient,
    session_name: &str,
    bytes: &[u8],
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
) -> Result<(), String> {
    let pane_target = resolve_tmux_active_pane_target(client, session_name)?;
    client.send_bytes(&pane_target, bytes)?;
    with_debug_stats(debug_stats, |stats| {
        stats.pty_bytes_written += bytes.len() as u64;
    });
    Ok(())
}

fn capture_tmux_surface(
    client: &dyn TmuxPaneClient,
    session_name: &str,
    scroll_offset_lines: usize,
) -> Result<(TerminalSurface, usize), String> {
    let snapshot = read_tmux_pane_snapshot(client, session_name)?;
    let max_scroll_lines = snapshot.lines.len().saturating_sub(snapshot.state.rows);
    let surface = build_surface_from_tmux_snapshot(&snapshot, scroll_offset_lines);
    Ok((surface, max_scroll_lines))
}

fn read_tmux_pane_snapshot(
    client: &dyn TmuxPaneClient,
    session_name: &str,
) -> Result<TmuxPaneSnapshot, String> {
    let pane_target = resolve_tmux_active_pane_target(client, session_name)?;
    let state = client.pane_state(&pane_target)?;
    let capture_output = client.capture_pane(&pane_target, TMUX_VIEWER_HISTORY_LIMIT)?;
    let mut lines = capture_output
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect::<Vec<_>>();
    if capture_output.ends_with('\n') {
        let _ = lines.pop();
    }

    Ok(TmuxPaneSnapshot { state, lines })
}

fn build_surface_from_tmux_snapshot(
    snapshot: &TmuxPaneSnapshot,
    scroll_offset_lines: usize,
) -> TerminalSurface {
    let dimensions = TerminalDimensions {
        cols: snapshot.state.cols.max(1),
        rows: snapshot.state.rows.max(1),
    };
    let config = TermConfig {
        scrolling_history: TMUX_VIEWER_HISTORY_LIMIT,
        ..TermConfig::default()
    };
    let mut terminal = Term::new(config, &dimensions, VoidListener);
    let mut parser = ansi::Processor::<ansi::StdSyncHandler>::new();
    parser.advance(&mut terminal, b"\x1b[2J\x1b[H");

    let max_scroll_lines = snapshot.lines.len().saturating_sub(snapshot.state.rows);
    let start = max_scroll_lines.saturating_sub(scroll_offset_lines.min(max_scroll_lines));
    for (row_index, line) in snapshot
        .lines
        .iter()
        .skip(start)
        .take(snapshot.state.rows)
        .enumerate()
    {
        let move_sequence = format!("\x1b[{};1H\x1b[0m", row_index + 1);
        parser.advance(&mut terminal, move_sequence.as_bytes());
        parser.advance(&mut terminal, line.as_bytes());
        parser.advance(&mut terminal, b"\x1b[K");
    }

    let mut surface = build_surface(&terminal);
    if let Some(cursor) = surface.cursor.as_mut() {
        cursor.x = snapshot
            .state
            .cursor_x
            .min(snapshot.state.cols.saturating_sub(1));
        cursor.y = snapshot
            .state
            .cursor_y
            .min(snapshot.state.rows.saturating_sub(1));
        cursor.visible = snapshot.state.cursor_visible && scroll_offset_lines == 0;
    }
    surface
}
