use crate::app::{AgentCommand, AppCommand, OwnedTmuxCommand};

use super::super::super::{
    state::{AgentListUiState, HudRect},
    view_models::{AgentListRowKey, AgentListRowKind, AgentListView},
};
use bevy::prelude::Vec2;

use super::{agent_list_content_height, agent_row_text_hit_rect, agent_rows, row_main_rect};

/// Returns the agent-list row currently under the pointer, if any.
pub(crate) fn row_at_point(
    state: &AgentListUiState,
    shell_rect: HudRect,
    point: Vec2,
    agent_list_view: &AgentListView,
) -> Option<AgentListRowKey> {
    agent_rows(
        shell_rect,
        state.scroll_offset,
        state.hovered_row.as_ref(),
        agent_list_view,
    )
    .into_iter()
    .find(|row| row.rect.contains(point))
    .map(|row| row.key)
}

pub(crate) fn text_row_at_point(
    state: &AgentListUiState,
    shell_rect: HudRect,
    point: Vec2,
    agent_list_view: &AgentListView,
) -> Option<AgentListRowKey> {
    agent_rows(
        shell_rect,
        state.scroll_offset,
        state.hovered_row.as_ref(),
        agent_list_view,
    )
    .into_iter()
    .find(|row| agent_row_text_hit_rect(row_main_rect(row)).contains(point))
    .map(|row| row.key)
}

pub(crate) fn selected_text_for_rows(
    agent_list_view: &AgentListView,
    anchor: &AgentListRowKey,
    focus: &AgentListRowKey,
) -> Option<String> {
    let start = agent_list_view
        .rows
        .iter()
        .position(|row| &row.key == anchor)?;
    let end = agent_list_view
        .rows
        .iter()
        .position(|row| &row.key == focus)?;
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let text = agent_list_view.rows[start..=end]
        .iter()
        .map(|row| match row.kind {
            AgentListRowKind::OwnedTmux { .. } => format!("↳ {}", row.label),
            AgentListRowKind::Agent { .. } => row.label.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

/// Computes the display-order slot the cursor currently points at during a drag reorder.
pub(crate) fn reorder_target_index(
    state: &AgentListUiState,
    shell_rect: HudRect,
    point: Vec2,
    agent_list_view: &AgentListView,
) -> Option<usize> {
    let rows = agent_rows(
        shell_rect,
        state.scroll_offset,
        state.hovered_row.as_ref(),
        agent_list_view,
    )
    .into_iter()
    .filter(|row| !row.is_tmux_child())
    .collect::<Vec<_>>();
    if rows.is_empty() {
        return None;
    }
    Some(
        rows.iter()
            .position(|row| point.y <= row.rect.y + row.rect.h * 0.5)
            .unwrap_or(rows.len() - 1),
    )
}

/// Converts a click on an agent-list row into the appropriate app command(s).
pub(crate) fn handle_pointer_click(
    state: &AgentListUiState,
    shell_rect: HudRect,
    point: Vec2,
    agent_list_view: &AgentListView,
    emitted_commands: &mut Vec<AppCommand>,
) {
    let Some(key) = row_at_point(state, shell_rect, point, agent_list_view) else {
        return;
    };
    match key {
        AgentListRowKey::Agent(agent_id) => {
            emitted_commands.push(AppCommand::OwnedTmux(OwnedTmuxCommand::ClearSelection));
            emitted_commands.push(AppCommand::Agent(AgentCommand::Focus(agent_id)));
            emitted_commands.push(AppCommand::Agent(AgentCommand::Inspect(agent_id)));
        }
        AgentListRowKey::OwnedTmux(session_uid) => {
            emitted_commands.push(AppCommand::OwnedTmux(OwnedTmuxCommand::Select {
                session_uid,
            }));
        }
    }
}

/// Updates the retained hovered-row key for the agent list and reports whether it changed.
pub(crate) fn handle_hover(
    state: &mut AgentListUiState,
    shell_rect: HudRect,
    point: Option<Vec2>,
    agent_list_view: &AgentListView,
) -> bool {
    let hovered_row =
        point.and_then(|point| row_at_point(state, shell_rect, point, agent_list_view));
    if state.hovered_row == hovered_row {
        return false;
    }
    state.hovered_row = hovered_row;
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
    if state.hovered_row.is_none() {
        return false;
    }
    state.hovered_row = None;
    true
}
