use crate::hud::{
    render::{HudColors, HudPainter, HudRenderInputs},
    HudModuleModel, HudRect, HUD_BUTTON_HEIGHT,
};
use bevy::prelude::Vec2;
use bevy_vello::prelude::VelloTextAnchor;

use super::debug_toolbar_buttons;

/// Renders content.
pub(crate) fn render_content(
    model: &HudModuleModel,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    let HudModuleModel::DebugToolbar(_) = model else {
        return;
    };
    let buttons = debug_toolbar_buttons(
        content_rect,
        inputs.terminal_manager,
        inputs.focus_state,
        inputs.presentation_store,
        inputs.view_state,
        inputs.layout_state,
    );
    let active_status = inputs
        .focus_state
        .active_snapshot(inputs.terminal_manager)
        .map(|snapshot| snapshot.runtime.status.as_str())
        .unwrap_or("no active terminal");
    let active_id = inputs
        .focus_state
        .active_id()
        .map(|id| id.0)
        .unwrap_or_default();
    let debug_stats = inputs
        .focus_state
        .active_debug_stats(inputs.terminal_manager);
    let font_summary = match inputs.font_state.report.as_ref() {
        Some(Ok(report)) => format!("font {}", report.primary.family),
        Some(Err(error)) => format!("font error {error}"),
        None => "font loading".to_owned(),
    };

    painter.label(
        Vec2::new(content_rect.x, content_rect.y + HUD_BUTTON_HEIGHT + 8.0),
        &format!(
            "terms {} · active {} · {} · zoom {:.2}",
            inputs.terminal_manager.terminal_ids().len(),
            active_id,
            active_status,
            inputs.view_state.distance,
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
        &font_summary,
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
            debug_stats.key_events_seen,
            debug_stats.updates_dropped,
            debug_stats.dirty_rows_uploaded,
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
