use crate::{
    hud::{HudIntent, HudLayoutState, HudModuleModel, HudRect},
    terminals::{
        TerminalFocusState, TerminalManager, TerminalPresentationStore, TerminalViewState,
    },
};
use bevy::prelude::Vec2;

use super::{debug_toolbar_buttons, DebugToolbarAction};

#[allow(
    clippy::too_many_arguments,
    reason = "toolbar hit routing needs geometry, terminal state, HUD state, and command output together"
)]
/// Handles pointer click.
pub(crate) fn handle_pointer_click(
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    presentation_store: &TerminalPresentationStore,
    view_state: &TerminalViewState,
    layout_state: &HudLayoutState,
    emitted_commands: &mut Vec<HudIntent>,
) {
    if !matches!(model, HudModuleModel::DebugToolbar(_)) {
        return;
    }
    for button in debug_toolbar_buttons(
        shell_rect,
        terminal_manager,
        focus_state,
        presentation_store,
        view_state,
        layout_state,
    ) {
        if !button.rect.contains(point) {
            continue;
        }
        match button.action {
            DebugToolbarAction::SpawnTerminal => emitted_commands.push(HudIntent::SpawnTerminal),
            DebugToolbarAction::ShowAll => emitted_commands.push(HudIntent::ShowAllTerminals),
            DebugToolbarAction::TogglePixelPerfect => {
                emitted_commands.push(HudIntent::ToggleActiveTerminalDisplayMode)
            }
            DebugToolbarAction::ResetView => emitted_commands.push(HudIntent::ResetTerminalView),
            DebugToolbarAction::SendCommand(command) => {
                emitted_commands.push(HudIntent::SendActiveTerminalCommand(command.to_owned()))
            }
            DebugToolbarAction::ToggleModule(id) => {
                emitted_commands.push(HudIntent::ToggleModule(id));
            }
        }
        break;
    }
}
