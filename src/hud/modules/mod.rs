mod agent_list;
mod debug_toolbar;

use crate::{
    hud::{AgentDirectory, HudDispatcher, HudEvent, HudModuleId, HudModuleModel, HudRect},
    terminals::{TerminalManager, TerminalPresentationStore, TerminalViewState},
};
use bevy::prelude::Vec2;

pub(crate) use agent_list::agent_rows;
#[cfg(test)]
pub(crate) use agent_list::resolve_agent_label;
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

pub(crate) fn handle_event(model: &mut HudModuleModel, event: &HudEvent) {
    match model {
        HudModuleModel::DebugToolbar(_) => debug_toolbar::handle_event(model, event),
        HudModuleModel::AgentList(_) => agent_list::handle_event(model, event),
    }
}
