use crate::{
    hud::{AgentDirectory, HudState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        append_debug_log, kill_active_terminal_session_and_remove, mark_terminal_sessions_dirty,
        spawn_attached_terminal_with_presentation, TerminalManager, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalSessionPersistenceState, TerminalViewState,
        PERSISTENT_SESSION_PREFIX,
    },
};
use bevy::{prelude::*, window::RequestRedraw};

#[allow(
    clippy::too_many_arguments,
    reason = "terminal spawn spans tmux provisioning, runtime spawn, projection spawn, and persistence"
)]
pub(crate) fn apply_terminal_lifecycle_requests(
    mut requests: MessageReader<crate::hud::TerminalLifecycleRequest>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    mut hud_state: ResMut<HudState>,
    mut agent_directory: ResMut<AgentDirectory>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut view_state: ResMut<TerminalViewState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        let mut state_changed = false;
        match request {
            crate::hud::TerminalLifecycleRequest::Spawn => {
                let session_name = match runtime_spawner.create_session(PERSISTENT_SESSION_PREFIX) {
                    Ok(session_name) => session_name,
                    Err(error) => {
                        append_debug_log(format!("spawn terminal failed: {error}"));
                        continue;
                    }
                };
                let (terminal_id, _) = match spawn_attached_terminal_with_presentation(
                    &mut commands,
                    &mut images,
                    &mut terminal_manager,
                    &mut presentation_store,
                    &runtime_spawner,
                    session_name.clone(),
                    true,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        append_debug_log(format!(
                            "attach terminal failed for {}: {error}",
                            session_name
                        ));
                        let _ = runtime_spawner.kill_session(&session_name);
                        continue;
                    }
                };
                hud_state.reconcile_direct_terminal_input(terminal_manager.active_id());
                if matches!(
                    visibility_state.policy,
                    TerminalVisibilityPolicy::Isolate(_)
                ) {
                    visibility_state.policy = TerminalVisibilityPolicy::Isolate(terminal_id);
                }
                view_state.focus_terminal(Some(terminal_id));
                mark_terminal_sessions_dirty(&mut session_persistence, Some(&time));
                append_debug_log(format!(
                    "spawned terminal {} session={}",
                    terminal_id.0, session_name
                ));
                state_changed = true;
            }
            crate::hud::TerminalLifecycleRequest::KillActive => {
                match kill_active_terminal_session_and_remove(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut agent_directory,
                    &mut session_persistence,
                    &mut visibility_state,
                    &mut view_state,
                ) {
                    Ok(Some((terminal_id, session_name))) => {
                        append_debug_log(format!(
                            "killed terminal {} session={}",
                            terminal_id.0, session_name
                        ));
                        state_changed = true;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        if let Some(active_id) = terminal_manager.active_id() {
                            let session_name = terminal_manager
                                .get(active_id)
                                .map(|terminal| terminal.session_name.as_str())
                                .unwrap_or("<missing>");
                            append_debug_log(format!(
                                "kill terminal failed for {}: {error}",
                                session_name
                            ));
                        } else {
                            append_debug_log(format!("kill terminal failed: {error}"));
                        }
                    }
                }
                hud_state.reconcile_direct_terminal_input(terminal_manager.active_id());
            }
        }
        if state_changed {
            redraws.write(RequestRedraw);
        }
    }
}
