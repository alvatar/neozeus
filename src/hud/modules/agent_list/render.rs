use crate::hud::{
    render::{apply_alpha, HudPainter, HudRenderInputs},
    HudModuleModel, HudRect, HUD_MODULE_PADDING,
};
use bevy::prelude::Vec2;
use bevy_vello::{prelude::VelloTextAnchor, vello::peniko};

use super::{
    agent_row_rect, agent_rows, AgentListRowSection, AGENT_LIST_BLOOM_RED_B,
    AGENT_LIST_BLOOM_RED_G, AGENT_LIST_BLOOM_RED_R, AGENT_LIST_BORDER_ORANGE_B,
    AGENT_LIST_BORDER_ORANGE_G, AGENT_LIST_BORDER_ORANGE_R, AGENT_LIST_HEADER_HEIGHT,
    AGENT_LIST_LEFT_RAIL_WIDTH,
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
const EVA_ORANGE_DIM: peniko::Color = peniko::Color::from_rgba8(
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

#[allow(
    clippy::too_many_arguments,
    reason = "agent-list text helper needs position/color/anchor plus non-uniform scaling"
)]
// Draws label.
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

// Draws button rect.
fn draw_button_rect(
    painter: &mut HudPainter,
    rect: HudRect,
    stroke: peniko::Color,
    fill: peniko::Color,
) {
    painter.fill_rect(rect, fill, 0.0);
    painter.stroke_rect_width(rect, stroke, 2.5);
}

// Implements marker fill.
fn marker_fill(has_notes: bool) -> peniko::Color {
    if has_notes {
        TASK_RED
    } else {
        EVA_BLACK
    }
}

// Draws left rail.
fn draw_left_rail(painter: &mut HudPainter, content_rect: HudRect) {
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

// Renders content.
pub(crate) fn render_content(
    model: &HudModuleModel,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    let HudModuleModel::AgentList(state) = model else {
        return;
    };

    painter.fill_rect(content_rect, apply_alpha(EVA_BLACK, 0.98), 0.0);
    draw_left_rail(painter, content_rect);

    draw_label(
        painter,
        Vec2::new(
            content_rect.x + AGENT_LIST_LEFT_RAIL_WIDTH + HUD_MODULE_PADDING,
            content_rect.y + 10.0,
        ),
        "AGENT SUPPORT SYSTEM",
        18.0,
        EVA_ORANGE_BRIGHT,
        VelloTextAnchor::TopLeft,
        0.82,
        1.08,
    );
    draw_label(
        painter,
        Vec2::new(
            content_rect.x + content_rect.w - 14.0,
            content_rect.y + 12.0,
        ),
        "SEG.A",
        13.0,
        EVA_ORANGE_DIM,
        VelloTextAnchor::TopRight,
        0.88,
        1.04,
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

    for row in agent_rows(
        content_rect,
        state.scroll_offset,
        state.hovered_terminal,
        inputs.terminal_manager,
        inputs.focus_state,
        inputs.agent_directory,
    ) {
        if row.rect.y + row.rect.h < content_rect.y || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }

        let main_rect = agent_row_rect(row.rect, AgentListRowSection::Main);
        let marker_rect = agent_row_rect(row.rect, AgentListRowSection::Marker);
        let accent_rect = agent_row_rect(row.rect, AgentListRowSection::Accent);
        let stroke = if row.focused {
            EVA_SELECTED
        } else if row.hovered {
            EVA_ORANGE_BRIGHT
        } else {
            EVA_ORANGE
        };
        let fill = if row.focused {
            apply_alpha(EVA_BLACK, 0.94)
        } else if row.hovered {
            apply_alpha(EVA_BLACK, 0.92)
        } else {
            apply_alpha(EVA_BLACK, 0.90)
        };
        let has_notes = inputs
            .terminal_manager
            .get(row.terminal_id)
            .is_some_and(|terminal| inputs.notes_state.has_note_text(&terminal.session_name));

        draw_button_rect(painter, main_rect, stroke, fill);
        draw_button_rect(painter, marker_rect, stroke, marker_fill(has_notes));
        if row.focused {
            painter.fill_rect(accent_rect, EVA_EMISSIVE_RED, 0.0);
        }

        draw_label(
            painter,
            Vec2::new(main_rect.x + 12.0, main_rect.y + 2.0),
            &row.display_label,
            16.0,
            if row.focused {
                EVA_ORANGE_BRIGHT
            } else {
                EVA_ORANGE
            },
            VelloTextAnchor::TopLeft,
            0.76,
            1.14,
        );
    }
}
