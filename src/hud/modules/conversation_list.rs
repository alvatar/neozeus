use crate::{
    agents::AgentId,
    app::{AgentCommand, AppCommand},
};

use super::super::{
    render::{HudColors, HudPainter, HudRenderInputs},
    state::{ConversationListUiState, HudRect, HUD_ROW_HEIGHT},
    view_models::ConversationListView,
};
use bevy::prelude::Vec2;
use bevy_vello::prelude::VelloTextAnchor;

const ROW_GAP: f32 = 10.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ConversationRow {
    pub(crate) agent_id: AgentId,
    pub(crate) rect: HudRect,
    pub(crate) label: String,
    pub(crate) message_count: usize,
    pub(crate) selected: bool,
    pub(crate) hovered: bool,
}

/// Handles row stride.
fn row_stride() -> f32 {
    HUD_ROW_HEIGHT + ROW_GAP
}

/// Handles rows.
pub(crate) fn rows(
    shell_rect: HudRect,
    scroll_offset: f32,
    hovered_agent: Option<AgentId>,
    conversation_list_view: &ConversationListView,
) -> Vec<ConversationRow> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    conversation_list_view
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| ConversationRow {
            agent_id: row.agent_id,
            rect: HudRect {
                x: shell_rect.x,
                y: shell_rect.y + index as f32 * row_stride() - scroll_offset,
                w: shell_rect.w,
                h: HUD_ROW_HEIGHT,
            },
            label: row.label.clone(),
            message_count: row.message_count,
            selected: row.selected,
            hovered: hovered_agent == Some(row.agent_id),
        })
        .collect()
}

/// Handles pointer click.
pub(crate) fn handle_pointer_click(
    state: &ConversationListUiState,
    shell_rect: HudRect,
    point: Vec2,
    conversation_list_view: &ConversationListView,
    emitted_commands: &mut Vec<AppCommand>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    for row in rows(
        shell_rect,
        state.scroll_offset,
        state.hovered_agent,
        conversation_list_view,
    ) {
        if !row.rect.contains(point) {
            continue;
        }
        emitted_commands.push(AppCommand::Agent(AgentCommand::Focus(row.agent_id)));
        emitted_commands.push(AppCommand::Agent(AgentCommand::Inspect(row.agent_id)));
        break;
    }
}

/// Handles hover.
pub(crate) fn handle_hover(
    state: &mut ConversationListUiState,
    shell_rect: HudRect,
    point: Option<Vec2>,
    conversation_list_view: &ConversationListView,
) -> bool {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    let hovered_agent = point.and_then(|point| {
        rows(
            shell_rect,
            state.scroll_offset,
            None,
            conversation_list_view,
        )
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

/// Clears hover.
pub(crate) fn clear_hover(state: &mut ConversationListUiState) -> bool {
    if state.hovered_agent.is_none() {
        return false;
    }
    state.hovered_agent = None;
    true
}

/// Handles scroll.
pub(crate) fn handle_scroll(
    state: &mut ConversationListUiState,
    delta_y: f32,
    row_count: usize,
    height: f32,
) {
    let content_height = match row_count {
        0 => 0.0,
        _ => row_count as f32 * row_stride() - ROW_GAP,
    }
    .max(height);
    let max_scroll = (content_height - height).max(0.0);
    state.scroll_offset = (state.scroll_offset - delta_y).clamp(0.0, max_scroll);
}

/// Renders content.
pub(crate) fn render_content(
    state: &ConversationListUiState,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    painter.label(
        Vec2::new(content_rect.x + 8.0, content_rect.y + 6.0),
        "Recent conversations",
        14.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );

    for row in rows(
        content_rect,
        state.scroll_offset,
        state.hovered_agent,
        inputs.conversation_list_view,
    ) {
        if row.rect.y + row.rect.h < content_rect.y || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }
        painter.fill_rect(
            row.rect,
            if row.selected {
                HudColors::ROW_FOCUSED
            } else if row.hovered {
                HudColors::ROW_HOVERED
            } else {
                HudColors::FRAME
            },
            4.0,
        );
        painter.stroke_rect(row.rect, HudColors::BORDER, 4.0);
        painter.label(
            Vec2::new(row.rect.x + 10.0, row.rect.y + 6.0),
            &format!("{} · {}", row.label, row.message_count),
            14.0,
            HudColors::TEXT,
            VelloTextAnchor::TopLeft,
        );
    }
}
