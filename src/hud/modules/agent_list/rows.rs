use crate::{
    agents::AgentId,
    hud::view_models::{
        AgentListActivity, AgentListRowKey, AgentListRowKind, AgentListView, OwnedTmuxOwnerBinding,
    },
    terminals::TerminalId,
};

use super::super::super::state::{HudRect, HUD_MODULE_PADDING, HUD_ROW_HEIGHT};

pub(crate) const AGENT_LIST_HEADER_HEIGHT: f32 = 52.0;
pub(crate) const AGENT_LIST_LEFT_RAIL_WIDTH: f32 = 20.0;
const AGENT_LIST_ROW_MARKER_WIDTH: f32 = 12.0;
const AGENT_LIST_ROW_MARKER_GAP: f32 = 10.0;
const AGENT_LIST_ROW_GAP: f32 = 14.0;
const TMUX_CHILD_ROW_INDENT: f32 = 24.0;
const TMUX_CHILD_ROW_RIGHT_INSET: f32 = 4.0;
const TMUX_CHILD_ROW_HEIGHT: f32 = 18.0;
pub(crate) const AGENT_ROW_LABEL_TEXT_SIZE: f32 = 16.0;
pub(crate) const AGENT_ROW_LABEL_SCALE_X: f32 = 0.76;
pub(crate) const AGENT_ROW_LABEL_SCALE_Y: f32 = 1.14;
pub(crate) const TMUX_ROW_LABEL_TEXT_SIZE: f32 = 14.0;
pub(crate) const TMUX_ROW_LABEL_SCALE_X: f32 = 0.74;
pub(crate) const TMUX_ROW_LABEL_SCALE_Y: f32 = 1.04;
pub(crate) const AGENT_LIST_BORDER_ORANGE_R: u8 = 225;
pub(crate) const AGENT_LIST_BORDER_ORANGE_G: u8 = 129;
pub(crate) const AGENT_LIST_BORDER_ORANGE_B: u8 = 10;
pub(crate) const AGENT_LIST_BLOOM_RED_R: u8 = 143;
pub(crate) const AGENT_LIST_BLOOM_RED_G: u8 = 37;
pub(crate) const AGENT_LIST_BLOOM_RED_B: u8 = 15;
pub(crate) const AGENT_LIST_WORKING_GREEN_R: u8 = crate::shared::visual_contracts::WORKING_GREEN_R;
pub(crate) const AGENT_LIST_WORKING_GREEN_G: u8 = crate::shared::visual_contracts::WORKING_GREEN_G;
pub(crate) const AGENT_LIST_WORKING_GREEN_B: u8 = crate::shared::visual_contracts::WORKING_GREEN_B;
pub(crate) const AGENT_LIST_PAUSED_GRAY_R: u8 = 116;
pub(crate) const AGENT_LIST_PAUSED_GRAY_G: u8 = 118;
pub(crate) const AGENT_LIST_PAUSED_GRAY_B: u8 = 124;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AgentListRowSection {
    Main,
    Marker,
    Accent,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::hud) enum AgentRowKind {
    Agent {
        agent_id: AgentId,
        terminal_id: Option<TerminalId>,
        has_tasks: bool,
        interactive: bool,
        activity: AgentListActivity,
        paused: bool,
        aegis_enabled: bool,
        context_pct_milli: Option<i32>,
    },
    OwnedTmux {
        owner_agent_id: Option<AgentId>,
        orphaned: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::hud) struct AgentRow {
    pub(in crate::hud) key: AgentListRowKey,
    pub(in crate::hud) label: String,
    pub(in crate::hud) rect: HudRect,
    pub(in crate::hud) focused: bool,
    pub(in crate::hud) hovered: bool,
    pub(in crate::hud) dragging: bool,
    pub(in crate::hud) kind: AgentRowKind,
}

impl AgentRow {
    pub(in crate::hud) fn owner_agent_id(&self) -> Option<AgentId> {
        match self.kind {
            AgentRowKind::Agent { agent_id, .. } => Some(agent_id),
            AgentRowKind::OwnedTmux { owner_agent_id, .. } => owner_agent_id,
        }
    }

    #[cfg(test)]
    pub(in crate::hud) fn terminal_id(&self) -> Option<TerminalId> {
        match self.kind {
            AgentRowKind::Agent { terminal_id, .. } => terminal_id,
            AgentRowKind::OwnedTmux { .. } => None,
        }
    }

    #[cfg(test)]
    pub(in crate::hud) fn activity(&self) -> Option<AgentListActivity> {
        match self.kind {
            AgentRowKind::Agent { activity, .. } => Some(activity),
            AgentRowKind::OwnedTmux { .. } => None,
        }
    }

    #[cfg(test)]
    pub(in crate::hud) fn paused(&self) -> bool {
        match self.kind {
            AgentRowKind::Agent { paused, .. } => paused,
            AgentRowKind::OwnedTmux { .. } => false,
        }
    }

    pub(in crate::hud) fn aegis_enabled(&self) -> bool {
        match self.kind {
            AgentRowKind::Agent { aegis_enabled, .. } => aegis_enabled,
            AgentRowKind::OwnedTmux { .. } => false,
        }
    }

    #[cfg(test)]
    pub(in crate::hud) fn has_tasks(&self) -> bool {
        match self.kind {
            AgentRowKind::Agent { has_tasks, .. } => has_tasks,
            AgentRowKind::OwnedTmux { .. } => false,
        }
    }

    #[cfg(test)]
    pub(in crate::hud) fn is_orphan_tmux(&self) -> bool {
        matches!(self.kind, AgentRowKind::OwnedTmux { orphaned: true, .. })
    }

    pub(in crate::hud) fn is_tmux_child(&self) -> bool {
        matches!(self.kind, AgentRowKind::OwnedTmux { .. })
    }
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
    row.label.clone()
}

pub(crate) fn row_main_rect(row: &AgentRow) -> HudRect {
    if row.is_tmux_child() {
        let y = row.rect.y + ((row.rect.h - TMUX_CHILD_ROW_HEIGHT) * 0.5).max(0.0);
        HudRect {
            x: row.rect.x + TMUX_CHILD_ROW_INDENT,
            y,
            w: (row.rect.w - TMUX_CHILD_ROW_INDENT - TMUX_CHILD_ROW_RIGHT_INSET).max(24.0),
            h: TMUX_CHILD_ROW_HEIGHT,
        }
    } else {
        agent_row_rect(row.rect, AgentListRowSection::Main)
    }
}

pub(crate) fn agent_row_label_position(main_rect: HudRect, row: &AgentRow) -> bevy::prelude::Vec2 {
    bevy::prelude::Vec2::new(
        main_rect.x + 12.0,
        main_rect.y + if row.is_tmux_child() { 1.0 } else { 2.0 },
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
            let kind = match &row.kind {
                AgentListRowKind::Agent {
                    agent_id,
                    terminal_id,
                    has_tasks,
                    interactive,
                    activity,
                    paused,
                    aegis_enabled,
                    context_pct_milli,
                    ..
                } => AgentRowKind::Agent {
                    agent_id: *agent_id,
                    terminal_id: *terminal_id,
                    has_tasks: *has_tasks,
                    interactive: *interactive,
                    activity: *activity,
                    paused: *paused,
                    aegis_enabled: *aegis_enabled,
                    context_pct_milli: *context_pct_milli,
                },
                AgentListRowKind::OwnedTmux { owner, .. } => AgentRowKind::OwnedTmux {
                    owner_agent_id: match owner {
                        OwnedTmuxOwnerBinding::Bound(agent_id) => Some(*agent_id),
                        OwnedTmuxOwnerBinding::Orphan => None,
                    },
                    orphaned: matches!(owner, OwnedTmuxOwnerBinding::Orphan),
                },
            };
            AgentRow {
                key: row.key.clone(),
                label: row.label.clone(),
                rect: HudRect {
                    x: content_x,
                    y: content_y + index as f32 * row_stride - scroll_offset,
                    w: content_w,
                    h: HUD_ROW_HEIGHT,
                },
                focused: row.focused,
                hovered: hovered_row == Some(&row.key),
                dragging,
                kind,
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
