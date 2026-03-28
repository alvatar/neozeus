use crate::{
    conversations::{
        mark_conversations_dirty, AgentTaskStore, ConversationPersistenceState, ConversationStore,
        MessageTransportAdapter,
    },
    ui::ComposerMode,
};

use super::super::{commands::ComposerRequest, session::AppSessionState};
use bevy::window::RequestRedraw;

use super::{send_message, set_task_text};

#[allow(
    clippy::too_many_arguments,
    reason = "composer submit fans out into message or task use cases"
)]
/// Handles submit composer.
pub(crate) fn submit_composer(
    app_session: &mut AppSessionState,
    conversations: &mut ConversationStore,
    conversation_persistence: &mut ConversationPersistenceState,
    tasks: &mut AgentTaskStore,
    runtime_index: &crate::agents::AgentRuntimeIndex,
    terminal_manager: &crate::terminals::TerminalManager,
    transport: &MessageTransportAdapter,
    time: &bevy::prelude::Time,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let Some(session) = app_session.composer.session.clone() else {
        return;
    };
    match session.mode {
        ComposerMode::Message { agent_id } => {
            let body = app_session.composer.message_editor.text.clone();
            if !body.trim().is_empty() {
                send_message(
                    conversations.ensure_conversation(agent_id),
                    agent_id,
                    body,
                    conversations,
                    transport,
                    runtime_index,
                    terminal_manager,
                );
                mark_conversations_dirty(conversation_persistence, Some(time));
            }
            app_session.composer.discard_current_message();
        }
        ComposerMode::TaskEdit { agent_id } => {
            let text = app_session.composer.task_editor.text.clone();
            let _ = set_task_text(agent_id, &text, tasks);
            app_session.composer.close_task_editor();
        }
    }
    redraws.write(RequestRedraw);
}

/// Handles cancel composer.
pub(crate) fn cancel_composer(
    app_session: &mut AppSessionState,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    app_session.composer.cancel_preserving_draft();
    redraws.write(RequestRedraw);
}

/// Opens composer.
pub(crate) fn open_composer(
    request: &ComposerRequest,
    app_session: &mut AppSessionState,
    runtime_index: &crate::agents::AgentRuntimeIndex,
    tasks: &AgentTaskStore,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
    match request.mode {
        ComposerMode::Message { agent_id } => {
            if runtime_index.primary_terminal(agent_id).is_none() {
                return;
            }
            app_session.composer.open_message(agent_id);
        }
        ComposerMode::TaskEdit { agent_id } => {
            if runtime_index.primary_terminal(agent_id).is_none() {
                return;
            }
            let existing = tasks.text(agent_id).unwrap_or_default().to_owned();
            app_session.composer.open_task_editor(agent_id, &existing);
        }
    }
    redraws.write(RequestRedraw);
}
