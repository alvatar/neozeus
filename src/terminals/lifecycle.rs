use crate::{
    hud::{AgentDirectory, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        append_debug_log, mark_terminal_sessions_dirty, presentation::spawn_terminal_presentation,
        TerminalBridge, TerminalFocusState, TerminalId, TerminalManager, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalSessionPersistenceState, TerminalViewState,
    },
};
use bevy::prelude::*;

#[allow(
    clippy::too_many_arguments,
    reason = "terminal attach joins daemon bridge creation, domain state, projection spawn, and presentation assets"
)]
// Spawns attached terminal with presentation.
pub(crate) fn spawn_attached_terminal_with_presentation(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    presentation_store: &mut TerminalPresentationStore,
    runtime_spawner: &TerminalRuntimeSpawner,
    session_name: String,
    focus: bool,
) -> Result<(TerminalId, TerminalBridge), String> {
    let bridge = runtime_spawner.spawn_attached(&session_name)?;
    let (terminal_id, slot) = terminal_manager
        .create_terminal_without_focus_with_slot_and_session(bridge.clone(), session_name);
    if focus {
        focus_state.focus_terminal(terminal_manager, terminal_id);
    }
    #[cfg(test)]
    terminal_manager.replace_test_focus_state(focus_state);
    spawn_terminal_presentation(commands, images, presentation_store, terminal_id, slot);
    Ok((terminal_id, bridge))
}

// Implements adjacent terminal in creation order.
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

// Removes terminal with projection.
pub(crate) fn remove_terminal_with_projection(
    commands: &mut Commands,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    presentation_store: &mut TerminalPresentationStore,
    terminal_id: TerminalId,
) {
    let presented_terminal = presentation_store.remove(terminal_id);
    let _ = terminal_manager.remove_terminal(terminal_id);
    focus_state.forget_terminal(terminal_id);
    #[cfg(test)]
    terminal_manager.replace_test_focus_state(focus_state);
    if let Some(presented_terminal) = presented_terminal {
        commands.entity(presented_terminal.panel_entity).despawn();
        commands.entity(presented_terminal.frame_entity).despawn();
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "kill spans daemon authority, domain state, projection cleanup, and persistence/view updates"
)]
// Kills active terminal session and remove.
pub(crate) fn kill_active_terminal_session_and_remove(
    commands: &mut Commands,
    time: &Time,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    presentation_store: &mut TerminalPresentationStore,
    runtime_spawner: &TerminalRuntimeSpawner,
    agent_directory: &mut AgentDirectory,
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
    agent_directory.labels.remove(&active_id);
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
