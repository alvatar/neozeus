use crate::{
    hud::{HudCommand, HudDispatcher, HudModuleModel, HudRect, HudState},
    terminals::{TerminalManager, TerminalPresentationStore, TerminalViewState},
};
use bevy::prelude::Vec2;

use super::{debug_toolbar_buttons, DebugToolbarAction};

#[allow(
    clippy::too_many_arguments,
    reason = "toolbar hit routing needs geometry, terminal state, HUD state, and dispatcher together"
)]
pub(crate) fn handle_pointer_click(
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
    view_state: &TerminalViewState,
    hud_state: &HudState,
    dispatcher: &mut HudDispatcher,
) {
    if !matches!(model, HudModuleModel::DebugToolbar(_)) {
        return;
    }
    for button in debug_toolbar_buttons(
        shell_rect,
        terminal_manager,
        presentation_store,
        view_state,
        hud_state,
    ) {
        if !button.rect.contains(point) {
            continue;
        }
        match button.action {
            DebugToolbarAction::SpawnTerminal => {
                dispatcher.commands.push(HudCommand::SpawnTerminal)
            }
            DebugToolbarAction::ShowAll => dispatcher.commands.push(HudCommand::ShowAllTerminals),
            DebugToolbarAction::TogglePixelPerfect => dispatcher
                .commands
                .push(HudCommand::ToggleActiveTerminalDisplayMode),
            DebugToolbarAction::ResetView => {
                dispatcher.commands.push(HudCommand::ResetTerminalView)
            }
            DebugToolbarAction::SendCommand(command) => dispatcher
                .commands
                .push(HudCommand::SendActiveTerminalCommand(command.to_owned())),
            DebugToolbarAction::ToggleModule(id) => {
                dispatcher.commands.push(HudCommand::ToggleModule(id));
            }
        }
        break;
    }
}
