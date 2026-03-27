use crate::{
    hud::{TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        append_debug_log, mark_terminal_sessions_dirty, TerminalBridge, TerminalFocusState,
        TerminalId, TerminalManager, TerminalPresentationStore, TerminalRuntimeSpawner,
        TerminalSessionPersistenceState, TerminalViewState,
    },
};
use bevy::prelude::*;

#[allow(
    clippy::too_many_arguments,
    reason = "terminal attach joins daemon bridge creation, domain state, projection spawn, and presentation assets"
)]
/// Attaches an existing daemon session into local terminal state and spawns its presentation entities.
///
/// The function asks the runtime spawner for an attached bridge, creates the terminal in the manager
/// without implicitly changing creation order semantics, optionally focuses it, and then creates the
/// panel/frame presentation projection for the returned slot.
pub(crate) fn attach_terminal_session(
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    session_name: String,
    focus: bool,
) -> Result<(TerminalId, TerminalBridge), String> {
    let bridge = runtime_spawner.spawn_attached(&session_name)?;
    let (terminal_id, _slot) = terminal_manager
        .create_terminal_without_focus_with_slot_and_session(bridge.clone(), session_name);
    if focus {
        focus_state.focus_terminal(terminal_manager, terminal_id);
    }
    #[cfg(test)]
    terminal_manager.replace_test_focus_state(focus_state);
    Ok((terminal_id, bridge))
}

/// Chooses the terminal that should become active after a given terminal is removed.
///
/// The policy is creation-order based rather than focus-order based: prefer the previous terminal if
/// one exists, otherwise fall back to the next terminal. That matches the mental model that terminal
/// slots are laid out in creation order and keeps deletion behavior predictable.
fn adjacent_terminal_in_creation_order(
    terminal_manager: &TerminalManager,
    terminal_id: TerminalId,
) -> Option<TerminalId> {
    let terminal_ids = terminal_manager.terminal_ids();
    let index = terminal_ids.iter().position(|id| *id == terminal_id)?;
    if index > 0 {
        terminal_ids.get(index - 1).copied()
    } else {
        terminal_ids.get(index + 1).copied()
    }
}

/// Removes a terminal from authoritative state while leaving projection cleanup to presentation sync.
///
/// Terminal lifecycle mutates only the terminal/focus stores. The later projection sync pass observes
/// the missing terminal id and despawns the now-stale panel/frame entities plus presentation-store
/// entry.
pub(crate) fn remove_terminal_with_projection(
    _commands: &mut Commands,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    _presentation_store: &mut TerminalPresentationStore,
    terminal_id: TerminalId,
) {
    let _ = terminal_manager.remove_terminal(terminal_id);
    focus_state.forget_terminal(terminal_id);
    #[cfg(test)]
    terminal_manager.replace_test_focus_state(focus_state);
}

#[allow(
    clippy::too_many_arguments,
    reason = "kill spans daemon authority, domain state, projection cleanup, and persistence/view updates"
)]
/// Kills the active daemon session when possible and removes the corresponding local terminal.
///
/// The function first snapshots the active terminal's session name and runtime state, then asks the
/// daemon to kill it. If daemon kill fails for an interactive terminal, the error is propagated; if
/// the terminal was already non-interactive, the failure is downgraded to a debug log and local
/// cleanup continues. After removal it selects a replacement focus target by creation order, updates
/// visibility/view state, marks persistence dirty, and returns the removed terminal id + session name.
pub(crate) fn kill_active_terminal_session_and_remove(
    commands: &mut Commands,
    time: &Time,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    presentation_store: &mut TerminalPresentationStore,
    runtime_spawner: &TerminalRuntimeSpawner,
    session_persistence: &mut TerminalSessionPersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
) -> Result<Option<(TerminalId, String)>, String> {
    let Some(active_id) = focus_state.active_id() else {
        return Ok(None);
    };
    let replacement_id = adjacent_terminal_in_creation_order(terminal_manager, active_id);
    let Some((session_name, runtime_state)) = terminal_manager.get(active_id).map(|terminal| {
        (
            terminal.session_name.clone(),
            terminal.snapshot.runtime.clone(),
        )
    }) else {
        return Ok(None);
    };
    if let Err(error) = runtime_spawner.kill_session(&session_name) {
        if runtime_state.is_interactive() {
            return Err(error);
        }
        append_debug_log(format!(
            "best-effort kill failed for non-interactive terminal {}: {error}",
            session_name
        ));
    }

    remove_terminal_with_projection(
        commands,
        terminal_manager,
        focus_state,
        presentation_store,
        active_id,
    );
    view_state.forget_terminal(active_id);
    if let Some(replacement_id) = replacement_id {
        focus_state.focus_terminal(terminal_manager, replacement_id);
        visibility_state.policy = TerminalVisibilityPolicy::Isolate(replacement_id);
        view_state.focus_terminal(Some(replacement_id));
    } else {
        visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
        view_state.focus_terminal(None);
    }
    #[cfg(test)]
    terminal_manager.replace_test_focus_state(focus_state);
    mark_terminal_sessions_dirty(session_persistence, Some(time));
    Ok(Some((active_id, session_name)))
}
