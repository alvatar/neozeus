use super::super::super::render::{HudColors, HudPainter, HudRenderInputs};
use super::super::super::state::HudRect;
use super::super::super::view_models::UsageBarView;
use bevy::prelude::Vec2;
use bevy_vello::prelude::{peniko, VelloTextAnchor};

const INFO_BAR_PADDING_X: f32 = 10.0;
const INFO_BAR_PADDING_Y: f32 = 8.0;
const INFO_BAR_SECTION_GAP: f32 = 10.0;
const INFO_BAR_COMPACT_SECTION_GAP: f32 = 6.0;
const INFO_BAR_ROW_GAP: f32 = 6.0;
const INFO_BAR_BAR_CELL_COUNT: usize = 12;
const INFO_BAR_BAR_CELL_GAP: f32 = 2.0;
const INFO_BAR_LABEL_SIZE: f32 = 14.0;
const INFO_BAR_COMPACT_LABEL_SIZE: f32 = 13.0;
const INFO_BAR_VALUE_SIZE: f32 = 14.0;
const INFO_BAR_COMPACT_VALUE_SIZE: f32 = 13.0;
const INFO_BAR_MIN_BAR_CELL_WIDTH: f32 = 4.0;
const INFO_BAR_MAX_BAR_CELL_WIDTH: f32 = 7.0;
const INFO_BAR_PERCENT_WIDTH: f32 = 38.0;
const INFO_BAR_DETAIL_WIDTH: f32 = 72.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::hud) struct InfoBarCompactness {
    pub(in crate::hud) two_rows: bool,
    pub(in crate::hud) section_gap: f32,
    pub(in crate::hud) compact_text: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::hud) struct InfoBarSectionLayout {
    pub(in crate::hud) section_rect: HudRect,
    pub(in crate::hud) label_position: Vec2,
    pub(in crate::hud) bar_rect: HudRect,
    pub(in crate::hud) pct_rect: HudRect,
    pub(in crate::hud) detail_rect: HudRect,
}

/// Returns the Zeus usage gradient (cyan → yellow → red) for a given percentage.
pub(in crate::hud) fn usage_gradient_color(pct: f32) -> peniko::Color {
    let clamped = pct.clamp(0.0, 100.0) / 100.0;
    let (r, g, b) = if clamped < 0.70 {
        let t = clamped / 0.70;
        (
            (0x00 as f32 + (0xD7 as f32 - 0x00 as f32) * t).round() as u8,
            0xD7,
            (0xD7 as f32 + (0x00 as f32 - 0xD7 as f32) * t).round() as u8,
        )
    } else {
        let t = (clamped - 0.70) / 0.30;
        (
            (0xD7 as f32 + (0xFF as f32 - 0xD7 as f32) * t).round() as u8,
            (0xD7 as f32 + (0x33 as f32 - 0xD7 as f32) * t).round() as u8,
            (0x00 as f32 + (0x33 as f32 - 0x00 as f32) * t).round() as u8,
        )
    };
    peniko::Color::from_rgba8(r, g, b, 255)
}

/// Chooses the spacing/text-density policy for the current info-bar geometry.
pub(in crate::hud) fn info_bar_compactness(content_rect: HudRect) -> InfoBarCompactness {
    let usable_width = (content_rect.w - INFO_BAR_PADDING_X * 2.0).max(0.0);
    let single_row_section_width = ((usable_width - INFO_BAR_SECTION_GAP * 3.0) / 4.0).max(0.0);
    if single_row_section_width < 240.0 {
        return InfoBarCompactness {
            two_rows: true,
            section_gap: INFO_BAR_COMPACT_SECTION_GAP,
            compact_text: true,
        };
    }
    InfoBarCompactness {
        two_rows: false,
        section_gap: if single_row_section_width < 280.0 {
            INFO_BAR_COMPACT_SECTION_GAP
        } else {
            INFO_BAR_SECTION_GAP
        },
        compact_text: single_row_section_width < 280.0,
    }
}

