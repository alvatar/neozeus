use crate::{
    hud::{AgentDirectory, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        mark_terminal_sessions_dirty, spawn_terminal_presentation, TerminalBridge, TerminalId,
        TerminalManager, TerminalPresentationStore, TerminalRuntimeSpawner, TerminalSessionClient,
        TerminalSessionPersistenceState, TerminalViewState, TmuxClientResource,
    },
};
use bevy::prelude::*;

#[allow(
    clippy::too_many_arguments,
    reason = "terminal spawn joins runtime, domain, projection, tmux pane client, and presentation assets"
)]
pub(crate) fn spawn_attached_terminal_with_presentation(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    terminal_manager: &mut TerminalManager,
    presentation_store: &mut TerminalPresentationStore,
    runtime_spawner: &TerminalRuntimeSpawner,
    tmux_client: &TmuxClientResource,
    session_name: String,
    focus: bool,
) -> (TerminalId, TerminalBridge) {
    let bridge = runtime_spawner.spawn_attached(
        crate::terminals::TerminalAttachTarget::TmuxViewer {
            session_name: session_name.clone(),
        },
        Some(tmux_client.shared_pane_client()),
    );
    let (terminal_id, slot) = if focus {
        terminal_manager.create_terminal_with_slot_and_session(bridge.clone(), session_name)
    } else {
        terminal_manager
            .create_terminal_without_focus_with_slot_and_session(bridge.clone(), session_name)
    };
    spawn_terminal_presentation(commands, images, presentation_store, terminal_id, slot);
    (terminal_id, bridge)
}

pub(crate) fn remove_terminal_with_projection(
    commands: &mut Commands,
    terminal_manager: &mut TerminalManager,
    presentation_store: &mut TerminalPresentationStore,
    terminal_id: TerminalId,
) {
    let presented_terminal = presentation_store.remove(terminal_id);
    let _ = terminal_manager.remove_terminal(terminal_id);
    if let Some(presented_terminal) = presented_terminal {
        commands.entity(presented_terminal.panel_entity).despawn();
        commands.entity(presented_terminal.frame_entity).despawn();
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "kill spans session authority, domain state, projection cleanup, and persistence/view updates"
)]
pub(crate) fn kill_active_terminal_session_and_remove(
    commands: &mut Commands,
    time: &Time,
    terminal_manager: &mut TerminalManager,
    presentation_store: &mut TerminalPresentationStore,
    session_client: &dyn TerminalSessionClient,
    agent_directory: &mut AgentDirectory,
    session_persistence: &mut TerminalSessionPersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
) -> Result<Option<(TerminalId, String)>, String> {
    let Some(active_id) = terminal_manager.active_id() else {
        return Ok(None);
    };
    let Some(session_name) = terminal_manager
        .get(active_id)
        .map(|terminal| terminal.session_name.clone())
    else {
        return Ok(None);
    };
    session_client.kill_session(&session_name)?;

    remove_terminal_with_projection(commands, terminal_manager, presentation_store, active_id);
    agent_directory.labels.remove(&active_id);
    visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
    view_state.forget_terminal(active_id);
    view_state.focus_terminal(None);
    mark_terminal_sessions_dirty(session_persistence, Some(time));
    Ok(Some((active_id, session_name)))
}
