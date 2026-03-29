use super::super::super::render::{HudColors, HudPainter, HudRenderInputs};
use super::super::super::state::HudRect;
use super::super::super::view_models::UsageBarView;
use bevy::prelude::Vec2;
use bevy_vello::prelude::{peniko, VelloTextAnchor};

pub(in crate::hud) const INFO_BAR_BACKGROUND: peniko::Color = HudColors::FRAME;
pub(in crate::hud) const INFO_BAR_BORDER: peniko::Color = HudColors::BORDER;
const INFO_BAR_LABEL_COLOR: peniko::Color = HudColors::TEXT_MUTED;
const INFO_BAR_TRACK_COLOR: peniko::Color = HudColors::BUTTON;
const INFO_BAR_TRACK_SEPARATOR: peniko::Color = HudColors::BORDER;
const INFO_BAR_PADDING_X: f32 = 4.0;
const INFO_BAR_PADDING_Y: f32 = 10.0;
const INFO_BAR_ROW_GAP: f32 = 8.0;
const INFO_BAR_CONTENT_WIDTH_RATIO: f32 = 0.5;
const INFO_BAR_SECTION_GAP: f32 = 18.0;
const INFO_BAR_COMPACT_SECTION_GAP: f32 = 10.0;
const INFO_BAR_LABEL_GAP: f32 = 8.0;
const INFO_BAR_VALUE_GAP: f32 = 6.0;
const INFO_BAR_BAR_HEIGHT: f32 = 14.0;
const INFO_BAR_LABEL_SIZE: f32 = 14.0;
const INFO_BAR_COMPACT_LABEL_SIZE: f32 = 13.0;
const INFO_BAR_VALUE_SIZE: f32 = 14.0;
const INFO_BAR_COMPACT_VALUE_SIZE: f32 = 13.0;
const INFO_BAR_PERCENT_WIDTH: f32 = 36.0;
const INFO_BAR_COMPACT_PERCENT_WIDTH: f32 = 32.0;
const INFO_BAR_DETAIL_WIDTH: f32 = 76.0;
const INFO_BAR_COMPACT_DETAIL_WIDTH: f32 = 64.0;
const INFO_BAR_MIN_BAR_WIDTH: f32 = 40.0;
const INFO_BAR_MINI_BAR_WIDTH: f32 = 28.0;
const INFO_BAR_SESSION_WIDTH_RATIO: f32 = 0.56;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::hud) struct InfoBarDensity {
    pub(in crate::hud) compact: bool,
    pub(in crate::hud) section_gap: f32,
    pub(in crate::hud) percent_width: f32,
    pub(in crate::hud) detail_width: f32,
    pub(in crate::hud) label_size: f32,
    pub(in crate::hud) value_size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::hud) struct InfoBarMetricLayout {
    pub(in crate::hud) group_rect: HudRect,
    pub(in crate::hud) label_position: Vec2,
    pub(in crate::hud) bar_rect: HudRect,
    pub(in crate::hud) pct_rect: HudRect,
    pub(in crate::hud) detail_rect: HudRect,
}

/// Returns the NeoZeus warm usage gradient (muted amber → orange → hot orange-red).
pub(in crate::hud) fn usage_gradient_color(pct: f32) -> peniko::Color {
    let clamped = pct.clamp(0.0, 100.0) / 100.0;
    let low = peniko::Color::from_rgba8(216, 160, 96, 255);
    let mid = HudColors::TEXT;
    let high = peniko::Color::from_rgba8(255, 96, 48, 255);
    if clamped < 0.70 {
        mix_color(low, mid, clamped / 0.70)
    } else {
        mix_color(mid, high, (clamped - 0.70) / 0.30)
    }
}

fn mix_color(a: peniko::Color, b: peniko::Color, t: f32) -> peniko::Color {
    let a = a.to_rgba8();
    let b = b.to_rgba8();
    let t = t.clamp(0.0, 1.0);
    peniko::Color::from_rgba8(
        (a.r as f32 + (b.r as f32 - a.r as f32) * t).round() as u8,
        (a.g as f32 + (b.g as f32 - a.g as f32) * t).round() as u8,
        (a.b as f32 + (b.b as f32 - a.b as f32) * t).round() as u8,
        (a.a as f32 + (b.a as f32 - a.a as f32) * t).round() as u8,
    )
}

