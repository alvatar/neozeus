use crate::agents::AgentStatus;

use super::super::super::render::{apply_alpha, HudPainter, HudRenderInputs};
use super::super::super::state::{AgentListUiState, HudRect, HUD_MODULE_PADDING};
use bevy::prelude::Vec2;
use bevy_vello::{prelude::VelloTextAnchor, vello::peniko};

use super::{
    agent_row_rect, projected_agent_rows, AgentListDragPreview, AgentListRowSection,
    AGENT_LIST_BLOOM_RED_B, AGENT_LIST_BLOOM_RED_G, AGENT_LIST_BLOOM_RED_R,
    AGENT_LIST_BORDER_ORANGE_B, AGENT_LIST_BORDER_ORANGE_G, AGENT_LIST_BORDER_ORANGE_R,
    AGENT_LIST_HEADER_HEIGHT, AGENT_LIST_LEFT_RAIL_WIDTH, AGENT_LIST_WORKING_GREEN_B,
    AGENT_LIST_WORKING_GREEN_G, AGENT_LIST_WORKING_GREEN_R,
};

const EVA_ORANGE: peniko::Color = peniko::Color::from_rgba8(
    AGENT_LIST_BORDER_ORANGE_R,
    AGENT_LIST_BORDER_ORANGE_G,
    AGENT_LIST_BORDER_ORANGE_B,
    255,
);
const EVA_ORANGE_BRIGHT: peniko::Color = peniko::Color::from_rgba8(
    AGENT_LIST_BORDER_ORANGE_R,
    AGENT_LIST_BORDER_ORANGE_G,
    AGENT_LIST_BORDER_ORANGE_B,
    255,
);
const EVA_SELECTED: peniko::Color = peniko::Color::from_rgba8(
    AGENT_LIST_BORDER_ORANGE_R,
    AGENT_LIST_BORDER_ORANGE_G,
    AGENT_LIST_BORDER_ORANGE_B,
    255,
);
const EVA_CYAN: peniko::Color = peniko::Color::from_rgba8(96, 238, 255, 255);
const EVA_BLACK: peniko::Color = peniko::Color::from_rgba8(0, 0, 0, 255);
const EVA_EMISSIVE_RED: peniko::Color = peniko::Color::from_rgba8(
    AGENT_LIST_BLOOM_RED_R,
    AGENT_LIST_BLOOM_RED_G,
    AGENT_LIST_BLOOM_RED_B,
    255,
);
const TASK_RED: peniko::Color = peniko::Color::from_rgba8(255, 24, 24, 255);
const DISCONNECTED_RED: peniko::Color = peniko::Color::from_rgba8(160, 34, 24, 255);
const WORKING_ROW_COLOR: peniko::Color = peniko::Color::from_rgba8(
    AGENT_LIST_WORKING_GREEN_R,
    AGENT_LIST_WORKING_GREEN_G,
    AGENT_LIST_WORKING_GREEN_B,
    255,
);

#[allow(
    clippy::too_many_arguments,
    reason = "agent-list text helper needs position/color/anchor plus non-uniform scaling"
)]
/// Draws label.
fn draw_label(
    painter: &mut HudPainter,
    position: Vec2,
    text: &str,
    size: f32,
    color: peniko::Color,
    anchor: VelloTextAnchor,
    scale_x: f32,
    scale_y: f32,
) {
    painter.label_scaled(position, text, size, color, anchor, scale_x, scale_y);
}

/// Draws button rect.
fn draw_button_rect(
    painter: &mut HudPainter,
    rect: HudRect,
    stroke: peniko::Color,
    fill: peniko::Color,
) {
    painter.fill_rect(rect, fill, 0.0);
    painter.stroke_rect_width(rect, stroke, 2.5);
}

fn agent_fill_color(
    status: AgentStatus,
    focused: bool,
    hovered: bool,
    dragging: bool,
) -> peniko::Color {
    if dragging {
        apply_alpha(EVA_BLACK, 0.98)
    } else if status == AgentStatus::Working {
        apply_alpha(WORKING_ROW_COLOR, 0.22)
    } else if focused {
        apply_alpha(EVA_BLACK, 0.94)
    } else if hovered {
        apply_alpha(EVA_BLACK, 0.92)
    } else {
        apply_alpha(EVA_BLACK, 0.90)
    }
}

fn agent_accent_color(status: AgentStatus, focused: bool, dragging: bool) -> Option<peniko::Color> {
    if dragging {
        Some(EVA_EMISSIVE_RED)
    } else if status == AgentStatus::Working {
        Some(WORKING_ROW_COLOR)
    } else if focused {
        Some(EVA_EMISSIVE_RED)
    } else {
        None
    }
}

/// Handles marker fill.
fn marker_fill(status: AgentStatus, has_tasks: bool, interactive: bool) -> peniko::Color {
    if !interactive {
        DISCONNECTED_RED
    } else if status == AgentStatus::Working {
        WORKING_ROW_COLOR
    } else if has_tasks {
        TASK_RED
    } else {
        EVA_BLACK
    }
}

/// Draws left rail.
fn draw_left_rail(painter: &mut HudPainter, content_rect: HudRect) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    let tick_x = content_rect.x + 5.0;
    let top = content_rect.y + HUD_MODULE_PADDING + 4.0;
    let bottom = content_rect.y + content_rect.h - HUD_MODULE_PADDING;
    let major_step = 34.0;
    let minor_offset = major_step * 0.5;

    let mut y = top + 18.0;
    while y <= bottom - 2.0 {
        painter.fill_rect(
            HudRect {
                x: tick_x,
                y,
                w: 8.0,
                h: 2.0,
            },
            apply_alpha(EVA_CYAN, 0.82),
            0.0,
        );

        let minor_y = y + minor_offset;
        if minor_y <= bottom - 2.0 {
            painter.fill_rect(
                HudRect {
                    x: tick_x,
                    y: minor_y,
                    w: 4.0,
                    h: 2.0,
                },
                apply_alpha(EVA_CYAN, 0.72),
                0.0,
            );
        }

        y += major_step;
    }
}

