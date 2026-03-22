use crate::terminals::{
    append_debug_log, note_terminal_error, with_debug_stats, RuntimeNotifier, TerminalAttachTarget,
    TerminalCommand, TerminalDebugStats, TerminalFrameUpdate, TerminalRuntimeState,
    TerminalSurface, TerminalUpdate, TerminalUpdateMailbox, TmuxPaneClient,
};
use std::sync::{mpsc::Receiver, Arc, Mutex};

pub(crate) use crate::terminals::ansi_surface::build_surface;
#[cfg(test)]
pub(crate) use crate::terminals::ansi_surface::{resolve_alacritty_color, xterm_indexed_rgb};
pub(crate) use crate::terminals::damage::compute_terminal_damage;

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

pub(crate) fn send_terminal_status_update(
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

pub(crate) fn send_terminal_frame_update(
    update_mailbox: &Arc<TerminalUpdateMailbox>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    notifier: &RuntimeNotifier,
    previous_surface: Option<&TerminalSurface>,
    surface: TerminalSurface,
    backend_status: &str,
) {
    let damage = compute_terminal_damage(previous_surface, &surface);
    if matches!(damage, crate::terminals::TerminalDamage::Rows(ref rows) if rows.is_empty()) {
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

pub(crate) fn send_command_payload_bytes(command: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(command.len() + 1);
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if matches!(chars.peek(), Some('\n')) {
                    let _ = chars.next();
                }
                bytes.push(b'\r');
            }
            '\n' => bytes.push(b'\r'),
            _ => {
                let mut encoded = [0_u8; 4];
                bytes.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            }
        }
    }
    bytes.push(b'\r');
    bytes
}

pub(crate) fn terminal_worker(
    input_rx: Receiver<TerminalCommand>,
    update_mailbox: Arc<TerminalUpdateMailbox>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
    notifier: RuntimeNotifier,
    attach_target: TerminalAttachTarget,
    tmux_client: Option<Arc<dyn TmuxPaneClient>>,
) {
    if let TerminalAttachTarget::TmuxViewer { session_name } = &attach_target {
        let Some(tmux_client) = tmux_client else {
            send_terminal_status_update(
                &update_mailbox,
                &debug_stats,
                TerminalRuntimeState::failed(
                    "tmux viewer backend started without tmux client".to_owned(),
                ),
                None,
                &notifier,
            );
            return;
        };
        crate::terminals::tmux_viewer_backend::run_tmux_viewer_worker(
            input_rx,
            update_mailbox,
            debug_stats,
            notifier,
            session_name,
            tmux_client,
        );
        return;
    }

    crate::terminals::pty_backend::run_pty_worker(
        input_rx,
        update_mailbox,
        debug_stats,
        notifier,
        attach_target,
    );
}