/// Chooses the density policy for the current info-bar width while keeping the reference layout.
pub(in crate::hud) fn info_bar_density(content_rect: HudRect) -> InfoBarDensity {
    if content_rect.w < 1120.0 {
        return InfoBarDensity {
            compact: true,
            section_gap: INFO_BAR_COMPACT_SECTION_GAP,
            percent_width: INFO_BAR_COMPACT_PERCENT_WIDTH,
            detail_width: INFO_BAR_COMPACT_DETAIL_WIDTH,
            label_size: INFO_BAR_COMPACT_LABEL_SIZE,
            value_size: INFO_BAR_COMPACT_VALUE_SIZE,
        };
    }
    InfoBarDensity {
        compact: false,
        section_gap: INFO_BAR_SECTION_GAP,
        percent_width: INFO_BAR_PERCENT_WIDTH,
        detail_width: INFO_BAR_DETAIL_WIDTH,
        label_size: INFO_BAR_LABEL_SIZE,
        value_size: INFO_BAR_VALUE_SIZE,
    }
}

/// Computes the two provider-row rectangles for the reference Zeus layout.
pub(in crate::hud) fn info_bar_row_rects(content_rect: HudRect) -> [HudRect; 2] {
    let row_height =
        ((content_rect.h - INFO_BAR_PADDING_Y * 2.0 - INFO_BAR_ROW_GAP) / 2.0).max(0.0);
    let content_width =
        ((content_rect.w - INFO_BAR_PADDING_X * 2.0) * INFO_BAR_CONTENT_WIDTH_RATIO).max(0.0);
    [
        HudRect {
            x: content_rect.x + INFO_BAR_PADDING_X,
            y: content_rect.y + INFO_BAR_PADDING_Y,
            w: content_width,
            h: row_height,
        },
        HudRect {
            x: content_rect.x + INFO_BAR_PADDING_X,
            y: content_rect.y + INFO_BAR_PADDING_Y + row_height + INFO_BAR_ROW_GAP,
            w: content_width,
            h: row_height,
        },
    ]
}

/// Splits one provider row into `Session` and `Week` metric groups.
pub(in crate::hud) fn info_bar_metric_group_rects(
    row_rect: HudRect,
    density: InfoBarDensity,
) -> [HudRect; 2] {
    let usable_width = (row_rect.w - density.section_gap).max(0.0);
    let session_width = (usable_width * INFO_BAR_SESSION_WIDTH_RATIO).max(0.0);
    let week_width = (usable_width - session_width).max(0.0);
    [
        HudRect {
            x: row_rect.x,
            y: row_rect.y,
            w: session_width,
            h: row_rect.h,
        },
        HudRect {
            x: row_rect.x + session_width + density.section_gap,
            y: row_rect.y,
            w: week_width,
            h: row_rect.h,
        },
    ]
}

/// Computes the inner geometry for one usage metric within a provider row.
pub(in crate::hud) fn info_bar_metric_layout(
    group_rect: HudRect,
    label_width: f32,
    density: InfoBarDensity,
) -> InfoBarMetricLayout {
    let mut detail_width = density.detail_width;
    let mut percent_width = density.percent_width;
    let base_x = group_rect.x + label_width + INFO_BAR_LABEL_GAP;
    let fixed_width = label_width
        + INFO_BAR_LABEL_GAP
        + percent_width
        + INFO_BAR_VALUE_GAP
        + detail_width
        + INFO_BAR_VALUE_GAP;
    let mut bar_width = group_rect.w - fixed_width;
    if bar_width < INFO_BAR_MIN_BAR_WIDTH {
        let shortage = INFO_BAR_MIN_BAR_WIDTH - bar_width;
        let detail_take = shortage.min(detail_width - INFO_BAR_MINI_BAR_WIDTH);
        detail_width -= detail_take;
        bar_width += detail_take;
    }
    if bar_width < INFO_BAR_MIN_BAR_WIDTH {
        let shortage = INFO_BAR_MIN_BAR_WIDTH - bar_width;
        let percent_take = shortage.min(percent_width - INFO_BAR_MINI_BAR_WIDTH);
        percent_width -= percent_take;
        bar_width += percent_take;
    }
    bar_width = bar_width.max(INFO_BAR_MINI_BAR_WIDTH);
    let bar_y = group_rect.y + (group_rect.h - INFO_BAR_BAR_HEIGHT) * 0.5;
    let pct_x = base_x + bar_width + INFO_BAR_VALUE_GAP;
    let detail_x = pct_x + percent_width + INFO_BAR_VALUE_GAP;
    InfoBarMetricLayout {
        group_rect,
        label_position: Vec2::new(group_rect.x, group_rect.y + group_rect.h * 0.5),
        bar_rect: HudRect {
            x: base_x,
            y: bar_y,
            w: bar_width,
            h: INFO_BAR_BAR_HEIGHT,
        },
        pct_rect: HudRect {
            x: pct_x,
            y: bar_y,
            w: percent_width,
            h: INFO_BAR_BAR_HEIGHT,
        },
        detail_rect: HudRect {
            x: detail_x,
            y: bar_y,
            w: detail_width,
            h: INFO_BAR_BAR_HEIGHT,
        },
    }
}

