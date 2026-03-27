use crate::hud::{AgentListView, HudIntent, HudModuleModel, HudRect};
use bevy::prelude::Vec2;

use super::{agent_list_content_height, agent_rows};

/// Converts a click on an agent-list row into focus + isolate intents for that terminal.
///
/// The function rebuilds the currently visible row list from the retained module state and derived
/// view-model, then selects the first row whose rectangle contains the click point.
pub(crate) fn handle_pointer_click(
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    agent_list_view: &AgentListView,
    emitted_commands: &mut Vec<HudIntent>,
) {
    let HudModuleModel::AgentList(state) = model else {
        return;
    };
    for row in agent_rows(
        shell_rect,
        state.scroll_offset,
        state.hovered_terminal,
        agent_list_view,
    ) {
        if row.rect.contains(point) {
            let Some(terminal_id) = row.terminal_id else {
                continue;
            };
            emitted_commands.push(HudIntent::FocusTerminal(terminal_id));
            emitted_commands.push(HudIntent::HideAllButTerminal(terminal_id));
            break;
        }
    }
}

/// Updates the retained hovered-terminal id for the agent list and reports whether it changed.
///
/// Hover is recomputed from the current pointer position against the currently visible rows. Returning
/// a boolean lets the caller request redraw only when hover state actually changed.
pub(crate) fn handle_hover(
    model: &mut HudModuleModel,
    shell_rect: HudRect,
    point: Option<Vec2>,
    agent_list_view: &AgentListView,
) -> bool {
    let HudModuleModel::AgentList(state) = model else {
        return false;
    };
    let hovered_terminal = point.and_then(|point| {
        agent_rows(shell_rect, state.scroll_offset, None, agent_list_view)
            .into_iter()
            .find(|row| row.rect.contains(point))
            .and_then(|row| row.terminal_id)
    });
    if state.hovered_terminal == hovered_terminal {
        return false;
    }
    state.hovered_terminal = hovered_terminal;
    true
}

/// Applies vertical scrolling to the agent-list module.
///
/// Scroll offset is clamped against the current content height so the list can never scroll past its
/// real bounds even as the row count changes.
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

/// Clears any retained hover target from the agent list and reports whether that changed state.
///
/// This lets the caller avoid unnecessary redraws when the list was already in a non-hovered state.
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
