use crate::hud::{
    render::{apply_alpha, HudColors, HudPainter, HudRenderInputs},
    HudModuleModel, HudRect, HUD_MODULE_PADDING,
};
use bevy::prelude::Vec2;
use bevy_vello::{prelude::VelloTextAnchor, vello::peniko};

use super::{
    agent_rows, AGENT_LIST_HEADER_HEIGHT, AGENT_LIST_LEFT_RAIL_WIDTH, AGENT_LIST_ROW_MARKER_GAP,
    AGENT_LIST_ROW_MARKER_WIDTH,
};

const EVA_ORANGE: peniko::Color = peniko::Color::from_rgba8(255, 120, 12, 255);
const EVA_ORANGE_BRIGHT: peniko::Color = peniko::Color::from_rgba8(255, 146, 26, 255);
const EVA_ORANGE_DIM: peniko::Color = peniko::Color::from_rgba8(222, 92, 14, 255);
const EVA_CYAN: peniko::Color = peniko::Color::from_rgba8(96, 238, 255, 255);
const EVA_BLACK: peniko::Color = peniko::Color::from_rgba8(0, 0, 0, 255);

fn inflate_rect(rect: HudRect, amount: f32) -> HudRect {
    HudRect {
        x: rect.x - amount,
        y: rect.y - amount,
        w: rect.w + amount * 2.0,
        h: rect.h + amount * 2.0,
    }
}

fn row_main_rect(rect: HudRect) -> HudRect {
    HudRect {
        x: rect.x,
        y: rect.y + 2.0,
        w: (rect.w - AGENT_LIST_ROW_MARKER_WIDTH - AGENT_LIST_ROW_MARKER_GAP).max(12.0),
        h: (rect.h - 4.0).max(10.0),
    }
}

fn row_marker_rect(rect: HudRect) -> HudRect {
    HudRect {
        x: rect.x + rect.w - AGENT_LIST_ROW_MARKER_WIDTH,
        y: rect.y + 2.0,
        w: AGENT_LIST_ROW_MARKER_WIDTH,
        h: (rect.h - 4.0).max(10.0),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "agent-list text glow helper needs position/color/anchor plus non-uniform scaling"
)]
fn glow_label(
    painter: &mut HudPainter,
    position: Vec2,
    text: &str,
    size: f32,
    color: peniko::Color,
    anchor: VelloTextAnchor,
    scale_x: f32,
    scale_y: f32,
) {
    for (offset, alpha) in [
        (Vec2::new(0.0, 0.0), 0.18),
        (Vec2::new(0.7, 0.0), 0.12),
        (Vec2::new(-0.7, 0.0), 0.12),
        (Vec2::new(0.0, 0.7), 0.08),
    ] {
        painter.label_scaled(
            position + offset,
            text,
            size,
            apply_alpha(color, alpha),
            anchor,
            scale_x,
            scale_y,
        );
    }
    painter.label_scaled(position, text, size, color, anchor, scale_x, scale_y);
}

fn glow_rect(painter: &mut HudPainter, rect: HudRect, stroke: peniko::Color, fill: peniko::Color) {
    painter.fill_rect(inflate_rect(rect, 2.0), apply_alpha(stroke, 0.08), 0.0);
    painter.stroke_rect(inflate_rect(rect, 2.0), apply_alpha(stroke, 0.20), 0.0);
    painter.stroke_rect(inflate_rect(rect, 1.0), apply_alpha(stroke, 0.36), 0.0);
    painter.fill_rect(rect, fill, 0.0);
    painter.stroke_rect(rect, stroke, 0.0);
}

fn draw_left_rail(painter: &mut HudPainter, content_rect: HudRect) {
    let rail_x = content_rect.x + 12.0;
    let top = content_rect.y + HUD_MODULE_PADDING;
    let bottom = content_rect.y + content_rect.h - HUD_MODULE_PADDING;
    painter.fill_rect(
        HudRect {
            x: rail_x,
            y: top,
            w: 2.0,
            h: (bottom - top).max(0.0),
        },
        apply_alpha(EVA_CYAN, 0.85),
        0.0,
    );

    let mut idx = 0usize;
    let mut y = top + 24.0;
    while y <= bottom - 2.0 {
        painter.fill_rect(
            HudRect {
                x: rail_x - 6.0,
                y,
                w: 14.0,
                h: 2.0,
            },
            apply_alpha(EVA_CYAN, 0.8),
            0.0,
        );
        if idx < 6 {
            glow_label(
                painter,
                Vec2::new(rail_x - 28.0, y - 10.0),
                &format!("+{:02}", idx + 1),
                13.0,
                EVA_CYAN,
                VelloTextAnchor::TopLeft,
                0.9,
                1.05,
            );
        }
        idx += 1;
        y += 48.0;
    }
}

pub(crate) fn render_content(
    model: &HudModuleModel,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    let HudModuleModel::AgentList(state) = model else {
        return;
    };

    draw_left_rail(painter, content_rect);

    glow_label(
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
    glow_label(
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

    for (index, row) in agent_rows(
        content_rect,
        state.scroll_offset,
        state.hovered_terminal,
        inputs.terminal_manager,
        inputs.agent_directory,
    )
    .into_iter()
    .enumerate()
    {
        if row.rect.y + row.rect.h < content_rect.y || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }

        let main_rect = row_main_rect(row.rect);
        let marker_rect = row_marker_rect(row.rect);
        let stroke = if row.focused {
            EVA_ORANGE_BRIGHT
        } else if row.hovered {
            EVA_ORANGE
        } else {
            EVA_ORANGE_DIM
        };
        let fill = if row.focused {
            apply_alpha(EVA_BLACK, 0.94)
        } else if row.hovered {
            apply_alpha(EVA_BLACK, 0.92)
        } else {
            apply_alpha(EVA_BLACK, 0.90)
        };

        glow_rect(painter, main_rect, stroke, fill);
        glow_rect(
            painter,
            marker_rect,
            stroke,
            if row.focused {
                apply_alpha(EVA_ORANGE_BRIGHT, 0.96)
            } else {
                apply_alpha(EVA_ORANGE, 0.78)
            },
        );

        glow_label(
            painter,
            Vec2::new(main_rect.x + 5.0, main_rect.y + 2.0),
            &row.label.to_uppercase(),
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
        glow_label(
            painter,
            Vec2::new(marker_rect.x + marker_rect.w * 0.5, marker_rect.y + 3.0),
            &format!("{:02}", index + 1),
            11.0,
            HudColors::TEXT_ON_ACCENT,
            VelloTextAnchor::Top,
            0.84,
            1.08,
        );
    }
}
