use crate::app::{AgentCommand, AppCommand};

use super::super::super::{
    state::{AgentListUiState, HudRect},
    view_models::AgentListView,
};
use bevy::prelude::Vec2;

use super::{agent_list_content_height, agent_rows};

/// Converts a click on an agent-list row into focus + inspect commands.
pub(crate) fn handle_pointer_click(
    state: &AgentListUiState,
    shell_rect: HudRect,
    point: Vec2,
    agent_list_view: &AgentListView,
    emitted_commands: &mut Vec<AppCommand>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    for row in agent_rows(
        shell_rect,
        state.scroll_offset,
        state.hovered_agent,
        agent_list_view,
    ) {
        if row.rect.contains(point) {
            emitted_commands.push(AppCommand::Agent(AgentCommand::Focus(row.agent_id)));
            emitted_commands.push(AppCommand::Agent(AgentCommand::Inspect(row.agent_id)));
            break;
        }
    }
}

/// Updates the retained hovered-agent id for the agent list and reports whether it changed.
pub(crate) fn handle_hover(
    state: &mut AgentListUiState,
    shell_rect: HudRect,
    point: Option<Vec2>,
    agent_list_view: &AgentListView,
) -> bool {
    let hovered_agent = point.and_then(|point| {
        agent_rows(shell_rect, state.scroll_offset, None, agent_list_view)
            .into_iter()
            .find(|row| row.rect.contains(point))
            .map(|row| row.agent_id)
    });
    if state.hovered_agent == hovered_agent {
        return false;
    }
    state.hovered_agent = hovered_agent;
    true
}

/// Applies vertical scrolling to the agent-list module.
pub(crate) fn handle_scroll(
    state: &mut AgentListUiState,
    delta_y: f32,
    row_count: usize,
    height: f32,
) {
    let content_height = agent_list_content_height(row_count).max(height);
    let max_scroll = (content_height - height).max(0.0);
    state.scroll_offset = (state.scroll_offset - delta_y).clamp(0.0, max_scroll);
}

/// Clears any retained hover target from the agent list and reports whether that changed state.
pub(crate) fn clear_hover(state: &mut AgentListUiState) -> bool {
    if state.hovered_agent.is_none() {
        return false;
    }
    state.hovered_agent = None;
    true
}
