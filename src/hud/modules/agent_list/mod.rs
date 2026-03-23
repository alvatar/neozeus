mod interaction;
mod render;

use crate::{
    hud::{AgentDirectory, HudRect, HUD_MODULE_PADDING, HUD_ROW_HEIGHT},
    terminals::{TerminalId, TerminalManager},
};

pub(crate) const AGENT_LIST_HEADER_HEIGHT: f32 = 52.0;
pub(crate) const AGENT_LIST_LEFT_RAIL_WIDTH: f32 = 20.0;
pub(crate) const AGENT_LIST_ROW_MARKER_WIDTH: f32 = 12.0;
pub(crate) const AGENT_LIST_ROW_MARKER_GAP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AgentListRowSection {
    Main,
    Marker,
}

pub(crate) use interaction::{clear_hover, handle_hover, handle_pointer_click, handle_scroll};
pub(crate) use render::render_content;

fn hash01(seed: u32) -> f32 {
    let mixed = seed.wrapping_mul(1_597_334_677).rotate_left(13) ^ 0x68bc_21ebu32;
    (mixed & 1023) as f32 / 1023.0
}

pub(crate) fn agent_button_irregularities(rect: HudRect, seed: u32) -> Vec<(HudRect, f32)> {
    let inset = 1.5;
    let bounds = HudRect {
        x: rect.x + inset,
        y: rect.y + inset,
        w: (rect.w - inset * 2.0).max(1.0),
        h: (rect.h - inset * 2.0).max(1.0),
    };

    let mut irregularities = Vec::new();
    let top_band_w = (bounds.w * (0.34 + hash01(seed + 1) * 0.22)).min(bounds.w);
    irregularities.push((
        HudRect {
            x: bounds.x + 1.0,
            y: bounds.y + 1.0,
            w: top_band_w.max(2.0),
            h: 1.2,
        },
        0.16 + hash01(seed + 2) * 0.14,
    ));

    let mid_band_x = bounds.x + bounds.w * (0.14 + hash01(seed + 3) * 0.28);
    let mid_band_w = ((bounds.x + bounds.w - mid_band_x - 1.0).max(2.0))
        .min(bounds.w * (0.18 + hash01(seed + 4) * 0.22));
    irregularities.push((
        HudRect {
            x: mid_band_x,
            y: bounds.y + bounds.h * (0.34 + hash01(seed + 5) * 0.16),
            w: mid_band_w,
            h: 1.0,
        },
        0.10 + hash01(seed + 6) * 0.10,
    ));

    irregularities.push((
        HudRect {
            x: bounds.x + 1.0,
            y: bounds.y + 1.0,
            w: 1.0,
            h: (bounds.h * (0.42 + hash01(seed + 7) * 0.24)).max(3.0),
        },
        0.14 + hash01(seed + 8) * 0.12,
    ));

    let bottom_band_w = (bounds.w * (0.22 + hash01(seed + 9) * 0.18)).min(bounds.w);
    irregularities.push((
        HudRect {
            x: bounds.x + bounds.w - bottom_band_w - 1.0,
            y: bounds.y + bounds.h - 2.2,
            w: bottom_band_w.max(2.0),
            h: 1.0,
        },
        0.08 + hash01(seed + 10) * 0.10,
    ));

    irregularities
        .into_iter()
        .filter(|(rect, alpha)| rect.w > 0.0 && rect.h > 0.0 && *alpha > 0.0)
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentRow {
    pub(crate) terminal_id: TerminalId,
    pub(crate) label: String,
    pub(crate) rect: HudRect,
    pub(crate) focused: bool,
    pub(crate) hovered: bool,
}

pub(crate) fn agent_row_rect(rect: HudRect, section: AgentListRowSection) -> HudRect {
    match section {
        AgentListRowSection::Main => HudRect {
            x: rect.x,
            y: rect.y + 2.0,
            w: (rect.w - AGENT_LIST_ROW_MARKER_WIDTH - AGENT_LIST_ROW_MARKER_GAP).max(12.0),
            h: (rect.h - 4.0).max(10.0),
        },
        AgentListRowSection::Marker => HudRect {
            x: rect.x + rect.w - AGENT_LIST_ROW_MARKER_WIDTH,
            y: rect.y + 2.0,
            w: AGENT_LIST_ROW_MARKER_WIDTH,
            h: (rect.h - 4.0).max(10.0),
        },
    }
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
    let content_x = shell_rect.x + AGENT_LIST_LEFT_RAIL_WIDTH + 1.0;
    let content_y = shell_rect.y + HUD_MODULE_PADDING + AGENT_LIST_HEADER_HEIGHT;
    let content_w = (shell_rect.w - AGENT_LIST_LEFT_RAIL_WIDTH - 3.0).max(0.0);
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