/// Computes the section rects used by the info bar, switching to a 2×2 layout when narrow.
pub(in crate::hud) fn info_bar_section_rects(content_rect: HudRect) -> [HudRect; 4] {
    let compact = info_bar_compactness(content_rect);
    let available_width = (content_rect.w - INFO_BAR_PADDING_X * 2.0).max(0.0);
    if compact.two_rows {
        let section_width = ((available_width - compact.section_gap) / 2.0).max(0.0);
        let section_height =
            ((content_rect.h - INFO_BAR_PADDING_Y * 2.0 - INFO_BAR_ROW_GAP) / 2.0).max(0.0);
        return [
            HudRect {
                x: content_rect.x + INFO_BAR_PADDING_X,
                y: content_rect.y + INFO_BAR_PADDING_Y,
                w: section_width,
                h: section_height,
            },
            HudRect {
                x: content_rect.x + INFO_BAR_PADDING_X + section_width + compact.section_gap,
                y: content_rect.y + INFO_BAR_PADDING_Y,
                w: section_width,
                h: section_height,
            },
            HudRect {
                x: content_rect.x + INFO_BAR_PADDING_X,
                y: content_rect.y + INFO_BAR_PADDING_Y + section_height + INFO_BAR_ROW_GAP,
                w: section_width,
                h: section_height,
            },
            HudRect {
                x: content_rect.x + INFO_BAR_PADDING_X + section_width + compact.section_gap,
                y: content_rect.y + INFO_BAR_PADDING_Y + section_height + INFO_BAR_ROW_GAP,
                w: section_width,
                h: section_height,
            },
        ];
    }

    let total_gap = compact.section_gap * 3.0;
    let section_width = ((available_width - total_gap) / 4.0).max(0.0);
    let section_height = (content_rect.h - INFO_BAR_PADDING_Y * 2.0).max(0.0);
    std::array::from_fn(|index| HudRect {
        x: content_rect.x
            + INFO_BAR_PADDING_X
            + index as f32 * (section_width + compact.section_gap),
        y: content_rect.y + INFO_BAR_PADDING_Y,
        w: section_width,
        h: section_height,
    })
}

/// Computes the inner geometry for one info-bar section.
pub(in crate::hud) fn info_bar_section_layout(
    section_rect: HudRect,
    label_width: f32,
) -> InfoBarSectionLayout {
    let bar_height = 14.0;
    let label_gap = 6.0;
    let value_gap = 6.0;
    let bar_available_width = (section_rect.w
        - label_width
        - INFO_BAR_PERCENT_WIDTH
        - INFO_BAR_DETAIL_WIDTH
        - label_gap
        - value_gap * 2.0)
        .max(INFO_BAR_BAR_CELL_COUNT as f32 * INFO_BAR_MIN_BAR_CELL_WIDTH);
    let cell_width = ((bar_available_width
        - INFO_BAR_BAR_CELL_GAP * (INFO_BAR_BAR_CELL_COUNT.saturating_sub(1) as f32))
        / INFO_BAR_BAR_CELL_COUNT as f32)
        .clamp(INFO_BAR_MIN_BAR_CELL_WIDTH, INFO_BAR_MAX_BAR_CELL_WIDTH);
    let final_bar_width = cell_width * INFO_BAR_BAR_CELL_COUNT as f32
        + INFO_BAR_BAR_CELL_GAP * (INFO_BAR_BAR_CELL_COUNT.saturating_sub(1) as f32);
    let y = section_rect.y + (section_rect.h - bar_height) * 0.5;
    let bar_x = section_rect.x + label_width + label_gap;
    let pct_x = bar_x + final_bar_width + value_gap;
    let detail_x = pct_x + INFO_BAR_PERCENT_WIDTH + value_gap;
    InfoBarSectionLayout {
        section_rect,
        label_position: Vec2::new(section_rect.x, section_rect.y + section_rect.h * 0.5),
        bar_rect: HudRect {
            x: bar_x,
            y,
            w: final_bar_width,
            h: bar_height,
        },
        pct_rect: HudRect {
            x: pct_x,
            y,
            w: INFO_BAR_PERCENT_WIDTH,
            h: bar_height,
        },
        detail_rect: HudRect {
            x: detail_x,
            y,
            w: INFO_BAR_DETAIL_WIDTH,
            h: bar_height,
        },
    }
}

