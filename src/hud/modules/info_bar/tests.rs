use super::{input::handle_pointer_click, render};
use crate::hud::{
    default_hud_module_instance, HudRect, HudState, HudWidgetKey, InfoBarView, UsageBarView,
    HUD_MODULE_DEFINITIONS,
};
use bevy::prelude::*;

fn sample_info_bar_view() -> InfoBarView {
    InfoBarView {
        claude_session: UsageBarView {
            label: "Claude Session:".into(),
            pct_milli: 8_000,
            detail_text: "(now)".into(),
            available: true,
        },
        claude_week: UsageBarView {
            label: "Week:".into(),
            pct_milli: 54_000,
            detail_text: "(4d01h)".into(),
            available: true,
        },
        openai_session: UsageBarView {
            label: "OpenAI Session:".into(),
            pct_milli: 2_000,
            detail_text: "(4h44m)".into(),
            available: true,
        },
        openai_week: UsageBarView {
            label: "Week:".into(),
            pct_milli: 50_000,
            detail_text: "(4d18h)".into(),
            available: true,
        },
    }
}

/// Verifies that the info bar ignores pointer clicks while it is intentionally non-interactive.
#[test]
fn info_bar_click_does_not_emit_commands() {
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::InfoBar,
        default_hud_module_instance(&HUD_MODULE_DEFINITIONS[0]),
    );

    let mut emitted_commands = Vec::new();
    handle_pointer_click(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 1280.0,
            h: 60.0,
        },
        Vec2::new(120.0, 20.0),
        &InfoBarView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert!(emitted_commands.is_empty());
}

/// Verifies that the usage gradient uses the NeoZeus warm palette rather than the borrowed Zeus
/// teal palette.
#[test]
fn usage_gradient_keeps_neozeus_orange_ramp() {
    let low = render::usage_gradient_color(0.0).to_rgba8();
    assert_eq!((low.r, low.g, low.b, low.a), (216, 160, 96, 255));
    let mid = render::usage_gradient_color(70.0).to_rgba8();
    assert_eq!((mid.r, mid.g, mid.b, mid.a), (238, 96, 2, 255));
    let high = render::usage_gradient_color(100.0).to_rgba8();
    assert_eq!((high.r, high.g, high.b, high.a), (255, 96, 48, 255));
}

/// Verifies that the reference layout keeps the Claude row above the OpenAI row and only uses
/// the left half of the info bar width.
#[test]
fn info_bar_rows_keep_provider_order_and_use_left_half_width() {
    let rect = HudRect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 60.0,
    };
    let rows = render::info_bar_row_rects(rect);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].y < rows[1].y);
    assert_eq!(rows[0].x, rows[1].x);
    assert!(rows[0].w <= rect.w * 0.5);
}

/// Verifies that each provider row places `Session` before `Week`.
#[test]
fn info_bar_metric_group_rects_keep_session_before_week() {
    let row = render::info_bar_row_rects(HudRect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 60.0,
    })[0];
    let groups = render::info_bar_metric_group_rects(row, render::info_bar_density(row));
    assert_eq!(groups.len(), 2);
    assert!(groups[0].x < groups[1].x);
    assert!(groups[0].w > groups[1].w);
}

/// Verifies that metric layout keeps label, bar, percent, and detail in left-to-right order.
#[test]
fn info_bar_metric_layout_keeps_geometry_ordered() {
    let row = render::info_bar_row_rects(HudRect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 60.0,
    })[0];
    let density = render::info_bar_density(row);
    let group = render::info_bar_metric_group_rects(row, density)[0];
    let layout = render::info_bar_metric_layout(group, 120.0, density);
    assert!(layout.bar_rect.x >= group.x);
    assert!(layout.bar_rect.x + layout.bar_rect.w <= layout.pct_rect.x);
    assert!(layout.pct_rect.x + layout.pct_rect.w <= layout.detail_rect.x);
}

/// Verifies that narrower widths switch to the compact spacing policy without changing row order.
#[test]
fn info_bar_density_compacts_on_narrow_widths() {
    let density = render::info_bar_density(HudRect {
        x: 0.0,
        y: 0.0,
        w: 1000.0,
        h: 60.0,
    });
    assert!(density.compact);
    let wide_density = render::info_bar_density(HudRect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 60.0,
    });
    assert!(!wide_density.compact);
}

/// Verifies that the sample view keeps the exact visible label order from the reference layout.
#[test]
fn sample_info_bar_view_keeps_expected_bar_order() {
    let info_bar = sample_info_bar_view();
    let labels = [
        info_bar.claude_session.label,
        info_bar.claude_week.label,
        info_bar.openai_session.label,
        info_bar.openai_week.label,
    ];
    assert_eq!(
        labels,
        ["Claude Session:", "Week:", "OpenAI Session:", "Week:"]
    );
}
