use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex},
    app::AppSessionState,
    conversations::{AgentTaskStore, ConversationStore, MessageDeliveryState},
    terminals::TerminalManager,
};
use bevy::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentListRowView {
    pub(crate) agent_id: AgentId,
    pub(crate) terminal_id: Option<crate::terminals::TerminalId>,
    pub(crate) label: String,
    pub(crate) focused: bool,
    pub(crate) has_tasks: bool,
    pub(crate) interactive: bool,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentListView {
    pub(crate) rows: Vec<AgentListRowView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationListRowView {
    pub(crate) agent_id: AgentId,
    pub(crate) terminal_id: Option<crate::terminals::TerminalId>,
    pub(crate) conversation_id: crate::conversations::ConversationId,
    pub(crate) label: String,
    pub(crate) message_count: usize,
    pub(crate) selected: bool,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationListView {
    pub(crate) rows: Vec<ConversationListRowView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ThreadMessageView {
    pub(crate) body: String,
    pub(crate) delivered: bool,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ThreadView {
    pub(crate) agent_id: Option<AgentId>,
    pub(crate) messages: Vec<ThreadMessageView>,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerView {
    pub(crate) title: Option<String>,
    pub(crate) text: String,
    pub(crate) visible: bool,
}

#[allow(
    clippy::too_many_arguments,
    reason = "view-model derivation reads the authoritative stores and writes the derived UI projections"
)]
pub(crate) fn sync_hud_view_models(
    agent_catalog: Res<AgentCatalog>,
    runtime_index: Res<AgentRuntimeIndex>,
    app_session: Res<AppSessionState>,
    terminal_manager: Res<TerminalManager>,
    task_store: Res<AgentTaskStore>,
    conversations: Res<ConversationStore>,
    mut agent_list: ResMut<AgentListView>,
    mut conversation_list: ResMut<ConversationListView>,
    mut thread_view: ResMut<ThreadView>,
    mut composer_view: ResMut<ComposerView>,
) {
    agent_list.rows = agent_catalog
        .iter()
        .map(|(agent_id, record)| {
            let terminal_id = runtime_index.primary_terminal(agent_id);
            let interactive = terminal_id
                .and_then(|terminal_id| terminal_manager.get(terminal_id))
                .is_some_and(|terminal| terminal.snapshot.runtime.is_interactive());
            AgentListRowView {
                agent_id,
                terminal_id,
                label: record.label.clone(),
                focused: app_session.active_agent == Some(agent_id),
                has_tasks: task_store
                    .text(agent_id)
                    .is_some_and(|text| !text.trim().is_empty()),
                interactive,
            }
        })
        .collect();

    conversation_list.rows = agent_catalog
        .iter()
        .filter_map(|(agent_id, record)| {
            let conversation_id = conversations.conversation_for_agent(agent_id)?;
            Some(ConversationListRowView {
                agent_id,
                terminal_id: runtime_index.primary_terminal(agent_id),
                conversation_id,
                label: record.label.clone(),
                message_count: conversations.messages_for(conversation_id).len(),
                selected: app_session.active_agent == Some(agent_id),
            })
        })
        .collect();

    thread_view.agent_id = app_session.active_agent;
    thread_view.messages = app_session
        .active_agent
        .and_then(|agent_id| conversations.conversation_for_agent(agent_id))
        .map(|conversation_id| {
            conversations
                .messages_for(conversation_id)
                .into_iter()
                .map(|message| ThreadMessageView {
                    body: message.body.clone(),
                    delivered: matches!(message.delivery, MessageDeliveryState::Delivered),
                })
                .collect()
        })
        .unwrap_or_default();

    composer_view.visible = app_session.composer.session.is_some();
    composer_view.title = app_session
        .composer
        .session
        .as_ref()
        .map(|session| match session.mode {
            crate::ui::ComposerMode::Message { agent_id } => format!(
                "Message {}",
                agent_catalog
                    .agents
                    .get(&agent_id)
                    .map(|record| record.label.as_str())
                    .unwrap_or("agent")
            ),
            crate::ui::ComposerMode::TaskEdit { agent_id } => format!(
                "Tasks {}",
                agent_catalog
                    .agents
                    .get(&agent_id)
                    .map(|record| record.label.as_str())
                    .unwrap_or("agent")
            ),
        });
    composer_view.text = if app_session.composer.message_editor.visible {
        app_session.composer.message_editor.text.clone()
    } else if app_session.composer.task_editor.visible {
        app_session.composer.task_editor.text.clone()
    } else {
        String::new()
    };
}

#[cfg(test)]
mod tests;
