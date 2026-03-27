use crate::{
    hud::HudModalState,
    terminals::{clear_done_tasks, mark_terminal_notes_dirty, TerminalManager, TerminalNotesState},
};
use bevy::{prelude::*, window::RequestRedraw};

/// Applies task-note mutations and task-consumption actions for a terminal.
///
/// Each request variant is resolved through the terminal's session name, because notes are persisted
/// per session rather than per transient terminal id. After a successful change, the system marks the
/// notes state dirty, refreshes the open task dialog if it is showing that same terminal, and asks
/// for a redraw so the HUD reflects the updated task text immediately.
pub(crate) fn apply_terminal_task_requests(
    mut requests: MessageReader<crate::hud::TerminalTaskRequest>,
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    mut notes_state: ResMut<TerminalNotesState>,
    mut modal_state: ResMut<HudModalState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        let mut changed_terminal = None;
        let changed = match request {
            crate::hud::TerminalTaskRequest::SetText { terminal_id, text } => {
                terminal_manager.get(*terminal_id).is_some_and(|terminal| {
                    let changed = notes_state.set_note_text(&terminal.session_name, text);
                    if changed {
                        changed_terminal = Some(*terminal_id);
                    }
                    changed
                })
            }
            crate::hud::TerminalTaskRequest::ClearDone { terminal_id } => {
                terminal_manager.get(*terminal_id).is_some_and(|terminal| {
                    let (updated, removed) = clear_done_tasks(
                        notes_state
                            .note_text(&terminal.session_name)
                            .unwrap_or_default(),
                    );
                    if removed == 0 {
                        return false;
                    }
                    let changed = notes_state.set_note_text(&terminal.session_name, &updated);
                    if changed {
                        changed_terminal = Some(*terminal_id);
                    }
                    changed
                })
            }
            crate::hud::TerminalTaskRequest::Append { terminal_id, text } => {
                terminal_manager.get(*terminal_id).is_some_and(|terminal| {
                    let changed = notes_state.append_task_from_text(&terminal.session_name, text);
                    if changed {
                        changed_terminal = Some(*terminal_id);
                    }
                    changed
                })
            }
            crate::hud::TerminalTaskRequest::Prepend { terminal_id, text } => {
                terminal_manager.get(*terminal_id).is_some_and(|terminal| {
                    let changed = notes_state.prepend_task_from_text(&terminal.session_name, text);
                    if changed {
                        changed_terminal = Some(*terminal_id);
                    }
                    changed
                })
            }
            crate::hud::TerminalTaskRequest::ConsumeNext { terminal_id } => {
                terminal_manager.get(*terminal_id).is_some_and(|terminal| {
                    let Some(task_text) = notes_state.note_text(&terminal.session_name) else {
                        return false;
                    };
                    let Some((message, updated_task_text)) =
                        crate::terminals::extract_next_task(task_text)
                    else {
                        return false;
                    };
                    if message.trim().is_empty() {
                        return false;
                    }
                    terminal
                        .bridge
                        .send(crate::terminals::TerminalCommand::SendCommand(message));
                    let changed =
                        notes_state.set_note_text(&terminal.session_name, &updated_task_text);
                    if changed {
                        changed_terminal = Some(*terminal_id);
                    }
                    changed
                })
            }
        };
        if let Some(terminal_id) = changed_terminal {
            if modal_state.task_dialog.visible
                && modal_state.task_dialog.target_terminal == Some(terminal_id)
            {
                let task_text = terminal_manager
                    .get(terminal_id)
                    .and_then(|terminal| notes_state.note_text(&terminal.session_name))
                    .unwrap_or_default()
                    .to_owned();
                modal_state.task_dialog.load_text(&task_text);
            }
        }
        if changed {
            mark_terminal_notes_dirty(&mut notes_state, Some(&time));
            redraws.write(RequestRedraw);
        }
    }
}
