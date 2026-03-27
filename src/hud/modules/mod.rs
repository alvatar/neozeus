mod agent_list;
mod conversation_list;
mod debug_toolbar;
mod thread_pane;

use crate::{
    hud::{
        render::{HudPainter, HudRenderInputs},
        AgentListView, ConversationListView, HudIntent, HudLayoutState, HudModuleModel, HudRect,
        HudWidgetKey, ThreadView,
    },
    terminals::{
        TerminalFocusState, TerminalManager, TerminalPresentationStore, TerminalViewState,
    },
};
use bevy::prelude::Vec2;

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
    module_id: HudWidgetKey,
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    presentation_store: &TerminalPresentationStore,
    view_state: &TerminalViewState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
    thread_view: &ThreadView,
    layout_state: &HudLayoutState,
    emitted_commands: &mut Vec<HudIntent>,
) {
    match module_id {
        HudWidgetKey::DebugToolbar => debug_toolbar::handle_pointer_click(
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
        HudWidgetKey::AgentList => {
            let _ = (
                terminal_manager,
                focus_state,
                conversation_list_view,
                thread_view,
            );
            agent_list::handle_pointer_click(
                model,
                shell_rect,
                point,
                agent_list_view,
                emitted_commands,
            )
        }
        HudWidgetKey::ConversationList => {
            let _ = (
                terminal_manager,
                focus_state,
                presentation_store,
                view_state,
                agent_list_view,
                thread_view,
                layout_state,
            );
            conversation_list::handle_pointer_click(
                model,
                shell_rect,
                point,
                conversation_list_view,
                emitted_commands,
            )
        }
        HudWidgetKey::ThreadPane => {
            let _ = (
                terminal_manager,
                focus_state,
                presentation_store,
                view_state,
                agent_list_view,
                conversation_list_view,
                thread_view,
                layout_state,
            );
        }
        _ => {}
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "module hover routing needs retained model state plus the view-models that specific widgets consume"
)]
/// Dispatches hover updates to the addressed HUD module and returns whether its hover state changed.
///
/// Debug toolbar currently ignores hover, while agent-list hover is delegated to its own retained
/// interaction logic.
pub(crate) fn handle_hover(
    module_id: HudWidgetKey,
    model: &mut HudModuleModel,
    shell_rect: HudRect,
    point: Option<Vec2>,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
) -> bool {
    match module_id {
        HudWidgetKey::DebugToolbar | HudWidgetKey::ThreadPane => false,
        HudWidgetKey::AgentList => {
            let _ = (terminal_manager, focus_state, conversation_list_view);
            agent_list::handle_hover(model, shell_rect, point, agent_list_view)
        }
        HudWidgetKey::ConversationList => {
            let _ = (terminal_manager, focus_state, agent_list_view);
            conversation_list::handle_hover(model, shell_rect, point, conversation_list_view)
        }
        _ => false,
    }
}

/// Clears retained hover state for the addressed HUD module.
///
/// This is the counterpart to [`handle_hover`] and again only matters for modules that track hover
/// internally.
pub(crate) fn clear_hover(module_id: HudWidgetKey, model: &mut HudModuleModel) -> bool {
    match module_id {
        HudWidgetKey::DebugToolbar | HudWidgetKey::ThreadPane => false,
        HudWidgetKey::AgentList => agent_list::clear_hover(model),
        HudWidgetKey::ConversationList => conversation_list::clear_hover(model),
        _ => false,
    }
}

/// Dispatches module-body rendering to the addressed module implementation.
///
/// The generic HUD renderer only needs to know which module is being drawn; the module-specific body
/// drawing stays encapsulated behind this router.
pub(crate) fn render_module_content(
    module_id: HudWidgetKey,
    model: &HudModuleModel,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    match module_id {
        HudWidgetKey::DebugToolbar => {
            debug_toolbar::render_content(model, content_rect, painter, inputs)
        }
        HudWidgetKey::AgentList => agent_list::render_content(model, content_rect, painter, inputs),
        HudWidgetKey::ConversationList => {
            conversation_list::render_content(model, content_rect, painter, inputs)
        }
        HudWidgetKey::ThreadPane => {
            thread_pane::render_content(model, content_rect, painter, inputs)
        }
        _ => {}
    }
}

/// Dispatches scroll input to the addressed HUD module.
///
/// Only the agent list currently consumes scroll deltas; the debug toolbar intentionally ignores
/// them.
pub(crate) fn handle_scroll(
    module_id: HudWidgetKey,
    model: &mut HudModuleModel,
    delta_y: f32,
    terminal_manager: &TerminalManager,
    shell_rect: HudRect,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
) {
    match module_id {
        HudWidgetKey::DebugToolbar | HudWidgetKey::ThreadPane => {}
        HudWidgetKey::AgentList => {
            let _ = terminal_manager;
            agent_list::handle_scroll(model, delta_y, agent_list_view.rows.len(), shell_rect.h);
        }
        HudWidgetKey::ConversationList => {
            let _ = terminal_manager;
            conversation_list::handle_scroll(
                model,
                delta_y,
                conversation_list_view.rows.len(),
                shell_rect.h,
            );
        }
        _ => {}
    }
}
