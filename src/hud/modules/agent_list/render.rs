use crate::hud::{
    render::{HudColors, HudPainter, HudRenderInputs},
    HudModuleModel, HudRect,
};
use bevy::prelude::Vec2;
use bevy_vello::prelude::VelloTextAnchor;

use super::agent_rows;

pub(crate) fn render_content(
    model: &HudModuleModel,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    let HudModuleModel::AgentList(state) = model else {
        return;
    };
    for row in agent_rows(
        content_rect,
        state.scroll_offset,
        state.hovered_terminal,
        inputs.terminal_manager,
        inputs.agent_directory,
    ) {
        if row.rect.y + row.rect.h < content_rect.y || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }
        painter.fill_rect(row.rect, HudColors::BUTTON_ACTIVE, 0.0);
        painter.stroke_rect(row.rect, HudColors::BUTTON_BORDER, 0.0);
        painter.label(
            Vec2::new(row.rect.x + 10.0, row.rect.y + 7.0),
            &row.label,
            15.0,
            HudColors::TEXT_ON_ACCENT,
            VelloTextAnchor::TopLeft,
        );
    }
}
