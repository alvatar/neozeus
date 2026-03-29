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
            pct_milli: 42_000,
            detail_text: "(5m)".into(),
            available: true,
        },
        claude_week: UsageBarView {
            label: "Week:".into(),
            pct_milli: 10_000,
            detail_text: "(2h00m)".into(),
            available: true,
        },
        openai_session: UsageBarView {
            label: "OpenAI Session:".into(),
            pct_milli: 40_000,
            detail_text: "(45s)".into(),
            available: true,
        },
        openai_week: UsageBarView {
            label: "Week:".into(),
            pct_milli: 75_000,
            detail_text: "(1d00h)".into(),
            available: true,
        },
    }
}

/// Verifies that the info bar ignores pointer clicks while it is intentionally empty.
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
            h: 40.0,
        },
        Vec2::new(120.0, 20.0),
        &InfoBarView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert!(emitted_commands.is_empty());
}

/// Verifies that the usage gradient keeps the original Zeus cyan-to-red ramp.
#[test]
fn usage_gradient_keeps_original_cyan_to_red_ramp() {
    let low = render::usage_gradient_color(0.0).to_rgba8();
    assert_eq!((low.r, low.g, low.b, low.a), (0x00, 0xD7, 0xD7, 0xFF));
    let mid = render::usage_gradient_color(70.0).to_rgba8();
    assert_eq!((mid.r, mid.g, mid.b, mid.a), (0xD7, 0xD7, 0x00, 0xFF));
    let high = render::usage_gradient_color(100.0).to_rgba8();
    assert_eq!((high.r, high.g, high.b, high.a), (0xFF, 0x33, 0x33, 0xFF));
}

/// Verifies that info-bar sections are laid out left-to-right in stable order.
#[test]
fn info_bar_section_rects_preserve_bar_order() {
    let rects = render::info_bar_section_rects(HudRect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 40.0,
    });
    assert_eq!(rects.len(), 4);
    assert!(rects[0].x < rects[1].x);
    assert!(rects[1].x < rects[2].x);
    assert!(rects[2].x < rects[3].x);
}

/// Verifies that the section layout reserves bar, percent, and detail columns without overlap.
#[test]
fn info_bar_section_layout_keeps_geometry_ordered() {
    let section = render::info_bar_section_rects(HudRect {
        x: 0.0,
        y: 0.0,
        w: 1280.0,
        h: 40.0,
    })[0];
    let layout = render::info_bar_section_layout(section, 96.0);
    assert!(layout.bar_rect.x >= section.x);
    assert!(layout.bar_rect.x + layout.bar_rect.w <= layout.pct_rect.x);
    assert!(layout.pct_rect.x + layout.pct_rect.w <= layout.detail_rect.x);
}

/// Verifies that narrow info bars switch to a 2×2 section layout.
#[test]
fn info_bar_section_rects_wrap_to_two_rows_when_narrow() {
    let rects = render::info_bar_section_rects(HudRect {
        x: 0.0,
        y: 0.0,
        w: 1000.0,
        h: 72.0,
    });
    assert_eq!(rects[0].y, rects[1].y);
    assert!(rects[2].y > rects[0].y);
    assert_eq!(rects[2].x, rects[0].x);
    assert_eq!(rects[3].x, rects[1].x);
}

/// Verifies that the sample view keeps the expected visible bar order.
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
