use crate::{
    agents::{AgentId, AgentStatus},
    hud::view_models::{AgentListRowKey, AgentListRowKind, AgentListView, OwnedTmuxOwnerBinding},
    terminals::TerminalId,
};

use super::super::super::state::{HudRect, HUD_MODULE_PADDING, HUD_ROW_HEIGHT};

pub(crate) const AGENT_LIST_HEADER_HEIGHT: f32 = 52.0;
pub(crate) const AGENT_LIST_LEFT_RAIL_WIDTH: f32 = 20.0;
const AGENT_LIST_ROW_MARKER_WIDTH: f32 = 12.0;
const AGENT_LIST_ROW_MARKER_GAP: f32 = 10.0;
const AGENT_LIST_ROW_GAP: f32 = 14.0;
pub(crate) const AGENT_ROW_LABEL_TEXT_SIZE: f32 = 16.0;
pub(crate) const AGENT_ROW_LABEL_SCALE_X: f32 = 0.76;
pub(crate) const AGENT_ROW_LABEL_SCALE_Y: f32 = 1.14;
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
    pub(in crate::hud) key: AgentListRowKey,
    pub(in crate::hud) agent_id: Option<AgentId>,
    pub(in crate::hud) terminal_id: Option<TerminalId>,
    pub(in crate::hud) label: String,
    pub(in crate::hud) detail: Option<String>,
    pub(in crate::hud) rect: HudRect,
    pub(in crate::hud) focused: bool,
    pub(in crate::hud) hovered: bool,
    pub(in crate::hud) has_tasks: bool,
    pub(in crate::hud) interactive: bool,
    pub(in crate::hud) status: AgentStatus,
    pub(in crate::hud) context_pct_milli: Option<i32>,
    pub(in crate::hud) dragging: bool,
    pub(in crate::hud) is_tmux_child: bool,
    pub(in crate::hud) is_orphan_tmux: bool,
    pub(in crate::hud) tmux_attached: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::hud) struct AgentListDragPreview {
    pub(in crate::hud) agent_id: AgentId,
    pub(in crate::hud) cursor_y: f32,
    pub(in crate::hud) grab_offset_y: f32,
    pub(in crate::hud) target_index: usize,
}

/// Derives one sub-rectangle of an agent row for rendering or hit-testing.
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

pub(crate) fn agent_row_label_text(row: &AgentRow) -> String {
    if row.is_tmux_child {
        format!("↳ {}", row.label)
    } else {
        row.label.clone()
    }
}

pub(crate) fn agent_row_label_position(main_rect: HudRect, row: &AgentRow) -> bevy::prelude::Vec2 {
    bevy::prelude::Vec2::new(
        main_rect.x + if row.is_tmux_child { 18.0 } else { 12.0 },
        main_rect.y + 2.0,
    )
}

pub(crate) fn agent_row_text_hit_rect(main_rect: HudRect) -> HudRect {
    HudRect {
        x: main_rect.x + 10.0,
        y: main_rect.y + 1.0,
        w: (main_rect.w * 0.62).max(64.0),
        h: (main_rect.h - 2.0).max(10.0),
    }
}

fn agent_row_stride() -> f32 {
    HUD_ROW_HEIGHT + AGENT_LIST_ROW_GAP
}

pub(crate) fn agent_list_content_height(row_count: usize) -> f32 {
    match row_count {
        0 => 0.0,
        _ => row_count as f32 * agent_row_stride() - AGENT_LIST_ROW_GAP,
    }
}

pub(in crate::hud) fn agent_rows(
    shell_rect: HudRect,
    scroll_offset: f32,
    hovered_row: Option<&AgentListRowKey>,
    agent_list_view: &AgentListView,
) -> Vec<AgentRow> {
    projected_agent_rows(
        shell_rect,
        scroll_offset,
        hovered_row,
        agent_list_view,
        None,
    )
}

