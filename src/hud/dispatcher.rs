use crate::{
    hud::{
        AgentDirectory, HudIntent, HudModuleId, HudModuleRequest, HudState, TerminalFocusRequest,
        TerminalLifecycleRequest, TerminalSendRequest, TerminalViewRequest,
        TerminalVisibilityPolicy, TerminalVisibilityRequest, TerminalVisibilityState,
    },
    terminals::{
        append_debug_log, generate_unique_session_name, kill_active_terminal_session_and_remove,
        mark_terminal_sessions_dirty, provision_terminal_target,
        spawn_attached_terminal_with_presentation, TerminalManager, TerminalPresentationStore,
        TerminalProvisionTarget, TerminalRuntimeSpawner, TerminalSessionPersistenceState,
        TerminalViewState, TmuxClientResource,
        PERSISTENT_TMUX_SESSION_PREFIX,
    },
};
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
    presentation_store: &mut TerminalPresentationStore,
    session_client: &dyn crate::terminals::TerminalSessionClient,
    agent_directory: &mut AgentDirectory,
    session_persistence: &mut TerminalSessionPersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
) -> Result<Option<(crate::terminals::TerminalId, String)>, String> {
    kill_active_terminal_session_and_remove(
        commands,
        time,
        terminal_manager,
        presentation_store,
        session_client,
        agent_directory,
        session_persistence,
        visibility_state,
        view_state,
    )
}

pub(crate) fn dispatch_hud_intents(
    mut intents: MessageReader<HudIntent>,
    mut focus_requests: MessageWriter<TerminalFocusRequest>,
    mut visibility_requests: MessageWriter<TerminalVisibilityRequest>,
    mut module_requests: MessageWriter<HudModuleRequest>,
    mut view_requests: MessageWriter<TerminalViewRequest>,
    mut send_requests: MessageWriter<TerminalSendRequest>,
    mut lifecycle_requests: MessageWriter<TerminalLifecycleRequest>,
) {
    for intent in intents.read() {
        match intent {
            HudIntent::SpawnTerminal => {
                lifecycle_requests.write(TerminalLifecycleRequest::Spawn);
            }
            HudIntent::FocusTerminal(terminal_id) => {
                focus_requests.write(TerminalFocusRequest {
                    terminal_id: *terminal_id,
                });
            }
            HudIntent::HideAllButTerminal(terminal_id) => {
                visibility_requests.write(TerminalVisibilityRequest::Isolate(*terminal_id));
            }
            HudIntent::ShowAllTerminals => {
                visibility_requests.write(TerminalVisibilityRequest::ShowAll);
            }
            HudIntent::ToggleModule(id) => {
                module_requests.write(HudModuleRequest::Toggle(*id));
            }
            HudIntent::ResetModule(id) => {
                module_requests.write(HudModuleRequest::Reset(*id));
            }
            HudIntent::ToggleActiveTerminalDisplayMode => {
                view_requests.write(TerminalViewRequest::ToggleActiveDisplayMode);
            }
            HudIntent::ResetTerminalView => {
                view_requests.write(TerminalViewRequest::ResetActiveView);
            }
            HudIntent::SendActiveTerminalCommand(command) => {
                send_requests.write(TerminalSendRequest::Active(command.clone()));
            }
            HudIntent::SendTerminalCommand(terminal_id, command) => {
                send_requests.write(TerminalSendRequest::Target {
                    terminal_id: *terminal_id,
                    command: command.clone(),
                });
            }
            HudIntent::KillActiveTerminal => {
                lifecycle_requests.write(TerminalLifecycleRequest::KillActive);
            }
        }
    }
}

fn toggle_module(hud_state: &mut HudState, id: HudModuleId) {
    let enabled = hud_state
        .get(id)
        .is_some_and(|module| !module.shell.enabled);
    hud_state.set_module_enabled(id, enabled);
}

pub(crate) fn apply_terminal_focus_requests(
    mut requests: MessageReader<TerminalFocusRequest>,
    time: Res<Time>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut hud_state: ResMut<HudState>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
    mut view_state: ResMut<TerminalViewState>,
) {
    for request in requests.read() {
        terminal_manager.focus_terminal(request.terminal_id);
        hud_state.reconcile_direct_terminal_input(terminal_manager.active_id());
        view_state.focus_terminal(terminal_manager.active_id());
        mark_terminal_sessions_dirty(&mut session_persistence, Some(&time));
    }
}

