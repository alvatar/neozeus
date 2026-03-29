use crate::app::{mark_app_state_dirty, AppStatePersistenceState};

use super::{
    bridge::TerminalBridge,
    debug::append_debug_log,
    registry::{TerminalFocusState, TerminalId, TerminalManager},
    runtime::TerminalRuntimeSpawner,
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

/// Removes a terminal from authoritative state while leaving projection cleanup to presentation sync.
///
/// Terminal lifecycle mutates only the terminal/focus stores. The later projection sync pass observes
/// the missing terminal id and despawns the now-stale panel/frame entities plus presentation-store
/// entry.
fn remove_terminal_with_projection(
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
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
/// The helper performs only runtime-facing work plus terminal/focus state removal. It deliberately
/// does not choose replacement focus or mutate visibility/view policy; that policy belongs to the
/// app-layer use case that called it.
pub(crate) fn kill_active_terminal_session_and_remove(
    time: &Time,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    app_state_persistence: &mut AppStatePersistenceState,
) -> Result<Option<(TerminalId, String)>, String> {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let Some(active_id) = focus_state.active_id() else {
        return Ok(None);
    };
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

    remove_terminal_with_projection(terminal_manager, focus_state, active_id);
    #[cfg(test)]
    terminal_manager.replace_test_focus_state(focus_state);
    mark_app_state_dirty(app_state_persistence, Some(time));
    Ok(Some((active_id, session_name)))
}
