mod interaction;
mod render;

use crate::{
    hud::{AgentDirectory, HudRect, HUD_MODULE_PADDING, HUD_ROW_HEIGHT},
    terminals::{TerminalFocusState, TerminalId, TerminalManager},
};

pub(crate) const AGENT_LIST_HEADER_HEIGHT: f32 = 52.0;
pub(crate) const AGENT_LIST_LEFT_RAIL_WIDTH: f32 = 20.0;
pub(crate) const AGENT_LIST_ROW_MARKER_WIDTH: f32 = 12.0;
pub(crate) const AGENT_LIST_ROW_MARKER_GAP: f32 = 10.0;
pub(crate) const AGENT_LIST_ROW_GAP: f32 = 14.0;
pub(crate) const AGENT_LIST_BORDER_ORANGE_R: u8 = 225;
pub(crate) const AGENT_LIST_BORDER_ORANGE_G: u8 = 129;
pub(crate) const AGENT_LIST_BORDER_ORANGE_B: u8 = 10;
pub(crate) const AGENT_LIST_BLOOM_RED_R: u8 = 143;
pub(crate) const AGENT_LIST_BLOOM_RED_G: u8 = 37;
pub(crate) const AGENT_LIST_BLOOM_RED_B: u8 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AgentListRowSection {
    Main,
    Marker,
    Accent,
}

pub(crate) use interaction::{clear_hover, handle_hover, handle_pointer_click, handle_scroll};
pub(crate) use render::render_content;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentRow {
    pub(crate) terminal_id: TerminalId,
    pub(crate) label: String,
    pub(crate) display_label: String,
    pub(crate) rect: HudRect,
    pub(crate) focused: bool,
    pub(crate) hovered: bool,
}

/// Derives one sub-rectangle of an agent row for rendering or hit-testing.
///
/// A logical row is split into the main label box, the narrow status marker, and a tiny accent strip.
/// The helper bakes in the EVA-specific padding constants and clamps dimensions so very small rows do
/// not collapse to negative sizes.
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
        AgentListRowSection::Accent => HudRect {
            x: rect.x + 3.0,
            y: rect.y + 3.0,
            w: 8.0,
            h: (rect.h - 6.0).max(10.0),
        },
    }
}

/// Resolves the human-facing label shown for one terminal in the agent list.
///
/// Explicit directory labels win. Otherwise the function falls back to a stable `agent-N` label based
/// on the terminal's creation-order position, with the raw terminal id only used as a last resort.
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

/// Returns the vertical distance from one agent row origin to the next.
///
/// This is row height plus the fixed inter-row gap used by the agent-list layout.
pub(crate) fn agent_row_stride() -> f32 {
    HUD_ROW_HEIGHT + AGENT_LIST_ROW_GAP
}

/// Computes the total scrollable content height for a given number of agent rows.
///
/// The final row does not contribute a trailing gap, so the formula subtracts one inter-row gap when
/// at least one row exists.
pub(crate) fn agent_list_content_height(row_count: usize) -> f32 {
    match row_count {
        0 => 0.0,
        _ => row_count as f32 * agent_row_stride() - AGENT_LIST_ROW_GAP,
    }
}

/// Builds the retained row descriptors needed to render and interact with the agent list.
///
/// The rows are produced in terminal-manager order, positioned inside the module content area with the
/// current scroll offset applied, and annotated with focus/hover flags plus the resolved display label.
pub(crate) fn agent_rows(
    shell_rect: HudRect,
    scroll_offset: f32,
    hovered_terminal: Option<TerminalId>,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    agent_directory: &AgentDirectory,
) -> Vec<AgentRow> {
    let terminal_ids = terminal_manager.terminal_ids();
    let content_x = shell_rect.x + AGENT_LIST_LEFT_RAIL_WIDTH + 1.0;
    let content_y = shell_rect.y + HUD_MODULE_PADDING + AGENT_LIST_HEADER_HEIGHT;
    let content_w = (shell_rect.w - AGENT_LIST_LEFT_RAIL_WIDTH - 3.0).max(0.0);
    let row_stride = agent_row_stride();
    terminal_ids
        .iter()
        .enumerate()
        .map(|(index, terminal_id)| {
            let label = resolve_agent_label(terminal_ids, agent_directory, *terminal_id);
            AgentRow {
                terminal_id: *terminal_id,
                display_label: label.to_uppercase(),
                label,
                rect: HudRect {
                    x: content_x,
                    y: content_y + index as f32 * row_stride - scroll_offset,
                    w: content_w,
                    h: HUD_ROW_HEIGHT,
                },
                focused: focus_state.active_id() == Some(*terminal_id),
                hovered: hovered_terminal == Some(*terminal_id),
            }
        })
        .collect()
}
