mod agent_list;
mod conversation_list;
mod debug_toolbar;
mod thread_pane;

#[cfg(test)]
use crate::hud::ThreadView;
use crate::{
    app::AppCommand,
    hud::{
        render::{HudPainter, HudRenderInputs},
        AgentListUiState, AgentListView, ConversationListUiState, ConversationListView,
        DebugToolbarView, HudLayoutState, HudRect, HudWidgetKey,
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
pub(crate) use debug_toolbar::legacy_debug_toolbar_buttons as debug_toolbar_buttons;

#[allow(
    clippy::too_many_arguments,
    reason = "module click routing needs shell geometry, derived widget data, and command output together"
)]
pub(crate) fn handle_pointer_click(
    module_id: HudWidgetKey,
    shell_rect: HudRect,
    point: Vec2,
    agent_list_state: &AgentListUiState,
    conversation_list_state: &ConversationListUiState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
    debug_toolbar_view: &DebugToolbarView,
    layout_state: &HudLayoutState,
    emitted_commands: &mut Vec<AppCommand>,
) {
    match module_id {
        HudWidgetKey::DebugToolbar => debug_toolbar::handle_pointer_click(
            shell_rect,
            point,
            debug_toolbar_view,
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

pub(crate) fn handle_hover(
    module_id: HudWidgetKey,
    shell_rect: HudRect,
    point: Option<Vec2>,
    agent_list_state: &mut AgentListUiState,
    conversation_list_state: &mut ConversationListUiState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
) -> bool {
    match module_id {
        HudWidgetKey::DebugToolbar | HudWidgetKey::ThreadPane => false,
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

pub(crate) fn clear_hover(
    module_id: HudWidgetKey,
    agent_list_state: &mut AgentListUiState,
    conversation_list_state: &mut ConversationListUiState,
) -> bool {
    match module_id {
        HudWidgetKey::DebugToolbar | HudWidgetKey::ThreadPane => false,
        HudWidgetKey::AgentList => agent_list::clear_hover(agent_list_state),
        HudWidgetKey::ConversationList => conversation_list::clear_hover(conversation_list_state),
        _ => false,
    }
}

pub(crate) fn render_module_content(
    module_id: HudWidgetKey,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
    agent_list_state: &AgentListUiState,
    conversation_list_state: &ConversationListUiState,
) {
    match module_id {
        HudWidgetKey::DebugToolbar => debug_toolbar::render_content(content_rect, painter, inputs),
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

pub(crate) fn handle_scroll(
    module_id: HudWidgetKey,
    delta_y: f32,
    shell_rect: HudRect,
    agent_list_state: &mut AgentListUiState,
    conversation_list_state: &mut ConversationListUiState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
) {
    match module_id {
        HudWidgetKey::DebugToolbar | HudWidgetKey::ThreadPane => {}
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_pointer_click_legacy(
    module_id: HudWidgetKey,
    model: &crate::hud::HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    _terminal_manager: &crate::terminals::TerminalManager,
    _focus_state: &crate::terminals::TerminalFocusState,
    _presentation_store: &crate::terminals::TerminalPresentationStore,
    _view_state: &crate::terminals::TerminalViewState,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
    _thread_view: &ThreadView,
    layout_state: &HudLayoutState,
    emitted_commands: &mut Vec<crate::hud::HudIntent>,
) {
    let mut app_commands = Vec::new();
    let agent_list_state = match model {
        crate::hud::HudModuleModel::AgentList(state) => state.clone(),
        _ => AgentListUiState::default(),
    };
    let conversation_list_state = match model {
        crate::hud::HudModuleModel::ConversationList(state) => state.clone(),
        _ => ConversationListUiState::default(),
    };
    handle_pointer_click(
        module_id,
        shell_rect,
        point,
        &agent_list_state,
        &conversation_list_state,
        agent_list_view,
        conversation_list_view,
        &DebugToolbarView::default(),
        layout_state,
        &mut app_commands,
    );
    emitted_commands.extend(app_commands.into_iter().flat_map(|command| match command {
        AppCommand::Agent(crate::app::AgentCommand::SpawnTerminal) => {
            vec![crate::hud::HudIntent::SpawnTerminal]
        }
        AppCommand::Terminal(crate::app::TerminalCommand::SendCommandToActive { command }) => {
            vec![crate::hud::HudIntent::SendActiveTerminalCommand(command)]
        }
        AppCommand::Agent(crate::app::AgentCommand::Focus(_)) => Vec::new(),
        AppCommand::Agent(crate::app::AgentCommand::Inspect(agent_id)) => {
            let terminal_id = agent_list_view
                .rows
                .iter()
                .find(|row| row.agent_id == agent_id)
                .and_then(|row| row.terminal_id)
                .or_else(|| {
                    conversation_list_view
                        .rows
                        .iter()
                        .find(|row| row.agent_id == agent_id)
                        .and_then(|row| row.terminal_id)
                });
            terminal_id
                .map(|terminal_id| {
                    vec![
                        crate::hud::HudIntent::FocusTerminal(terminal_id),
                        crate::hud::HudIntent::HideAllButTerminal(terminal_id),
                    ]
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }));
}

#[cfg(test)]
pub(crate) fn handle_scroll_legacy(
    module_id: HudWidgetKey,
    model: &mut crate::hud::HudModuleModel,
    delta_y: f32,
    _terminal_manager: &crate::terminals::TerminalManager,
    shell_rect: HudRect,
    agent_list_view: &AgentListView,
    conversation_list_view: &ConversationListView,
) {
    match (module_id, model) {
        (HudWidgetKey::AgentList, crate::hud::HudModuleModel::AgentList(state)) => {
            handle_scroll(
                module_id,
                delta_y,
                shell_rect,
                state,
                &mut ConversationListUiState::default(),
                agent_list_view,
                conversation_list_view,
            );
        }
        (HudWidgetKey::ConversationList, crate::hud::HudModuleModel::ConversationList(state)) => {
            handle_scroll(
                module_id,
                delta_y,
                shell_rect,
                &mut AgentListUiState::default(),
                state,
                agent_list_view,
                conversation_list_view,
            );
        }
        _ => {}
    }
}
