use crate::hud::{
    render::{HudColors, HudPainter, HudRenderInputs},
    HudRect, HUD_BUTTON_HEIGHT,
};
use bevy::prelude::Vec2;
use bevy_vello::prelude::VelloTextAnchor;

use super::debug_toolbar_buttons;

/// Renders the debug toolbar's status text and button strip.
pub(crate) fn render_content(
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    let buttons =
        debug_toolbar_buttons(content_rect, inputs.debug_toolbar_view, inputs.layout_state);
    let debug = inputs.debug_toolbar_view;

    painter.label(
        Vec2::new(content_rect.x, content_rect.y + HUD_BUTTON_HEIGHT + 8.0),
        &format!(
            "terms {} · active {} · {} · zoom {:.2}",
            debug.terminal_count,
            debug.active_terminal_display,
            debug.active_status,
            debug.zoom_distance(),
        ),
        14.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(
            content_rect.x + 430.0,
            content_rect.y + HUD_BUTTON_HEIGHT + 8.0,
        ),
        &debug.font_summary,
        14.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(
            content_rect.x + 620.0,
            content_rect.y + HUD_BUTTON_HEIGHT + 8.0,
        ),
        &format!(
            "keys {} drop {} rows {}",
            debug.key_events_seen, debug.updates_dropped, debug.dirty_rows_uploaded,
        ),
        14.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    for button in buttons {
        painter.fill_rect(
            button.rect,
            if button.active {
                HudColors::BUTTON_ACTIVE
            } else {
                HudColors::BUTTON
            },
            6.0,
        );
        painter.stroke_rect(button.rect, HudColors::BUTTON_BORDER, 6.0);
        painter.label(
            Vec2::new(button.rect.x + 10.0, button.rect.y + 6.0),
            &button.label,
            14.0,
            if button.active {
                HudColors::TEXT_ON_ACCENT
            } else {
                HudColors::TEXT
            },
            VelloTextAnchor::TopLeft,
        );
    }
}