/// Renders the info bar usage contents in the reference two-row Zeus layout.
pub(crate) fn render_content(
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    let density = info_bar_density(content_rect);
    let rows = info_bar_row_rects(content_rect);
    render_provider_row(
        rows[0],
        density,
        &inputs.info_bar_view.claude_session,
        &inputs.info_bar_view.claude_week,
        painter,
    );
    render_provider_row(
        rows[1],
        density,
        &inputs.info_bar_view.openai_session,
        &inputs.info_bar_view.openai_week,
        painter,
    );
}

fn render_provider_row(
    row_rect: HudRect,
    density: InfoBarDensity,
    session_bar: &UsageBarView,
    week_bar: &UsageBarView,
    painter: &mut HudPainter,
) {
    let groups = info_bar_metric_group_rects(row_rect, density);
    render_metric(groups[0], density, session_bar, painter);
    render_metric(groups[1], density, week_bar, painter);
}

fn render_metric(
    group_rect: HudRect,
    density: InfoBarDensity,
    bar_view: &UsageBarView,
    painter: &mut HudPainter,
) {
    let label_width = painter.text_size(&bar_view.label, density.label_size).x;
    let layout = info_bar_metric_layout(group_rect, label_width, density);

    painter.label(
        layout.label_position,
        &bar_view.label,
        density.label_size,
        INFO_BAR_LABEL_COLOR,
        VelloTextAnchor::Left,
    );

    render_usage_bar(layout.bar_rect, bar_view.pct(), painter);

    let pct_text = if bar_view.available {
        format!("{:.0}%", bar_view.pct())
    } else {
        "--".to_owned()
    };
    let pct_width = painter.text_size(&pct_text, density.value_size).x;
    painter.label(
        Vec2::new(
            layout.pct_rect.x + layout.pct_rect.w - pct_width,
            layout.pct_rect.y + layout.pct_rect.h * 0.5,
        ),
        &pct_text,
        density.value_size,
        if bar_view.available {
            usage_gradient_color(bar_view.pct())
        } else {
            INFO_BAR_LABEL_COLOR
        },
        VelloTextAnchor::Left,
    );

    if !bar_view.detail_text.is_empty() {
        let detail_width = painter
            .text_size(&bar_view.detail_text, density.value_size)
            .x;
        painter.label(
            Vec2::new(
                layout.detail_rect.x + layout.detail_rect.w - detail_width,
                layout.detail_rect.y + layout.detail_rect.h * 0.5,
            ),
            &bar_view.detail_text,
            density.value_size,
            INFO_BAR_LABEL_COLOR,
            VelloTextAnchor::Left,
        );
    }
}

fn render_usage_bar(bar_rect: HudRect, pct: f32, painter: &mut HudPainter) {
    painter.fill_rect(bar_rect, INFO_BAR_TRACK_COLOR, 0.0);

    let pct = pct.clamp(0.0, 100.0);
    let filled_width = bar_rect.w * (pct / 100.0);
    if filled_width > 0.0 {
        let slices = ((filled_width / 4.0).ceil() as usize).clamp(1, 64);
        for slice_index in 0..slices {
            let start_t = slice_index as f32 / slices as f32;
            let end_t = (slice_index + 1) as f32 / slices as f32;
            let x0 = bar_rect.x + filled_width * start_t;
            let x1 = bar_rect.x + filled_width * end_t;
            let slice_rect = HudRect {
                x: x0,
                y: bar_rect.y,
                w: (x1 - x0).max(0.5),
                h: bar_rect.h,
            };
            let slice_color = usage_gradient_color(pct * end_t);
            painter.fill_rect(slice_rect, slice_color, 0.0);
        }
    }

    let stripe_count = ((bar_rect.w / 4.0).floor() as usize).max(1);
    for stripe_index in 1..stripe_count {
        let x = bar_rect.x + stripe_index as f32 * (bar_rect.w / stripe_count as f32);
        painter.fill_rect(
            HudRect {
                x,
                y: bar_rect.y,
                w: 1.0,
                h: bar_rect.h,
            },
            mix_color(INFO_BAR_TRACK_SEPARATOR, INFO_BAR_TRACK_COLOR, 0.3),
            0.0,
        );
    }
}
