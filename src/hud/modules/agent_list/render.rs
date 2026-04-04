use crate::agents::AgentStatus;

use super::super::super::render::{
    apply_alpha, interpolate_color, HudColors, HudPainter, HudRenderInputs,
};
use super::super::super::state::{AgentListUiState, HudRect, HUD_MODULE_PADDING};
use bevy::prelude::Vec2;
use bevy_vello::{prelude::VelloTextAnchor, vello::peniko};

use super::{
    agent_row_label_position, agent_row_label_text, agent_row_rect, projected_agent_rows,
    row_main_rect, AgentListDragPreview, AgentListRowSection, AGENT_LIST_BLOOM_RED_B,
    AGENT_LIST_BLOOM_RED_G, AGENT_LIST_BLOOM_RED_R, AGENT_LIST_BORDER_ORANGE_B,
    AGENT_LIST_BORDER_ORANGE_G, AGENT_LIST_BORDER_ORANGE_R, AGENT_LIST_HEADER_HEIGHT,
    AGENT_LIST_LEFT_RAIL_WIDTH, AGENT_LIST_WORKING_GREEN_B, AGENT_LIST_WORKING_GREEN_G,
    AGENT_LIST_WORKING_GREEN_R, AGENT_ROW_LABEL_SCALE_X, AGENT_ROW_LABEL_SCALE_Y,
    AGENT_ROW_LABEL_TEXT_SIZE, TMUX_ROW_LABEL_SCALE_X, TMUX_ROW_LABEL_SCALE_Y,
    TMUX_ROW_LABEL_TEXT_SIZE,
};
use super::rows::{AgentRow, AgentRowKind};

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
const TMUX_CHILD_ORANGE: peniko::Color = peniko::Color::from_rgba8(244, 172, 72, 255);
const TMUX_CHILD_ORANGE_BRIGHT: peniko::Color = peniko::Color::from_rgba8(255, 196, 106, 255);
const WORKING_ROW_COLOR: peniko::Color = peniko::Color::from_rgba8(
    AGENT_LIST_WORKING_GREEN_R,
    AGENT_LIST_WORKING_GREEN_G,
    AGENT_LIST_WORKING_GREEN_B,
    255,
);
const CONTEXT_BAR_TRACK_COLOR: peniko::Color = HudColors::BUTTON;

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

fn context_track_rect(main_rect: HudRect, marker_rect: HudRect) -> HudRect {
    let gap_left = main_rect.x + main_rect.w;
    let gap_right = marker_rect.x;
    HudRect {
        x: gap_left + 1.0,
        y: main_rect.y + 1.0,
        w: (gap_right - gap_left - 2.0).max(6.0),
        h: (main_rect.h - 2.0).max(12.0),
    }
}

fn context_segment_count(track_rect: HudRect) -> usize {
    let count = ((track_rect.h / 4.0).floor() as usize).clamp(1, 63);
    if count <= 1 {
        1
    } else if count.is_multiple_of(2) {
        count - 1
    } else {
        count
    }
}

fn context_segment_rect(
    track_rect: HudRect,
    segment_index: usize,
    segment_count: usize,
) -> HudRect {
    let slot_h = track_rect.h / segment_count as f32;
    let y0 = track_rect.y + slot_h * segment_index as f32;
    let y1 = if segment_index + 1 == segment_count {
        track_rect.y + track_rect.h
    } else {
        track_rect.y + slot_h * (segment_index + 1) as f32
    };
    HudRect {
        x: track_rect.x,
        y: y0,
        w: track_rect.w,
        h: (y1 - y0).max(1.0),
    }
}

fn context_active_segment_range(
    segment_count: usize,
    pct_milli: i32,
) -> std::ops::RangeInclusive<usize> {
    let clamped = pct_milli.clamp(0, 100_000) as f32 / 100_000.0;
    let center = segment_count / 2;
    let max_radius = segment_count / 2;
    let radius = (clamped * max_radius as f32).round() as usize;
    (center - radius)..=(center + radius)
}

