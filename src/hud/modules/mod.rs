mod agent_list;
mod conversation_list;
mod info_bar;
mod thread_pane;

use crate::app::AppCommand;

use super::{
    render::{HudPainter, HudRenderInputs},
    state::{AgentListUiState, ConversationListUiState, HudLayoutState, HudRect},
    view_models::{AgentListView, ConversationListView, InfoBarView},
    widgets::HudWidgetKey,
};
use bevy::prelude::Vec2;

pub(in crate::hud) use agent_list::agent_rows;
pub(crate) use agent_list::{
    agent_row_rect, reorder_target_index, row_at_point, row_main_rect, selected_text_for_rows,
    text_row_at_point, AgentListRowSection, AGENT_LIST_BLOOM_RED_B, AGENT_LIST_BLOOM_RED_G,
    AGENT_LIST_BLOOM_RED_R, AGENT_LIST_BORDER_ORANGE_B, AGENT_LIST_BORDER_ORANGE_G,
    AGENT_LIST_BORDER_ORANGE_R,
};
pub(in crate::hud) use info_bar::{INFO_BAR_BACKGROUND, INFO_BAR_BORDER};

#[allow(
    clippy::too_many_arguments,
    reason = "module click routing needs shell geometry, derived widget data, and command output together"
)]
/// Handles pointer click.
pub(crate) fn handle_pointer_click(
    module_id: HudWidgetKey,
    shell_rect: HudRect,
    point: Vec2,
    agent_list_state: &AgentListUiState,
    conversation_list_state: &ConversationListUiState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
    info_bar_view: &InfoBarView,
    layout_state: &HudLayoutState,
    emitted_commands: &mut Vec<AppCommand>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    match module_id {
        HudWidgetKey::InfoBar => info_bar::handle_pointer_click(
            shell_rect,
            point,
            info_bar_view,
            layout_state,
            emitted_commands,
        ),
        HudWidgetKey::AgentList => agent_list::handle_pointer_click(
            agent_list_state,
            shell_rect,
            point,
            agent_list_view,
            emitted_commands,
        ),
        HudWidgetKey::ConversationList => conversation_list::handle_pointer_click(
            conversation_list_state,
            shell_rect,
            point,
            conversation_list_view,
            emitted_commands,
        ),
        HudWidgetKey::ThreadPane => {}
        _ => {}
    }
}

/// Handles hover.
pub(crate) fn handle_hover(
    module_id: HudWidgetKey,
    shell_rect: HudRect,
    point: Option<Vec2>,
    agent_list_state: &mut AgentListUiState,
    conversation_list_state: &mut ConversationListUiState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
) -> bool {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    match module_id {
        HudWidgetKey::InfoBar | HudWidgetKey::ThreadPane => false,
        HudWidgetKey::AgentList => {
            agent_list::handle_hover(agent_list_state, shell_rect, point, agent_list_view)
        }
        HudWidgetKey::ConversationList => conversation_list::handle_hover(
            conversation_list_state,
            shell_rect,
            point,
            conversation_list_view,
        ),
        _ => false,
    }
}

/// Clears hover.
pub(crate) fn clear_hover(
    module_id: HudWidgetKey,
    agent_list_state: &mut AgentListUiState,
    conversation_list_state: &mut ConversationListUiState,
) -> bool {
    match module_id {
        HudWidgetKey::InfoBar | HudWidgetKey::ThreadPane => false,
        HudWidgetKey::AgentList => agent_list::clear_hover(agent_list_state),
        HudWidgetKey::ConversationList => conversation_list::clear_hover(conversation_list_state),
        _ => false,
    }
}

/// Renders module content.
pub(crate) fn render_module_content(
    module_id: HudWidgetKey,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
    agent_list_state: &AgentListUiState,
    conversation_list_state: &ConversationListUiState,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    match module_id {
        HudWidgetKey::InfoBar => info_bar::render_content(content_rect, painter, inputs),
        HudWidgetKey::AgentList => {
            agent_list::render_content(agent_list_state, content_rect, painter, inputs)
        }
        HudWidgetKey::ConversationList => conversation_list::render_content(
            conversation_list_state,
            content_rect,
            painter,
            inputs,
        ),
        HudWidgetKey::ThreadPane => thread_pane::render_content(content_rect, painter, inputs),
        _ => {}
    }
}

/// Handles scroll.
pub(crate) fn handle_scroll(
    module_id: HudWidgetKey,
    delta_y: f32,
    shell_rect: HudRect,
    agent_list_state: &mut AgentListUiState,
    conversation_list_state: &mut ConversationListUiState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    match module_id {
        HudWidgetKey::InfoBar | HudWidgetKey::ThreadPane => {}
        HudWidgetKey::AgentList => {
            agent_list::handle_scroll(
                agent_list_state,
                delta_y,
                agent_list_view.rows.len(),
                shell_rect.h,
            );
        }
        HudWidgetKey::ConversationList => {
            conversation_list::handle_scroll(
                conversation_list_state,
                delta_y,
                conversation_list_view.rows.len(),
                shell_rect.h,
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