pub(crate) fn apply_visibility_requests(
    mut requests: MessageReader<TerminalVisibilityRequest>,
    terminal_manager: Res<TerminalManager>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
) {
    for request in requests.read() {
        match request {
            TerminalVisibilityRequest::Isolate(terminal_id) => {
                visibility_state.policy = if terminal_manager.get(*terminal_id).is_some() {
                    TerminalVisibilityPolicy::Isolate(*terminal_id)
                } else {
                    TerminalVisibilityPolicy::ShowAll
                };
                append_debug_log(format!("hud visibility {:?}", visibility_state.policy));
            }
            TerminalVisibilityRequest::ShowAll => {
                visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
                append_debug_log("hud visibility show-all");
            }
        }
    }
}

pub(crate) fn apply_hud_module_requests(
    mut requests: MessageReader<HudModuleRequest>,
    mut hud_state: ResMut<HudState>,
) {
    for request in requests.read() {
        match request {
            HudModuleRequest::Toggle(id) => toggle_module(&mut hud_state, *id),
            HudModuleRequest::Reset(id) => {
                hud_state.reset_module(*id);
                append_debug_log(format!("hud module reset {}", id.number()));
            }
        }
    }
}

pub(crate) fn apply_terminal_view_requests(
    mut requests: MessageReader<TerminalViewRequest>,
    terminal_manager: Res<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    mut view_state: ResMut<TerminalViewState>,
) {
    for request in requests.read() {
        match request {
            TerminalViewRequest::ToggleActiveDisplayMode => {
                presentation_store.toggle_active_display_mode(terminal_manager.active_id());
            }
            TerminalViewRequest::ResetActiveView => {
                view_state.distance = 10.0;
                view_state.reset_active_offset(terminal_manager.active_id());
            }
        }
    }
}

pub(crate) fn apply_terminal_send_requests(
    mut requests: MessageReader<TerminalSendRequest>,
    terminal_manager: Res<TerminalManager>,
) {
    for request in requests.read() {
        match request {
            TerminalSendRequest::Active(command) => {
                if let Some(bridge) = terminal_manager.active_bridge() {
                    bridge.send(crate::terminals::TerminalCommand::SendCommand(
                        command.clone(),
                    ));
                }
            }
            TerminalSendRequest::Target {
                terminal_id,
                command,
            } => {
                if let Some(terminal) = terminal_manager.get(*terminal_id) {
                    terminal
                        .bridge
                        .send(crate::terminals::TerminalCommand::SendCommand(
                            command.clone(),
                        ));
                }
            }
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal spawn spans tmux provisioning, runtime spawn, projection spawn, and persistence"
)]
pub(crate) fn apply_terminal_lifecycle_requests(
    mut requests: MessageReader<TerminalLifecycleRequest>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    tmux_client: Res<TmuxClientResource>,
    mut hud_state: ResMut<HudState>,
    mut agent_directory: ResMut<AgentDirectory>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut view_state: ResMut<TerminalViewState>,
) {
    for request in requests.read() {
        match request {
            TerminalLifecycleRequest::Spawn => {
                let session_client = tmux_client.session_client();
                let Ok(session_name) =
                    generate_unique_session_name(session_client, PERSISTENT_TMUX_SESSION_PREFIX)
                else {
                    append_debug_log("spawn terminal failed: could not allocate tmux session name");
                    continue;
                };
                if let Err(error) = provision_terminal_target(
                    session_client,
                    &TerminalProvisionTarget::TmuxDetached {
                        session_name: session_name.clone(),
                    },
                ) {
                    append_debug_log(format!(
                        "spawn terminal failed for {}: {error}",
                        session_name
                    ));
                    continue;
                }
                let (terminal_id, _) = spawn_attached_terminal_with_presentation(
                    &mut commands,
                    &mut images,
                    &mut terminal_manager,
                    &mut presentation_store,
                    &runtime_spawner,
                    &tmux_client,
                    session_name.clone(),
                    true,
                );
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
            }
            TerminalLifecycleRequest::KillActive => {
                match kill_active_terminal_session_and_remove(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut presentation_store,
                    tmux_client.session_client(),
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
    }
}