fn agent_label_color(status: AgentStatus, focused: bool, dragging: bool) -> peniko::Color {
    if dragging {
        EVA_CYAN
    } else if status == AgentStatus::Working {
        WORKING_ROW_COLOR
    } else if focused {
        EVA_ORANGE_BRIGHT
    } else {
        EVA_ORANGE
    }
}

fn agent_row_stroke(
    status: AgentStatus,
    focused: bool,
    hovered: bool,
    dragging: bool,
) -> peniko::Color {
    if dragging {
        EVA_CYAN
    } else if status == AgentStatus::Working {
        WORKING_ROW_COLOR
    } else if focused {
        EVA_SELECTED
    } else if hovered {
        EVA_ORANGE_BRIGHT
    } else {
        EVA_ORANGE
    }
}

/// Renders content.
pub(crate) fn render_content(
    state: &AgentListUiState,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    painter.fill_rect(content_rect, apply_alpha(EVA_BLACK, 0.98), 0.0);
    draw_left_rail(painter, content_rect);

    draw_label(
        painter,
        Vec2::new(
            content_rect.x + AGENT_LIST_LEFT_RAIL_WIDTH + HUD_MODULE_PADDING,
            content_rect.y + 10.0,
        ),
        "NEOZEUS CONTROL ROOM // 0.1",
        18.0,
        EVA_ORANGE_BRIGHT,
        VelloTextAnchor::TopLeft,
        0.82,
        1.08,
    );
    painter.fill_rect(
        HudRect {
            x: content_rect.x + AGENT_LIST_LEFT_RAIL_WIDTH + HUD_MODULE_PADDING,
            y: content_rect.y + AGENT_LIST_HEADER_HEIGHT - 8.0,
            w: (content_rect.w - AGENT_LIST_LEFT_RAIL_WIDTH - HUD_MODULE_PADDING * 2.0).max(0.0),
            h: 2.0,
        },
        apply_alpha(EVA_CYAN, 0.8),
        0.0,
    );

    let drag_preview = match (
        state.drag.dragging_agent,
        state.drag.drag_cursor,
        state.drag.last_reorder_index,
    ) {
        (Some(agent_id), Some(cursor), Some(target_index)) => Some(AgentListDragPreview {
            agent_id,
            cursor_y: cursor.y,
            grab_offset_y: state.drag.drag_grab_offset_y,
            target_index,
        }),
        _ => None,
    };

    let mut rows = projected_agent_rows(
        content_rect,
        state.scroll_offset,
        state.hovered_agent,
        inputs.agent_list_view,
        drag_preview,
    );
    rows.sort_by_key(|row| row.dragging);

    for row in rows {
        if row.rect.y + row.rect.h < content_rect.y || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }

        let main_rect = agent_row_rect(row.rect, AgentListRowSection::Main);
        let marker_rect = agent_row_rect(row.rect, AgentListRowSection::Marker);
        let accent_rect = agent_row_rect(row.rect, AgentListRowSection::Accent);
        let stroke = agent_row_stroke(row.status, row.focused, row.hovered, row.dragging);
        let fill = agent_fill_color(row.status, row.focused, row.hovered, row.dragging);

        draw_button_rect(painter, main_rect, stroke, fill);
        draw_button_rect(
            painter,
            marker_rect,
            stroke,
            marker_fill(row.status, row.has_tasks, row.interactive),
        );
        if let Some(accent_fill) = agent_accent_color(row.status, row.focused, row.dragging) {
            painter.fill_rect(accent_rect, accent_fill, 0.0);
        }

        draw_label(
            painter,
            Vec2::new(main_rect.x + 12.0, main_rect.y + 2.0),
            &row.display_label,
            16.0,
            agent_label_color(row.status, row.focused, row.dragging),
            VelloTextAnchor::TopLeft,
            0.76,
            1.14,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        agent_accent_color, agent_fill_color, agent_label_color, agent_row_stroke, marker_fill,
        EVA_CYAN, WORKING_ROW_COLOR,
    };
    use crate::agents::AgentStatus;

    #[test]
    fn working_agent_rows_use_green_palette() {
        assert_eq!(
            agent_label_color(AgentStatus::Working, false, false),
            WORKING_ROW_COLOR
        );
        assert_eq!(
            agent_row_stroke(AgentStatus::Working, false, false, false),
            WORKING_ROW_COLOR
        );
        assert_eq!(
            agent_fill_color(AgentStatus::Working, false, false, false),
            super::apply_alpha(WORKING_ROW_COLOR, 0.22)
        );
        assert_eq!(
            marker_fill(AgentStatus::Working, false, true),
            WORKING_ROW_COLOR
        );
        assert_eq!(
            agent_accent_color(AgentStatus::Working, false, false),
            Some(WORKING_ROW_COLOR)
        );
    }

    #[test]
    fn dragging_still_overrides_working_green() {
        assert_eq!(
            agent_label_color(AgentStatus::Working, false, true),
            EVA_CYAN
        );
        assert_eq!(
            agent_row_stroke(AgentStatus::Working, true, true, true),
            EVA_CYAN
        );
    }
}