/// Renders the info bar usage contents.
pub(crate) fn render_content(
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    let bars = [
        &inputs.info_bar_view.claude_session,
        &inputs.info_bar_view.claude_week,
        &inputs.info_bar_view.openai_session,
        &inputs.info_bar_view.openai_week,
    ];
    for (section_rect, bar_view) in info_bar_section_rects(content_rect).into_iter().zip(bars) {
        render_usage_section(section_rect, bar_view, painter);
    }
}

fn render_usage_section(section_rect: HudRect, bar_view: &UsageBarView, painter: &mut HudPainter) {
    let compact = info_bar_compactness(section_rect);
    let label_size = if compact.compact_text {
        INFO_BAR_COMPACT_LABEL_SIZE
    } else {
        INFO_BAR_LABEL_SIZE
    };
    let value_size = if compact.compact_text {
        INFO_BAR_COMPACT_VALUE_SIZE
    } else {
        INFO_BAR_VALUE_SIZE
    };
    let label_width = painter.text_size(&bar_view.label, label_size).x;
    let layout = info_bar_section_layout(section_rect, label_width);

    painter.label(
        layout.label_position,
        &bar_view.label,
        label_size,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::Left,
    );

    render_usage_bar(layout.bar_rect, bar_view.pct(), painter);

    let pct_text = format!("{:>4}", format!("{:.0}%", bar_view.pct()));
    let pct_width = painter.text_size(&pct_text, value_size).x;
    painter.label(
        Vec2::new(
            layout.pct_rect.x + layout.pct_rect.w - pct_width,
            layout.pct_rect.y + layout.pct_rect.h * 0.5,
        ),
        &pct_text,
        value_size,
        usage_gradient_color(bar_view.pct()),
        VelloTextAnchor::Left,
    );

    let detail_text = format!("{:>8}", bar_view.detail_text);
    let detail_width = painter.text_size(&detail_text, value_size).x;
    painter.label(
        Vec2::new(
            layout.detail_rect.x + layout.detail_rect.w - detail_width,
            layout.detail_rect.y + layout.detail_rect.h * 0.5,
        ),
        &detail_text,
        value_size,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::Left,
    );
}

fn render_usage_bar(bar_rect: HudRect, pct: f32, painter: &mut HudPainter) {
    let cell_width = ((bar_rect.w
        - INFO_BAR_BAR_CELL_GAP * (INFO_BAR_BAR_CELL_COUNT.saturating_sub(1) as f32))
        / INFO_BAR_BAR_CELL_COUNT as f32)
        .max(0.0);
    let filled =
        ((pct.clamp(0.0, 100.0) / 100.0) * INFO_BAR_BAR_CELL_COUNT as f32).round() as usize;
    for index in 0..INFO_BAR_BAR_CELL_COUNT {
        let cell_rect = HudRect {
            x: bar_rect.x + index as f32 * (cell_width + INFO_BAR_BAR_CELL_GAP),
            y: bar_rect.y,
            w: cell_width,
            h: bar_rect.h,
        };
        let color = if index < filled {
            usage_gradient_color(((index + 1) as f32 / INFO_BAR_BAR_CELL_COUNT as f32) * 100.0)
        } else {
            HudColors::BUTTON
        };
        painter.fill_rect(cell_rect, color, 1.0);
        painter.stroke_rect(cell_rect, HudColors::BUTTON_BORDER, 1.0);
    }
}