fn context_bar_color(pct_milli: i32) -> peniko::Color {
    let clamped = pct_milli.clamp(0, 100_000) as f32 / 100_000.0;
    let low = peniko::Color::from_rgba8(216, 160, 96, 255);
    let mid = peniko::Color::from_rgba8(255, 148, 64, 255);
    let high = peniko::Color::from_rgba8(255, 36, 28, 255);
    if clamped < 0.60 {
        interpolate_color(low, mid, clamped / 0.60)
    } else {
        interpolate_color(mid, high, (clamped - 0.60) / 0.40)
    }
}

fn rendered_context_pct_milli(context_pct_milli: Option<i32>) -> i32 {
    context_pct_milli.unwrap_or(0)
}

fn tmux_child_chrome_color(focused: bool) -> peniko::Color {
    if focused {
        TMUX_CHILD_ORANGE_BRIGHT
    } else {
        TMUX_CHILD_ORANGE
    }
}

fn tmux_child_fill_color() -> peniko::Color {
    apply_alpha(EVA_BLACK, 0.90)
}

fn tmux_child_label_color(focused: bool) -> peniko::Color {
    tmux_child_chrome_color(focused)
}

fn tmux_child_connector(parent_main_rect: HudRect, child_main_rect: HudRect) -> (Vec2, Vec2) {
    (
        Vec2::new(parent_main_rect.x, parent_main_rect.y + parent_main_rect.h),
        Vec2::new(child_main_rect.x, child_main_rect.y),
    )
}

fn draw_agent_row_text_selection(painter: &mut HudPainter, position: Vec2, text: &str) {
    let text_width = painter.text_size(text, AGENT_ROW_LABEL_TEXT_SIZE).x * AGENT_ROW_LABEL_SCALE_X;
    painter.fill_rect(
        HudRect {
            x: position.x - 1.0,
            y: position.y + 1.0,
            w: text_width.max(6.0) + 2.0,
            h: 18.0,
        },
        apply_alpha(EVA_CYAN, 0.24),
        0.0,
    );
}

