use crate::hud::{
    render::{HudColors, HudPainter, HudRenderInputs},
    HudModuleModel, HudRect, HUD_ROW_HEIGHT,
};
use bevy::prelude::Vec2;
use bevy_vello::prelude::VelloTextAnchor;

pub(crate) fn render_content(
    model: &HudModuleModel,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    if !matches!(model, HudModuleModel::ThreadPane(_)) {
        return;
    }

    let header = inputs
        .thread_view
        .agent_id
        .and_then(|agent_id| {
            inputs
                .agent_list_view
                .rows
                .iter()
                .find(|row| row.agent_id == agent_id)
                .map(|row| row.label.clone())
        })
        .unwrap_or_else(|| "No thread selected".to_owned());
    painter.label(
        Vec2::new(content_rect.x + 8.0, content_rect.y + 6.0),
        &header,
        15.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );

    if inputs.thread_view.messages.is_empty() {
        painter.label(
            Vec2::new(content_rect.x + 8.0, content_rect.y + HUD_ROW_HEIGHT + 12.0),
            "No messages yet",
            14.0,
            HudColors::TEXT_MUTED,
            VelloTextAnchor::TopLeft,
        );
        return;
    }

    let mut y = content_rect.y + HUD_ROW_HEIGHT + 8.0;
    for message in &inputs.thread_view.messages {
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
            &message.body,
            13.0,
            if message.delivered {
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
