use crate::app::{AgentCommand, AppCommand, TerminalCommand, WidgetCommand};

use super::super::super::{
    state::{HudLayoutState, HudRect},
    view_models::DebugToolbarView,
};
use bevy::prelude::Vec2;

use super::{debug_toolbar_buttons, DebugToolbarAction};

#[allow(
    clippy::too_many_arguments,
    reason = "toolbar hit routing needs geometry, toolbar view state, HUD state, and command output together"
)]
/// Converts a debug-toolbar click into the corresponding app command.
pub(crate) fn handle_pointer_click(
    shell_rect: HudRect,
    point: Vec2,
    debug_toolbar_view: &DebugToolbarView,
    layout_state: &HudLayoutState,
    emitted_commands: &mut Vec<AppCommand>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    for button in debug_toolbar_buttons(shell_rect, debug_toolbar_view, layout_state) {
        if !button.rect.contains(point) {
            continue;
        }
        match button.action {
            DebugToolbarAction::SpawnTerminal => {
                emitted_commands.push(AppCommand::Agent(AgentCommand::SpawnTerminal))
            }
            DebugToolbarAction::ShowAll => {
                emitted_commands.push(AppCommand::Agent(AgentCommand::ShowAll))
            }
            DebugToolbarAction::TogglePixelPerfect => emitted_commands.push(AppCommand::Terminal(
                TerminalCommand::ToggleActiveDisplayMode,
            )),
            DebugToolbarAction::ResetView => {
                emitted_commands.push(AppCommand::Terminal(TerminalCommand::ResetActiveView))
            }
            DebugToolbarAction::SendCommand(command) => {
                emitted_commands.push(AppCommand::Terminal(TerminalCommand::SendCommandToActive {
                    command: command.to_owned(),
                }))
            }
            DebugToolbarAction::ToggleModule(id) => {
                emitted_commands.push(AppCommand::Widget(WidgetCommand::Toggle(id)));
            }
        }
        break;
    }
}
