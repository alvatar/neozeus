use crate::{
    hud::{AgentDirectory, HudIntent, HudModuleModel, HudRect},
    terminals::{TerminalFocusState, TerminalManager},
};
use bevy::prelude::Vec2;

use super::{agent_list_content_height, agent_rows};

/// Handles pointer click.
pub(crate) fn handle_pointer_click(
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    agent_directory: &AgentDirectory,
    emitted_commands: &mut Vec<HudIntent>,
) {
    let HudModuleModel::AgentList(state) = model else {
        return;
    };
    for row in agent_rows(
        shell_rect,
        state.scroll_offset,
        state.hovered_terminal,
        terminal_manager,
        focus_state,
        agent_directory,
    ) {
        if row.rect.contains(point) {
            emitted_commands.push(HudIntent::FocusTerminal(row.terminal_id));
            emitted_commands.push(HudIntent::HideAllButTerminal(row.terminal_id));
            break;
        }
    }
}

/// Handles hover.
pub(crate) fn handle_hover(
    model: &mut HudModuleModel,
    shell_rect: HudRect,
    point: Option<Vec2>,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    agent_directory: &AgentDirectory,
) -> bool {
    let HudModuleModel::AgentList(state) = model else {
        return false;
    };
    let hovered_terminal = point.and_then(|point| {
        agent_rows(
            shell_rect,
            state.scroll_offset,
            None,
            terminal_manager,
            focus_state,
            agent_directory,
        )
        .into_iter()
        .find(|row| row.rect.contains(point))
        .map(|row| row.terminal_id)
    });
    if state.hovered_terminal == hovered_terminal {
        return false;
    }
    state.hovered_terminal = hovered_terminal;
    true
}

/// Handles scroll.
pub(crate) fn handle_scroll(
    model: &mut HudModuleModel,
    delta_y: f32,
    row_count: usize,
    height: f32,
) {
    let HudModuleModel::AgentList(state) = model else {
        return;
    };
    let content_height = agent_list_content_height(row_count).max(height);
    let max_scroll = (content_height - height).max(0.0);
    state.scroll_offset = (state.scroll_offset - delta_y).clamp(0.0, max_scroll);
}

/// Clears hover.
pub(crate) fn clear_hover(model: &mut HudModuleModel) -> bool {
    let HudModuleModel::AgentList(state) = model else {
        return false;
    };
    if state.hovered_terminal.is_none() {
        return false;
    }
    state.hovered_terminal = None;
    true
}
