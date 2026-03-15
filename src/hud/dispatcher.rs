use crate::{
    hud::{HudCommand, HudState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        append_debug_log, spawn_terminal_presentation, TerminalManager, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalViewState,
    },
};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct HudDispatcher {
    pub(crate) commands: Vec<HudCommand>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD command application touches app/domain resources, terminal runtime, and HUD state together"
)]
pub(crate) fn apply_hud_commands(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    mut hud_state: ResMut<HudState>,
    mut dispatcher: ResMut<HudDispatcher>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut view_state: ResMut<TerminalViewState>,
) {
    let queued = std::mem::take(&mut dispatcher.commands);
    for command in queued {
        match command {
            HudCommand::SpawnTerminal => {
                let bridge = runtime_spawner.spawn();
                let (terminal_id, slot) = terminal_manager.create_terminal_with_slot(bridge);
                spawn_terminal_presentation(
                    &mut commands,
                    &mut images,
                    &mut presentation_store,
                    terminal_id,
                    slot,
                );
                append_debug_log(format!("spawned terminal {}", terminal_id.0));
            }
            HudCommand::FocusTerminal(id) => {
                terminal_manager.focus_terminal(id);
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
            HudCommand::ToggleActiveTerminalDisplayMode => {
                let active_id = terminal_manager.active_id();
                presentation_store.toggle_active_display_mode(active_id);
            }
            HudCommand::ResetTerminalView => {
                view_state.distance = 10.0;
                view_state.offset = Vec2::ZERO;
            }
            HudCommand::SendActiveTerminalCommand(command) => {
                if let Some(bridge) = terminal_manager.active_bridge() {
                    bridge.send(crate::terminals::TerminalCommand::SendCommand(command));
                }
            }
        }
    }
}
