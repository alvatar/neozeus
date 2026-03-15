mod agent_list;
mod debug_toolbar;

use crate::{
    hud::{
        render::{HudPainter, HudRenderInputs},
        AgentDirectory, HudDispatcher, HudModuleId, HudModuleModel, HudRect, HudState,
    },
    terminals::{TerminalManager, TerminalPresentationStore, TerminalViewState},
};
use bevy::prelude::Vec2;

#[cfg(test)]
pub(crate) use agent_list::agent_rows;
#[cfg(test)]
pub(crate) use agent_list::resolve_agent_label;
#[cfg(test)]
pub(crate) use debug_toolbar::debug_toolbar_buttons;

#[allow(
    clippy::too_many_arguments,
    reason = "module click routing needs shell geometry, terminal state, agent data, and dispatcher together"
)]
pub(crate) fn handle_pointer_click(
    module_id: HudModuleId,
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
    view_state: &TerminalViewState,
    agent_directory: &AgentDirectory,
    hud_state: &HudState,
    dispatcher: &mut HudDispatcher,
) {
    match module_id {
        HudModuleId::DebugToolbar => debug_toolbar::handle_pointer_click(
            model,
            shell_rect,
            point,
            terminal_manager,
            presentation_store,
            view_state,
            hud_state,
            dispatcher,
        ),
        HudModuleId::AgentList => agent_list::handle_pointer_click(
            model,
            shell_rect,
            point,
            terminal_manager,
            agent_directory,
            dispatcher,
        ),
    }
}

pub(crate) fn handle_hover(
    module_id: HudModuleId,
    model: &mut HudModuleModel,
    shell_rect: HudRect,
    point: Option<Vec2>,
    terminal_manager: &TerminalManager,
    agent_directory: &AgentDirectory,
) -> bool {
    match module_id {
        HudModuleId::DebugToolbar => false,
        HudModuleId::AgentList => {
            agent_list::handle_hover(model, shell_rect, point, terminal_manager, agent_directory)
        }
    }
}

pub(crate) fn clear_hover(module_id: HudModuleId, model: &mut HudModuleModel) -> bool {
    match module_id {
        HudModuleId::DebugToolbar => false,
        HudModuleId::AgentList => agent_list::clear_hover(model),
    }
}

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