pub(in crate::hud) fn projected_agent_rows(
    shell_rect: HudRect,
    scroll_offset: f32,
    hovered_row: Option<&AgentListRowKey>,
    agent_list_view: &AgentListView,
    drag_preview: Option<AgentListDragPreview>,
) -> Vec<AgentRow> {
    let content_x = shell_rect.x + AGENT_LIST_LEFT_RAIL_WIDTH + 1.0;
    let content_y = shell_rect.y + HUD_MODULE_PADDING + AGENT_LIST_HEADER_HEIGHT;
    let content_w = (shell_rect.w - AGENT_LIST_LEFT_RAIL_WIDTH - 3.0).max(0.0);
    let row_stride = agent_row_stride();

    let build_row =
        |index: usize, row: &crate::hud::view_models::AgentListRowView, dragging: bool| {
            let (
                agent_id,
                terminal_id,
                detail,
                has_tasks,
                interactive,
                status,
                context_pct_milli,
                is_tmux_child,
                is_orphan_tmux,
                tmux_attached,
            ) = match &row.kind {
                AgentListRowKind::Agent {
                    agent_id,
                    terminal_id,
                    has_tasks,
                    interactive,
                    status,
                    context_pct_milli,
                } => (
                    Some(*agent_id),
                    *terminal_id,
                    None,
                    *has_tasks,
                    *interactive,
                    *status,
                    *context_pct_milli,
                    false,
                    false,
                    false,
                ),
                AgentListRowKind::OwnedTmux {
                    owner,
                    tmux_name,
                    cwd,
                    attached,
                    ..
                } => (
                    match owner {
                        OwnedTmuxOwnerBinding::Bound(agent_id) => Some(*agent_id),
                        OwnedTmuxOwnerBinding::Orphan => None,
                    },
                    None,
                    Some(format!("{}  {}", tmux_name, cwd)),
                    false,
                    true,
                    AgentStatus::Unknown,
                    None,
                    true,
                    matches!(owner, OwnedTmuxOwnerBinding::Orphan),
                    *attached,
                ),
            };
            AgentRow {
                key: row.key.clone(),
                agent_id,
                terminal_id,
                label: row.label.clone(),
                detail,
                rect: HudRect {
                    x: content_x,
                    y: content_y + index as f32 * row_stride - scroll_offset,
                    w: content_w,
                    h: HUD_ROW_HEIGHT,
                },
                focused: row.focused,
                hovered: hovered_row == Some(&row.key),
                has_tasks,
                interactive,
                status,
                context_pct_milli,
                dragging,
                is_tmux_child,
                is_orphan_tmux,
                tmux_attached,
            }
        };

    let Some(drag_preview) = drag_preview.filter(|preview| {
        agent_list_view.rows.iter().any(|row| {
            matches!(
                row.kind,
                AgentListRowKind::Agent { agent_id, .. } if agent_id == preview.agent_id
            )
        })
    }) else {
        return agent_list_view
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| build_row(index, row, false))
            .collect();
    };

    let agent_rows_only = agent_list_view
        .rows
        .iter()
        .filter(|row| matches!(row.kind, AgentListRowKind::Agent { .. }))
        .collect::<Vec<_>>();
    let target_index = drag_preview
        .target_index
        .min(agent_rows_only.len().saturating_sub(1));
    let dragged_row = agent_rows_only
        .into_iter()
        .find(|row| {
            matches!(
                row.kind,
                AgentListRowKind::Agent { agent_id, .. } if agent_id == drag_preview.agent_id
            )
        })
        .expect("validated drag preview should reference an existing agent row");

    let mut rows = Vec::with_capacity(agent_list_view.rows.len());
    let mut projected_agent_index = 0usize;
    for row in agent_list_view.rows.iter() {
        if matches!(
            row.kind,
            AgentListRowKind::Agent { agent_id, .. } if agent_id == drag_preview.agent_id
        ) {
            continue;
        }
        let row_index = match row.kind {
            AgentListRowKind::Agent { .. } => {
                let index = if projected_agent_index < target_index {
                    projected_agent_index
                } else {
                    projected_agent_index + 1
                };
                projected_agent_index += 1;
                index
            }
            AgentListRowKind::OwnedTmux { .. } => projected_agent_index,
        };
        rows.push(build_row(row_index, row, false));
    }

    rows.push(AgentRow {
        rect: HudRect {
            x: content_x,
            y: drag_preview.cursor_y - drag_preview.grab_offset_y,
            w: content_w,
            h: HUD_ROW_HEIGHT,
        },
        dragging: true,
        ..build_row(target_index, dragged_row, true)
    });

    rows
}
