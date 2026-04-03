use super::super::render::{HudColors, HudPainter, HudRenderInputs};
use super::super::state::{HudRect, HUD_ROW_HEIGHT};
use bevy::prelude::Vec2;
use bevy_vello::prelude::VelloTextAnchor;

/// Renders content.
pub(crate) fn render_content(
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    painter.label(
        Vec2::new(content_rect.x + 8.0, content_rect.y + 6.0),
        &inputs.thread_view.header,
        15.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );

    if inputs.thread_view.is_empty() {
        painter.label(
            Vec2::new(content_rect.x + 8.0, content_rect.y + HUD_ROW_HEIGHT + 12.0),
            &inputs.thread_view.empty_message,
            14.0,
            HudColors::TEXT_MUTED,
            VelloTextAnchor::TopLeft,
        );
        return;
    }

    let mut y = content_rect.y + HUD_ROW_HEIGHT + 8.0;
    for (body, delivered) in inputs.thread_view.message_rows() {
        let rect = HudRect {
            x: content_rect.x,
            y,
            w: content_rect.w,
            h: HUD_ROW_HEIGHT + 8.0,
        };
        painter.fill_rect(rect, HudColors::FRAME, 4.0);
        painter.stroke_rect(rect, HudColors::BORDER, 4.0);
        painter.label(
            Vec2::new(rect.x + 8.0, rect.y + 8.0),
            &body,
            13.0,
            if delivered {
                HudColors::TEXT
            } else {
                HudColors::TEXT_MUTED
            },
            VelloTextAnchor::TopLeft,
        );
        y += rect.h + 8.0;
        if y > content_rect.y + content_rect.h - rect.h {
            break;
        }
    }
}
