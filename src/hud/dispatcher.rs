use crate::{
    hud::{
        AgentDirectory, HudIntent, HudModuleId, HudModuleRequest, HudState, TerminalFocusRequest,
        TerminalLifecycleRequest, TerminalSendRequest, TerminalTaskRequest, TerminalViewRequest,
        TerminalVisibilityPolicy, TerminalVisibilityRequest, TerminalVisibilityState,
    },
    terminals::{
        append_debug_log, kill_active_terminal_session_and_remove, mark_terminal_notes_dirty,
        mark_terminal_sessions_dirty, spawn_attached_terminal_with_presentation, TerminalManager,
        TerminalNotesState, TerminalPresentationStore, TerminalRuntimeSpawner,
        TerminalSessionPersistenceState, TerminalViewState, PERSISTENT_SESSION_PREFIX,
    },
};
use bevy::{prelude::*, window::RequestRedraw};

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
        presentation_store,
        runtime_spawner,
        agent_directory,
        session_persistence,
        visibility_state,
        view_state,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "intent fanout is intentionally explicit across narrow request channels"
)]
pub(crate) fn dispatch_hud_intents(
    mut intents: MessageReader<HudIntent>,
    mut focus_requests: MessageWriter<TerminalFocusRequest>,
    mut visibility_requests: MessageWriter<TerminalVisibilityRequest>,
    mut module_requests: MessageWriter<HudModuleRequest>,
    mut view_requests: MessageWriter<TerminalViewRequest>,
    mut send_requests: MessageWriter<TerminalSendRequest>,
    mut lifecycle_requests: MessageWriter<TerminalLifecycleRequest>,
    mut task_requests: MessageWriter<TerminalTaskRequest>,
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
            HudIntent::SetTerminalTaskText(terminal_id, text) => {
                task_requests.write(TerminalTaskRequest::SetText {
                    terminal_id: *terminal_id,
                    text: text.clone(),
                });
            }
            HudIntent::AppendTerminalTask(terminal_id, text) => {
                task_requests.write(TerminalTaskRequest::Append {
                    terminal_id: *terminal_id,
                    text: text.clone(),
                });
            }
            HudIntent::PrependTerminalTask(terminal_id, text) => {
                task_requests.write(TerminalTaskRequest::Prepend {
                    terminal_id: *terminal_id,
                    text: text.clone(),
                });
            }
            HudIntent::ConsumeNextTerminalTask(terminal_id) => {
                task_requests.write(TerminalTaskRequest::ConsumeNext {
                    terminal_id: *terminal_id,
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
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        terminal_manager.focus_terminal(request.terminal_id);
        hud_state.reconcile_direct_terminal_input(terminal_manager.active_id());
        view_state.focus_terminal(terminal_manager.active_id());
        mark_terminal_sessions_dirty(&mut session_persistence, Some(&time));
        redraws.write(RequestRedraw);
    }
}

pub(crate) fn apply_visibility_requests(
    mut requests: MessageReader<TerminalVisibilityRequest>,
    terminal_manager: Res<TerminalManager>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut redraws: MessageWriter<RequestRedraw>,
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
        redraws.write(RequestRedraw);
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
    mut redraws: MessageWriter<RequestRedraw>,
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
        redraws.write(RequestRedraw);
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

pub(crate) fn apply_terminal_task_requests(
    mut requests: MessageReader<TerminalTaskRequest>,
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    mut notes_state: ResMut<TerminalNotesState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    for request in requests.read() {
        let changed = match request {
            TerminalTaskRequest::SetText { terminal_id, text } => terminal_manager
                .get(*terminal_id)
                .is_some_and(|terminal| notes_state.set_note_text(&terminal.session_name, text)),
            TerminalTaskRequest::Append { terminal_id, text } => {
                terminal_manager.get(*terminal_id).is_some_and(|terminal| {
                    notes_state.append_task_from_text(&terminal.session_name, text)
                })
            }
            TerminalTaskRequest::Prepend { terminal_id, text } => {
                terminal_manager.get(*terminal_id).is_some_and(|terminal| {
                    notes_state.prepend_task_from_text(&terminal.session_name, text)
                })
            }
            TerminalTaskRequest::ConsumeNext { terminal_id } => {
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
                    notes_state.set_note_text(&terminal.session_name, &updated_task_text)
                })
            }
        };
        if changed {
            mark_terminal_notes_dirty(&mut notes_state, Some(&time));
            redraws.write(RequestRedraw);
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
            TerminalLifecycleRequest::Spawn => {
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
            TerminalLifecycleRequest::KillActive => {
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