fn draw_context_bar(
    painter: &mut HudPainter,
    main_rect: HudRect,
    marker_rect: HudRect,
    pct_milli: i32,
) {
    let track_rect = context_track_rect(main_rect, marker_rect);
    let segment_count = context_segment_count(track_rect);
    let active_range = context_active_segment_range(segment_count, pct_milli);
    let fill_color = context_bar_color(pct_milli);
    painter.fill_rect(track_rect, CONTEXT_BAR_TRACK_COLOR, 0.0);

    for segment_index in 0..segment_count {
        if active_range.contains(&segment_index) {
            painter.fill_rect(
                context_segment_rect(track_rect, segment_index, segment_count),
                fill_color,
                0.0,
            );
        }
    }

    for stripe_index in 1..segment_count {
        let y = track_rect.y + stripe_index as f32 * (track_rect.h / segment_count as f32);
        painter.fill_rect(
            HudRect {
                x: track_rect.x,
                y,
                w: track_rect.w,
                h: 1.0,
            },
            peniko::Color::from_rgba8(46, 43, 39, 255),
            0.0,
        );
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

fn render_agent_row(
    painter: &mut HudPainter,
    row: &AgentRow,
    main_rect: HudRect,
    status: AgentStatus,
    has_tasks: bool,
    interactive: bool,
    context_pct_milli: Option<i32>,
) {
    let marker_rect = agent_row_rect(row.rect, AgentListRowSection::Marker);
    let accent_rect = agent_row_rect(row.rect, AgentListRowSection::Accent);
    let stroke = agent_row_stroke(status, row.focused, row.hovered, row.dragging);
    let fill = agent_fill_color(status, row.focused, row.hovered, row.dragging);

    draw_button_rect(painter, main_rect, stroke, fill);
    draw_button_rect(
        painter,
        marker_rect,
        stroke,
        marker_fill(status, has_tasks, interactive),
    );
    draw_context_bar(
        painter,
        main_rect,
        marker_rect,
        rendered_context_pct_milli(context_pct_milli),
    );
    if let Some(accent_fill) = agent_accent_color(status, row.focused, row.dragging) {
        painter.fill_rect(accent_rect, accent_fill, 0.0);
    }
}

fn render_tmux_child_row(
    painter: &mut HudPainter,
    row: &AgentRow,
    main_rect: HudRect,
    owner_agent_id: Option<crate::agents::AgentId>,
    orphaned: bool,
    parent_main_rects: &std::collections::HashMap<crate::agents::AgentId, HudRect>,
) {
    if let Some(parent_main_rect) = owner_agent_id.and_then(|agent_id| parent_main_rects.get(&agent_id).copied()) {
        let (start, end) = tmux_child_connector(parent_main_rect, main_rect);
        painter.stroke_line(
            start,
            end,
            apply_alpha(tmux_child_chrome_color(row.focused), 0.72),
            2.5,
        );
    }
    let stroke = if orphaned {
        EVA_EMISSIVE_RED
    } else {
        tmux_child_chrome_color(row.focused)
    };
    draw_button_rect(painter, main_rect, stroke, tmux_child_fill_color());
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
        "NEOZEUS CONTROL // 0.1",
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
        state.hovered_row.as_ref(),
        inputs.agent_list_view,
        drag_preview,
    );
    rows.sort_by_key(|row| row.dragging);

    let mut parent_main_rects = std::collections::HashMap::new();
    for row in rows {
        if row.rect.y + row.rect.h < content_rect.y || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }

        let main_rect = row_main_rect(&row);
        match &row.kind {
            AgentRowKind::Agent {
                agent_id,
                has_tasks,
                interactive,
                status,
                context_pct_milli,
                ..
            } => {
                parent_main_rects.insert(*agent_id, main_rect);
                render_agent_row(
                    painter,
                    &row,
                    main_rect,
                    *status,
                    *has_tasks,
                    *interactive,
                    *context_pct_milli,
                );
            }
            AgentRowKind::OwnedTmux {
                owner_agent_id,
                orphaned,
                ..
            } => {
                render_tmux_child_row(
                    painter,
                    &row,
                    main_rect,
                    *owner_agent_id,
                    *orphaned,
                    &parent_main_rects,
                );
            }
        }

        let label_text = agent_row_label_text(&row);
        let label_position = agent_row_label_position(main_rect, &row);
        let selected_agent_list_text =
            inputs
                .agent_list_text_selection
                .selection()
                .map(|selection| {
                    let start = inputs
                        .agent_list_view
                        .rows
                        .iter()
                        .position(|candidate| candidate.key == selection.anchor_row)
                        .unwrap_or(usize::MAX);
                    let end = inputs
                        .agent_list_view
                        .rows
                        .iter()
                        .position(|candidate| candidate.key == selection.focus_row)
                        .unwrap_or(usize::MAX);
                    let (start, end) = if start <= end {
                        (start, end)
                    } else {
                        (end, start)
                    };
                    (start, end)
                });
        if selected_agent_list_text.is_some_and(|(start, end)| {
            inputs
                .agent_list_view
                .rows
                .iter()
                .position(|candidate| candidate.key == row.key)
                .is_some_and(|index| index >= start && index <= end)
        }) {
            draw_agent_row_text_selection(painter, label_position, &label_text);
        }
        draw_label(
            painter,
            label_position,
            &label_text,
            if row.is_tmux_child() {
                TMUX_ROW_LABEL_TEXT_SIZE
            } else {
                AGENT_ROW_LABEL_TEXT_SIZE
            },
            match &row.kind {
                AgentRowKind::Agent { status, .. } => {
                    agent_label_color(*status, row.focused, row.dragging)
                }
                AgentRowKind::OwnedTmux { orphaned, .. } => {
                    if *orphaned {
                        EVA_EMISSIVE_RED
                    } else {
                        tmux_child_label_color(row.focused)
                    }
                }
            },
            VelloTextAnchor::TopLeft,
            if row.is_tmux_child() {
                TMUX_ROW_LABEL_SCALE_X
            } else {
                AGENT_ROW_LABEL_SCALE_X
            },
            if row.is_tmux_child() {
                TMUX_ROW_LABEL_SCALE_Y
            } else {
                AGENT_ROW_LABEL_SCALE_Y
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        agent_accent_color, agent_fill_color, agent_label_color, agent_row_stroke,
        context_active_segment_range, context_bar_color, context_segment_count,
        context_segment_rect, context_track_rect, marker_fill, rendered_context_pct_milli,
        tmux_child_chrome_color, tmux_child_connector, tmux_child_fill_color,
        tmux_child_label_color, EVA_CYAN, TMUX_CHILD_ORANGE, TMUX_CHILD_ORANGE_BRIGHT,
        WORKING_ROW_COLOR,
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

    #[test]
    fn context_bar_expands_from_center_to_borders() {
        let track = context_track_rect(
            crate::hud::HudRect {
                x: 10.0,
                y: 40.0,
                w: 80.0,
                h: 30.0,
            },
            crate::hud::HudRect {
                x: 100.0,
                y: 40.0,
                w: 12.0,
                h: 30.0,
            },
        );
        let segment_count = context_segment_count(track);
        let zero = context_active_segment_range(segment_count, 0);
        let full = context_active_segment_range(segment_count, 100_000);

        assert_eq!(segment_count % 2, 1);
        assert_eq!(zero.start(), zero.end());
        assert_eq!(*zero.start(), segment_count / 2);
        assert_eq!(*full.start(), 0);
        assert_eq!(*full.end(), segment_count - 1);

        let top = context_segment_rect(track, 0, segment_count);
        let bottom = context_segment_rect(track, segment_count - 1, segment_count);
        assert_eq!(top.y, track.y);
        assert_eq!(bottom.y + bottom.h, track.y + track.h);
    }

    #[test]
    fn context_bar_reaches_hot_red_at_maximum() {
        let low = context_bar_color(0).to_rgba8();
        let high = context_bar_color(100_000).to_rgba8();

        assert!(high.r >= low.r);
        assert!(high.g < low.g);
        assert!(high.b < low.b);
    }

    #[test]
    fn missing_context_renders_as_zero_percent() {
        assert_eq!(rendered_context_pct_milli(None), 0);
        assert_eq!(rendered_context_pct_milli(Some(17_000)), 17_000);
    }

    #[test]
    fn tmux_child_rows_only_have_idle_and_selected_colors() {
        assert_eq!(tmux_child_chrome_color(false), TMUX_CHILD_ORANGE);
        assert_eq!(tmux_child_chrome_color(true), TMUX_CHILD_ORANGE_BRIGHT);
        assert_eq!(tmux_child_label_color(false), TMUX_CHILD_ORANGE);
        assert_eq!(tmux_child_label_color(true), TMUX_CHILD_ORANGE_BRIGHT);
        assert_eq!(
            tmux_child_fill_color(),
            super::apply_alpha(super::EVA_BLACK, 0.90)
        );
    }

    #[test]
    fn tmux_child_connector_runs_from_parent_bottom_left_to_child_top_left() {
        let parent = crate::hud::HudRect {
            x: 30.0,
            y: 50.0,
            w: 120.0,
            h: 24.0,
        };
        let child = crate::hud::HudRect {
            x: 58.0,
            y: 96.0,
            w: 100.0,
            h: 18.0,
        };

        let (start, end) = tmux_child_connector(parent, child);
        assert_eq!(start, bevy::prelude::Vec2::new(30.0, 74.0));
        assert_eq!(end, bevy::prelude::Vec2::new(58.0, 96.0));
    }
}
