mod agent_list;
mod debug_toolbar;

use crate::{
    hud::{
        render::{HudPainter, HudRenderInputs},
        AgentDirectory, HudIntent, HudLayoutState, HudModuleId, HudModuleModel, HudRect,
    },
    terminals::{
        TerminalFocusState, TerminalManager, TerminalPresentationStore, TerminalViewState,
    },
};
use bevy::prelude::Vec2;

#[cfg(test)]
pub(crate) use agent_list::resolve_agent_label;
pub(crate) use agent_list::{
    agent_row_rect, agent_rows, AgentListRowSection, AGENT_LIST_BLOOM_RED_B,
    AGENT_LIST_BLOOM_RED_G, AGENT_LIST_BLOOM_RED_R,
};
#[cfg(test)]
pub(crate) use agent_list::{
    AGENT_LIST_BORDER_ORANGE_B, AGENT_LIST_BORDER_ORANGE_G, AGENT_LIST_BORDER_ORANGE_R,
};
#[cfg(test)]
pub(crate) use debug_toolbar::debug_toolbar_buttons;

#[allow(
    clippy::too_many_arguments,
    reason = "module click routing needs shell geometry, terminal state, agent data, and command output together"
)]
/// Dispatches a HUD pointer click to the currently addressed module implementation.
///
/// This module-level router keeps module-specific click logic out of the generic HUD input system and
/// preserves a single call site regardless of which module type is active.
pub(crate) fn handle_pointer_click(
    module_id: HudModuleId,
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    presentation_store: &TerminalPresentationStore,
    view_state: &TerminalViewState,
    agent_directory: &AgentDirectory,
    layout_state: &HudLayoutState,
    emitted_commands: &mut Vec<HudIntent>,
) {
    match module_id {
        HudModuleId::DebugToolbar => debug_toolbar::handle_pointer_click(
            model,
            shell_rect,
            point,
            terminal_manager,
            focus_state,
            presentation_store,
            view_state,
            layout_state,
            emitted_commands,
        ),
        HudModuleId::AgentList => agent_list::handle_pointer_click(
            model,
            shell_rect,
            point,
            terminal_manager,
            focus_state,
            agent_directory,
            emitted_commands,
        ),
    }
}

/// Dispatches hover updates to the addressed HUD module and returns whether its hover state changed.
///
/// Debug toolbar currently ignores hover, while agent-list hover is delegated to its own retained
/// interaction logic.
pub(crate) fn handle_hover(
    module_id: HudModuleId,
    model: &mut HudModuleModel,
    shell_rect: HudRect,
    point: Option<Vec2>,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    agent_directory: &AgentDirectory,
) -> bool {
    match module_id {
        HudModuleId::DebugToolbar => false,
        HudModuleId::AgentList => agent_list::handle_hover(
            model,
            shell_rect,
            point,
            terminal_manager,
            focus_state,
            agent_directory,
        ),
    }
}

/// Clears retained hover state for the addressed HUD module.
///
/// This is the counterpart to [`handle_hover`] and again only matters for modules that track hover
/// internally.
pub(crate) fn clear_hover(module_id: HudModuleId, model: &mut HudModuleModel) -> bool {
    match module_id {
        HudModuleId::DebugToolbar => false,
        HudModuleId::AgentList => agent_list::clear_hover(model),
    }
}

/// Dispatches module-body rendering to the addressed module implementation.
///
/// The generic HUD renderer only needs to know which module is being drawn; the module-specific body
/// drawing stays encapsulated behind this router.
pub(crate) fn render_module_content(
    module_id: HudModuleId,
    model: &HudModuleModel,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    match module_id {
        HudModuleId::DebugToolbar => {
            debug_toolbar::render_content(model, content_rect, painter, inputs)
        }
        HudModuleId::AgentList => agent_list::render_content(model, content_rect, painter, inputs),
    }
}

/// Dispatches scroll input to the addressed HUD module.
///
/// Only the agent list currently consumes scroll deltas; the debug toolbar intentionally ignores
/// them.
pub(crate) fn handle_scroll(
    module_id: HudModuleId,
    model: &mut HudModuleModel,
    delta_y: f32,
    terminal_manager: &TerminalManager,
    shell_rect: HudRect,
) {
    match module_id {
        HudModuleId::DebugToolbar => {}
        HudModuleId::AgentList => {
            agent_list::handle_scroll(
                model,
                delta_y,
                terminal_manager.terminal_ids().len(),
                shell_rect.h,
            );
        }
    }
}
