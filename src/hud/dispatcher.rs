use crate::{
    hud::{
        AgentDirectory, HudCommand, HudState, TerminalVisibilityPolicy, TerminalVisibilityState,
    },
    terminals::{
        append_debug_log, generate_unique_session_name, mark_terminal_sessions_dirty,
        provision_terminal_target, spawn_attached_terminal_with_presentation, TerminalManager,
        TerminalPanel, TerminalPanelFrame, TerminalPresentationStore, TerminalProvisionTarget,
        TerminalRuntimeSpawner, TerminalSessionPersistenceState, TerminalViewState,
        TmuxClientResource, PERSISTENT_TMUX_SESSION_PREFIX,
    },
};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct HudDispatcher {
    pub(crate) commands: Vec<HudCommand>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal kill must touch tmux, manager/store resources, labels, persistence, and ECS entities together"
)]
pub(crate) fn kill_active_terminal(
    commands: &mut Commands,
    time: &Time,
    terminal_manager: &mut TerminalManager,
    presentation_store: &mut TerminalPresentationStore,
    tmux_client: &dyn crate::terminals::TmuxClient,
    agent_directory: &mut AgentDirectory,
    session_persistence: &mut TerminalSessionPersistenceState,
    visibility_state: &mut TerminalVisibilityState,
    view_state: &mut TerminalViewState,
    terminal_panels: &Query<(Entity, &TerminalPanel)>,
    terminal_frames: &Query<(Entity, &TerminalPanelFrame)>,
) {
    let Some(active_id) = terminal_manager.active_id() else {
        return;
    };
    let Some(session_name) = terminal_manager
        .get(active_id)
        .map(|terminal| terminal.session_name.clone())
    else {
        return;
    };
    if let Err(error) = tmux_client.kill_session(&session_name) {
        append_debug_log(format!(
            "kill terminal failed for {}: {error}",
            session_name
        ));
        return;
    }

    let _ = terminal_manager.remove_terminal(active_id);
    let _ = presentation_store.remove(active_id);
    agent_directory.labels.remove(&active_id);
    visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
    view_state.forget_terminal(active_id);
    view_state.focus_terminal(terminal_manager.active_id());
    mark_terminal_sessions_dirty(session_persistence, Some(time));

    for (entity, panel) in terminal_panels {
        if panel.id == active_id {
            commands.entity(entity).despawn();
        }
    }
    for (entity, frame) in terminal_frames {
        if frame.id == active_id {
            commands.entity(entity).despawn();
        }
    }
    append_debug_log(format!(
        "killed terminal {} session={}",
        active_id.0, session_name
    ));
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD command application touches app/domain resources, terminal runtime, and HUD state together"
)]
pub(crate) fn apply_hud_commands(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    tmux_client: Res<TmuxClientResource>,
    mut hud_state: ResMut<HudState>,
    mut dispatcher: ResMut<HudDispatcher>,
    mut agent_directory: ResMut<AgentDirectory>,
    mut session_persistence: ResMut<TerminalSessionPersistenceState>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut view_state: ResMut<TerminalViewState>,
    terminal_panels: Query<(Entity, &TerminalPanel)>,
    terminal_frames: Query<(Entity, &TerminalPanelFrame)>,
) {
    let queued = std::mem::take(&mut dispatcher.commands);
    for command in queued {
        match command {
            HudCommand::SpawnTerminal => {
                let client = tmux_client.client();
                let Ok(session_name) =
                    generate_unique_session_name(client, PERSISTENT_TMUX_SESSION_PREFIX)
                else {
                    append_debug_log("spawn terminal failed: could not allocate tmux session name");
                    continue;
                };
                if let Err(error) = provision_terminal_target(
                    client,
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
                    session_name.clone(),
                    true,
                );
                view_state.focus_terminal(Some(terminal_id));
                mark_terminal_sessions_dirty(&mut session_persistence, Some(&time));
                append_debug_log(format!(
                    "spawned terminal {} session={}",
                    terminal_id.0, session_name
                ));
            }
            HudCommand::FocusTerminal(id) => {
                terminal_manager.focus_terminal(id);
                view_state.focus_terminal(terminal_manager.active_id());
                mark_terminal_sessions_dirty(&mut session_persistence, Some(&time));
            }
            HudCommand::HideAllButTerminal(id) => {
                visibility_state.policy = TerminalVisibilityPolicy::Isolate(id);
                append_debug_log(format!("hud visibility isolate {}", id.0));
            }
            HudCommand::ShowAllTerminals => {
                visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
                append_debug_log("hud visibility show-all");
            }
            HudCommand::ToggleModule(id) => {
                let enabled = hud_state
                    .get(id)
                    .is_some_and(|module| !module.shell.enabled);
                hud_state.set_module_enabled(id, enabled);
            }
            HudCommand::ResetModule(id) => {
                hud_state.reset_module(id);
                append_debug_log(format!("hud module reset {}", id.number()));
            }
            HudCommand::ToggleActiveTerminalDisplayMode => {
                let active_id = terminal_manager.active_id();
                presentation_store.toggle_active_display_mode(active_id);
            }
            HudCommand::ResetTerminalView => {
                view_state.distance = 10.0;
                view_state.reset_active_offset(terminal_manager.active_id());
            }
            HudCommand::SendActiveTerminalCommand(command) => {
                if let Some(bridge) = terminal_manager.active_bridge() {
                    bridge.send(crate::terminals::TerminalCommand::SendCommand(command));
                }
            }
            HudCommand::SendTerminalCommand(id, command) => {
                if let Some(terminal) = terminal_manager.get(id) {
                    terminal
                        .bridge
                        .send(crate::terminals::TerminalCommand::SendCommand(command));
                }
            }
            HudCommand::KillActiveTerminal => kill_active_terminal(
                &mut commands,
                &time,
                &mut terminal_manager,
                &mut presentation_store,
                tmux_client.client(),
                &mut agent_directory,
                &mut session_persistence,
                &mut visibility_state,
                &mut view_state,
                &terminal_panels,
                &terminal_frames,
            ),
        }
    }
}
