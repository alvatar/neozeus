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

/// Returns the daemon's freshest known runtime for one session after a kill attempt.
///
/// `Ok(None)` means the daemon no longer lists the session at all, which is treated as successful
/// teardown for the local cleanup decision. Errors are returned to the caller so it can fall back to
/// the conservative local-snapshot behavior instead of guessing.
fn daemon_runtime_after_kill_attempt(
    runtime_spawner: &TerminalRuntimeSpawner,
    session_name: &str,
) -> Result<Option<crate::terminals::TerminalRuntimeState>, String> {
    Ok(runtime_spawner
        .list_session_infos()?
        .into_iter()
        .find(|session| session.session_id == session_name)
        .map(|session| session.runtime))
}

#[allow(
    clippy::too_many_arguments,
    reason = "kill spans daemon authority, domain state, projection cleanup, and persistence/view updates"
)]
fn kill_terminal_session_and_remove_by_id(
    time: &Time,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    app_state_persistence: &mut AppStatePersistenceState,
    terminal_id: TerminalId,
    session_name: &str,
) -> Result<Option<(TerminalId, String)>, String> {
    let Some(runtime_state) = terminal_manager
        .get(terminal_id)
        .map(|terminal| terminal.snapshot.runtime.clone())
    else {
        return Ok(None);
    };
    if let Err(error) = runtime_spawner.kill_session(session_name) {
        if runtime_state.is_interactive() {
            let daemon_session_stopped = match daemon_runtime_after_kill_attempt(
                runtime_spawner,
                session_name,
            ) {
                Ok(daemon_runtime) => daemon_runtime
                    .as_ref()
                    .is_none_or(|runtime| !runtime.is_interactive()),
                Err(query_error) => {
                    append_debug_log(format!(
                        "failed to verify daemon runtime for {session_name} after kill error: {query_error}"
                    ));
                    false
                }
            };
            if !daemon_session_stopped {
                return Err(error);
            }
        }
        append_debug_log(format!(
            "best-effort kill failed after terminal already stopped {session_name}: {error}"
        ));
    }

    remove_terminal_with_projection(terminal_manager, focus_state, terminal_id);
    mark_app_state_dirty(app_state_persistence, Some(time));
    Ok(Some((terminal_id, session_name.to_owned())))
}

#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "kill spans daemon authority, domain state, projection cleanup, and persistence/view updates"
)]
/// Test-only helper that kills whichever terminal is currently focused.
pub(crate) fn kill_active_terminal_session_and_remove(
    time: &Time,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    app_state_persistence: &mut AppStatePersistenceState,
) -> Result<Option<(TerminalId, String)>, String> {
    let Some(active_id) = focus_state.active_id() else {
        return Ok(None);
    };
    let Some(session_name) = terminal_manager
        .get(active_id)
        .map(|terminal| terminal.session_name.clone())
    else {
        return Ok(None);
    };
    kill_terminal_session_and_remove_by_id(
        time,
        terminal_manager,
        focus_state,
        runtime_spawner,
        app_state_persistence,
        active_id,
        &session_name,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "kill spans daemon authority, domain state, projection cleanup, and persistence/view updates"
)]
pub(crate) fn kill_terminal_session_and_remove(
    time: &Time,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    runtime_spawner: &TerminalRuntimeSpawner,
    app_state_persistence: &mut AppStatePersistenceState,
    terminal_id: TerminalId,
    session_name: &str,
) -> Result<Option<(TerminalId, String)>, String> {
    kill_terminal_session_and_remove_by_id(
        time,
        terminal_manager,
        focus_state,
        runtime_spawner,
        app_state_persistence,
        terminal_id,
        session_name,
    )
}
