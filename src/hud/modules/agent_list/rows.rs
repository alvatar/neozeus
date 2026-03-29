use crate::{
    agents::{AgentId, AgentStatus},
    terminals::TerminalId,
};

use super::super::super::{
    state::{HudRect, HUD_MODULE_PADDING, HUD_ROW_HEIGHT},
    view_models::AgentListView,
};

pub(crate) const AGENT_LIST_HEADER_HEIGHT: f32 = 52.0;
pub(crate) const AGENT_LIST_LEFT_RAIL_WIDTH: f32 = 20.0;
const AGENT_LIST_ROW_MARKER_WIDTH: f32 = 12.0;
const AGENT_LIST_ROW_MARKER_GAP: f32 = 10.0;
const AGENT_LIST_ROW_GAP: f32 = 14.0;
pub(crate) const AGENT_LIST_BORDER_ORANGE_R: u8 = 225;
pub(crate) const AGENT_LIST_BORDER_ORANGE_G: u8 = 129;
pub(crate) const AGENT_LIST_BORDER_ORANGE_B: u8 = 10;
pub(crate) const AGENT_LIST_BLOOM_RED_R: u8 = 143;
pub(crate) const AGENT_LIST_BLOOM_RED_G: u8 = 37;
pub(crate) const AGENT_LIST_BLOOM_RED_B: u8 = 15;
pub(crate) const AGENT_LIST_WORKING_GREEN_R: u8 = 82;
pub(crate) const AGENT_LIST_WORKING_GREEN_G: u8 = 173;
pub(crate) const AGENT_LIST_WORKING_GREEN_B: u8 = 112;
pub(crate) const AGENT_LIST_WORKING_GLOW_R: u8 = 84;
pub(crate) const AGENT_LIST_WORKING_GLOW_G: u8 = 220;
pub(crate) const AGENT_LIST_WORKING_GLOW_B: u8 = 190;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AgentListRowSection {
    Main,
    Marker,
    Accent,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::hud) struct AgentRow {
    pub(in crate::hud) agent_id: AgentId,
    pub(in crate::hud) terminal_id: Option<TerminalId>,
    pub(in crate::hud) label: String,
    pub(in crate::hud) display_label: String,
    pub(in crate::hud) rect: HudRect,
    pub(in crate::hud) focused: bool,
    pub(in crate::hud) hovered: bool,
    pub(in crate::hud) has_tasks: bool,
    pub(in crate::hud) interactive: bool,
    pub(in crate::hud) status: AgentStatus,
    pub(in crate::hud) dragging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::hud) struct AgentListDragPreview {
    pub(in crate::hud) agent_id: AgentId,
    pub(in crate::hud) cursor_y: f32,
    pub(in crate::hud) grab_offset_y: f32,
    pub(in crate::hud) target_index: usize,
}

/// Derives one sub-rectangle of an agent row for rendering or hit-testing.
///
/// A logical row is split into the main label box, the narrow status marker, and a tiny accent strip.
/// The helper bakes in the EVA-specific padding constants and clamps dimensions so very small rows do
/// not collapse to negative sizes.
pub(crate) fn agent_row_rect(rect: HudRect, section: AgentListRowSection) -> HudRect {
    match section {
        AgentListRowSection::Main => HudRect {
            x: rect.x,
            y: rect.y + 2.0,
            w: (rect.w - AGENT_LIST_ROW_MARKER_WIDTH - AGENT_LIST_ROW_MARKER_GAP).max(12.0),
            h: (rect.h - 4.0).max(10.0),
        },
        AgentListRowSection::Marker => HudRect {
            x: rect.x + rect.w - AGENT_LIST_ROW_MARKER_WIDTH,
            y: rect.y + 2.0,
            w: AGENT_LIST_ROW_MARKER_WIDTH,
            h: (rect.h - 4.0).max(10.0),
        },
        AgentListRowSection::Accent => HudRect {
            x: rect.x + 3.0,
            y: rect.y + 3.0,
            w: 8.0,
            h: (rect.h - 6.0).max(10.0),
        },
    }
}

/// Returns the vertical distance from one agent row origin to the next.
fn agent_row_stride() -> f32 {
    HUD_ROW_HEIGHT + AGENT_LIST_ROW_GAP
}

