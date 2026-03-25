pub(crate) use crate::hud::commands::{
    apply_hud_module_requests, apply_terminal_focus_requests, apply_terminal_lifecycle_requests,
    apply_terminal_send_requests, apply_terminal_task_requests, apply_terminal_view_requests,
    apply_visibility_requests, dispatch_hud_intents,
};

#[cfg(test)]
use crate::{
    hud::{AgentDirectory, TerminalVisibilityState},
    terminals::{
        kill_active_terminal_session_and_remove, TerminalFocusState, TerminalManager,
        TerminalPresentationStore, TerminalRuntimeSpawner, TerminalSessionPersistenceState,
        TerminalViewState,
    },
};
#[cfg(test)]
use bevy::prelude::*;

#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "test-visible helper intentionally wraps the terminal lifecycle boundary"
)]
pub(crate) fn kill_active_terminal(
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
) -> Result<Option<(crate::terminals::TerminalId, String)>, String> {
    kill_active_terminal_session_and_remove(
        commands,
        time,
        terminal_manager,
        focus_state,
        presentation_store,
        runtime_spawner,
        agent_directory,
        session_persistence,
        visibility_state,
        view_state,
    )
}
