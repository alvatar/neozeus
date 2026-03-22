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
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(Resource, Default)]
pub(crate) struct HudDispatcher {
    pub(crate) commands: Vec<HudCommand>,
}

#[derive(SystemParam)]
pub(crate) struct HudCommandContext<'w, 's> {
    pub(crate) commands: Commands<'w, 's>,
    pub(crate) images: ResMut<'w, Assets<Image>>,
    pub(crate) time: Res<'w, Time>,
    pub(crate) terminal_manager: ResMut<'w, TerminalManager>,
    pub(crate) presentation_store: ResMut<'w, TerminalPresentationStore>,
    pub(crate) runtime_spawner: Res<'w, TerminalRuntimeSpawner>,
    pub(crate) tmux_client: Res<'w, TmuxClientResource>,
    pub(crate) hud_state: ResMut<'w, HudState>,
    pub(crate) dispatcher: ResMut<'w, HudDispatcher>,
    pub(crate) agent_directory: ResMut<'w, AgentDirectory>,
    pub(crate) session_persistence: ResMut<'w, TerminalSessionPersistenceState>,
    pub(crate) visibility_state: ResMut<'w, TerminalVisibilityState>,
    pub(crate) view_state: ResMut<'w, TerminalViewState>,
    pub(crate) terminal_panels: Query<'w, 's, (Entity, &'static TerminalPanel)>,
    pub(crate) terminal_frames: Query<'w, 's, (Entity, &'static TerminalPanelFrame)>,
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

pub(crate) fn apply_hud_commands(mut ctx: HudCommandContext) {
    let queued = std::mem::take(&mut ctx.dispatcher.commands);
    for command in queued {
        match command {
            HudCommand::SpawnTerminal => {
                let client = ctx.tmux_client.client();
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
                    &mut ctx.commands,
                    &mut ctx.images,
                    &mut ctx.terminal_manager,
                    &mut ctx.presentation_store,
                    &ctx.runtime_spawner,
                    session_name.clone(),
                    true,
                );
                ctx.hud_state
                    .reconcile_direct_terminal_input(ctx.terminal_manager.active_id());
                ctx.view_state.focus_terminal(Some(terminal_id));
                mark_terminal_sessions_dirty(&mut ctx.session_persistence, Some(&ctx.time));
                append_debug_log(format!(
                    "spawned terminal {} session={}",
                    terminal_id.0, session_name
                ));
            }
            HudCommand::FocusTerminal(id) => {
                ctx.terminal_manager.focus_terminal(id);
                ctx.hud_state
                    .reconcile_direct_terminal_input(ctx.terminal_manager.active_id());
                ctx.view_state
                    .focus_terminal(ctx.terminal_manager.active_id());
                mark_terminal_sessions_dirty(&mut ctx.session_persistence, Some(&ctx.time));
            }
            HudCommand::HideAllButTerminal(id) => {
                ctx.visibility_state.policy = TerminalVisibilityPolicy::Isolate(id);
                append_debug_log(format!("hud visibility isolate {}", id.0));
            }
            HudCommand::ShowAllTerminals => {
                ctx.visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
                append_debug_log("hud visibility show-all");
            }
            HudCommand::ToggleModule(id) => {
                let enabled = ctx
                    .hud_state
                    .get(id)
                    .is_some_and(|module| !module.shell.enabled);
                ctx.hud_state.set_module_enabled(id, enabled);
            }
            HudCommand::ResetModule(id) => {
                ctx.hud_state.reset_module(id);
                append_debug_log(format!("hud module reset {}", id.number()));
            }
            HudCommand::ToggleActiveTerminalDisplayMode => {
                let active_id = ctx.terminal_manager.active_id();
                ctx.presentation_store.toggle_active_display_mode(active_id);
            }
            HudCommand::ResetTerminalView => {
                ctx.view_state.distance = 10.0;
                ctx.view_state
                    .reset_active_offset(ctx.terminal_manager.active_id());
            }
            HudCommand::SendActiveTerminalCommand(command) => {
                if let Some(bridge) = ctx.terminal_manager.active_bridge() {
                    bridge.send(crate::terminals::TerminalCommand::SendCommand(command));
                }
            }
            HudCommand::SendTerminalCommand(id, command) => {
                if let Some(terminal) = ctx.terminal_manager.get(id) {
                    terminal
                        .bridge
                        .send(crate::terminals::TerminalCommand::SendCommand(command));
                }
            }
            HudCommand::KillActiveTerminal => {
                kill_active_terminal(
                    &mut ctx.commands,
                    &ctx.time,
                    &mut ctx.terminal_manager,
                    &mut ctx.presentation_store,
                    ctx.tmux_client.client(),
                    &mut ctx.agent_directory,
                    &mut ctx.session_persistence,
                    &mut ctx.visibility_state,
                    &mut ctx.view_state,
                    &ctx.terminal_panels,
                    &ctx.terminal_frames,
                );
                ctx.hud_state
                    .reconcile_direct_terminal_input(ctx.terminal_manager.active_id());
            }
        }
    }
}