/// Computes the total scrollable content height for a given number of agent rows.
pub(crate) fn agent_list_content_height(row_count: usize) -> f32 {
    match row_count {
        0 => 0.0,
        _ => row_count as f32 * agent_row_stride() - AGENT_LIST_ROW_GAP,
    }
}

/// Builds the retained row descriptors needed to render and interact with the agent list.
pub(in crate::hud) fn agent_rows(
    shell_rect: HudRect,
    scroll_offset: f32,
    hovered_agent: Option<AgentId>,
    agent_list_view: &AgentListView,
) -> Vec<AgentRow> {
    projected_agent_rows(
        shell_rect,
        scroll_offset,
        hovered_agent,
        agent_list_view,
        None,
    )
}

/// Builds the retained row descriptors, optionally projecting one row into a live drag preview.
pub(in crate::hud) fn projected_agent_rows(
    shell_rect: HudRect,
    scroll_offset: f32,
    hovered_agent: Option<AgentId>,
    agent_list_view: &AgentListView,
    drag_preview: Option<AgentListDragPreview>,
) -> Vec<AgentRow> {
    let content_x = shell_rect.x + AGENT_LIST_LEFT_RAIL_WIDTH + 1.0;
    let content_y = shell_rect.y + HUD_MODULE_PADDING + AGENT_LIST_HEADER_HEIGHT;
    let content_w = (shell_rect.w - AGENT_LIST_LEFT_RAIL_WIDTH - 3.0).max(0.0);
    let row_stride = agent_row_stride();

    let Some(drag_preview) = drag_preview.filter(|preview| {
        agent_list_view
            .rows
            .iter()
            .any(|row| row.agent_id == preview.agent_id)
    }) else {
        return agent_list_view
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| AgentRow {
                agent_id: row.agent_id,
                terminal_id: row.terminal_id,
                display_label: row.label.to_uppercase(),
                label: row.label.clone(),
                rect: HudRect {
                    x: content_x,
                    y: content_y + index as f32 * row_stride - scroll_offset,
                    w: content_w,
                    h: HUD_ROW_HEIGHT,
                },
                focused: row.focused,
                hovered: hovered_agent == Some(row.agent_id),
                has_tasks: row.has_tasks,
                interactive: row.interactive,
                status: row.status,
                dragging: false,
            })
            .collect();
    };

    let target_index = drag_preview
        .target_index
        .min(agent_list_view.rows.len().saturating_sub(1));
    let dragged_row = agent_list_view
        .rows
        .iter()
        .find(|row| row.agent_id == drag_preview.agent_id)
        .expect("validated drag preview should reference an existing row");
    let mut rows = Vec::with_capacity(agent_list_view.rows.len());

    for (index, row) in agent_list_view
        .rows
        .iter()
        .filter(|row| row.agent_id != drag_preview.agent_id)
        .enumerate()
    {
        let projected_index = if index < target_index {
            index
        } else {
            index + 1
        };
        rows.push(AgentRow {
            agent_id: row.agent_id,
            terminal_id: row.terminal_id,
            display_label: row.label.to_uppercase(),
            label: row.label.clone(),
            rect: HudRect {
                x: content_x,
                y: content_y + projected_index as f32 * row_stride - scroll_offset,
                w: content_w,
                h: HUD_ROW_HEIGHT,
            },
            focused: row.focused,
            hovered: hovered_agent == Some(row.agent_id),
            has_tasks: row.has_tasks,
            interactive: row.interactive,
            status: row.status,
            dragging: false,
        });
    }

    rows.push(AgentRow {
        agent_id: dragged_row.agent_id,
        terminal_id: dragged_row.terminal_id,
        display_label: dragged_row.label.to_uppercase(),
        label: dragged_row.label.clone(),
        rect: HudRect {
            x: content_x,
            y: drag_preview.cursor_y - drag_preview.grab_offset_y,
            w: content_w,
            h: HUD_ROW_HEIGHT,
        },
        focused: dragged_row.focused,
        hovered: hovered_agent == Some(dragged_row.agent_id),
        has_tasks: dragged_row.has_tasks,
        interactive: dragged_row.interactive,
        status: dragged_row.status,
        dragging: true,
    });

    rows
}
