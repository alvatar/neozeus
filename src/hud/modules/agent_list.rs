use crate::{
    hud::{
        render::{HudColors, HudPainter, HudRenderInputs},
        AgentDirectory, HudCommand, HudDispatcher, HudModuleModel, HudRect, HUD_MODULE_PADDING,
        HUD_ROW_HEIGHT,
    },
    terminals::{TerminalId, TerminalManager},
};
use bevy::prelude::*;
use bevy_vello::prelude::VelloTextAnchor;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentRow {
    pub(crate) terminal_id: TerminalId,
    pub(crate) label: String,
    pub(crate) rect: HudRect,
    pub(crate) focused: bool,
    pub(crate) hovered: bool,
}

pub(crate) fn resolve_agent_label(
    terminal_ids: &[TerminalId],
    agent_directory: &AgentDirectory,
    terminal_id: TerminalId,
) -> String {
    if let Some(label) = agent_directory.labels.get(&terminal_id) {
        return label.clone();
    }
    let index = terminal_ids
        .iter()
        .position(|existing| *existing == terminal_id)
        .map(|index| index + 1)
        .unwrap_or(terminal_id.0 as usize);
    format!("agent-{index}")
}

pub(crate) fn agent_rows(
    shell_rect: HudRect,
    scroll_offset: f32,
    hovered_terminal: Option<TerminalId>,
    terminal_manager: &TerminalManager,
    agent_directory: &AgentDirectory,
) -> Vec<AgentRow> {
    let terminal_ids = terminal_manager.terminal_ids();
    let content_x = shell_rect.x + HUD_MODULE_PADDING;
    let content_y = shell_rect.y + HUD_MODULE_PADDING;
    let content_w = (shell_rect.w - HUD_MODULE_PADDING * 2.0).max(0.0);
    terminal_ids
        .iter()
        .enumerate()
        .map(|(index, terminal_id)| AgentRow {
            terminal_id: *terminal_id,
            label: resolve_agent_label(terminal_ids, agent_directory, *terminal_id),
            rect: HudRect {
                x: content_x,
                y: content_y + index as f32 * HUD_ROW_HEIGHT - scroll_offset,
                w: content_w,
                h: HUD_ROW_HEIGHT,
            },
            focused: terminal_manager.active_id() == Some(*terminal_id),
            hovered: hovered_terminal == Some(*terminal_id),
        })
        .collect()
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
        painter.fill_rect(
            row.rect,
            if row.focused {
                HudColors::ROW_FOCUSED
            } else if row.hovered {
                HudColors::ROW_HOVERED
            } else {
                HudColors::ROW
            },
            6.0,
        );
        painter.label(
            Vec2::new(row.rect.x + 10.0, row.rect.y + 7.0),
            &row.label,
            15.0,
            HudColors::TEXT,
            VelloTextAnchor::TopLeft,
        );
    }
    painter.label(
        Vec2::new(
            content_rect.x + HUD_MODULE_PADDING,
            content_rect.y + content_rect.h - HUD_ROW_HEIGHT,
        ),
        "click row: focus + isolate",
        13.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
}

pub(crate) fn handle_pointer_click(
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    terminal_manager: &TerminalManager,
    agent_directory: &AgentDirectory,
    dispatcher: &mut HudDispatcher,
) {
    let HudModuleModel::AgentList(state) = model else {
        return;
    };
    for row in agent_rows(
        shell_rect,
        state.scroll_offset,
        state.hovered_terminal,
        terminal_manager,
        agent_directory,
    ) {
        if row.rect.contains(point) {
            dispatcher
                .commands
                .push(HudCommand::FocusTerminal(row.terminal_id));
            dispatcher
                .commands
                .push(HudCommand::HideAllButTerminal(row.terminal_id));
            break;
        }
    }
}

pub(crate) fn handle_hover(
    model: &mut HudModuleModel,
    shell_rect: HudRect,
    point: Option<Vec2>,
    terminal_manager: &TerminalManager,
    agent_directory: &AgentDirectory,
) -> bool {
    let HudModuleModel::AgentList(state) = model else {
        return false;
    };
    let hovered_terminal = point.and_then(|point| {
        agent_rows(
            shell_rect,
            state.scroll_offset,
            None,
            terminal_manager,
            agent_directory,
        )
        .into_iter()
        .find(|row| row.rect.contains(point))
        .map(|row| row.terminal_id)
    });
    if state.hovered_terminal == hovered_terminal {
        return false;
    }
    state.hovered_terminal = hovered_terminal;
    true
}

pub(crate) fn handle_scroll(
    model: &mut HudModuleModel,
    delta_y: f32,
    row_count: usize,
    height: f32,
) {
    let HudModuleModel::AgentList(state) = model else {
        return;
    };
    let content_height = (row_count as f32 * HUD_ROW_HEIGHT).max(height);
    let max_scroll = (content_height - height).max(0.0);
    state.scroll_offset = (state.scroll_offset - delta_y).clamp(0.0, max_scroll);
}

pub(crate) fn clear_hover(model: &mut HudModuleModel) -> bool {
    let HudModuleModel::AgentList(state) = model else {
        return false;
    };
    if state.hovered_terminal.is_none() {
        return false;
    }
    state.hovered_terminal = None;
    true
}
