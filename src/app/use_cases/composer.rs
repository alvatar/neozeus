use crate::{
    app::AppSessionState,
    conversations::{AgentTaskStore, ConversationStore, MessageTransportAdapter},
    ui::ComposerMode,
};
use bevy::{prelude::Time, window::RequestRedraw};

use super::{send_message, set_task_text};

#[allow(
    clippy::too_many_arguments,
    reason = "composer submit fans out into message or task use cases"
)]
pub(crate) fn submit_composer(
    app_session: &mut AppSessionState,
    conversations: &mut ConversationStore,
    tasks: &mut AgentTaskStore,
    notes_state: &mut crate::terminals::TerminalNotesState,
    runtime_index: &crate::agents::AgentRuntimeIndex,
    terminal_manager: &crate::terminals::TerminalManager,
    transport: &MessageTransportAdapter,
    time: &Time,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    let Some(session) = app_session.composer.session.clone() else {
        return;
    };
    match session.mode {
        ComposerMode::Message { agent_id } => {
            let body = app_session.composer.message_editor.text.clone();
            if !body.trim().is_empty() {
                let command = crate::app::ConversationCommand::SendMessage {
                    conversation_id: conversations.ensure_conversation(agent_id),
                    sender: agent_id,
                    body,
                };
                if let crate::app::ConversationCommand::SendMessage {
                    conversation_id,
                    sender,
                    body,
                } = command
                {
                    send_message(
                        conversation_id,
                        sender,
                        body,
                        conversations,
                        transport,
                        runtime_index,
                        terminal_manager,
                    );
                }
            }
            app_session.composer.discard_current_message();
        }
        ComposerMode::TaskEdit { agent_id } => {
            let text = app_session.composer.task_editor.text.clone();
            let _ = set_task_text(agent_id, &text, tasks, notes_state, runtime_index, time);
            app_session.composer.close_task_editor();
        }
    }
    redraws.write(RequestRedraw);
}

pub(crate) fn cancel_composer(
    app_session: &mut AppSessionState,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    app_session.composer.cancel_preserving_draft();
    redraws.write(RequestRedraw);
}

pub(crate) fn open_composer(
    request: &crate::app::ComposerRequest,
    app_session: &mut AppSessionState,
    runtime_index: &crate::agents::AgentRuntimeIndex,
    tasks: &AgentTaskStore,
    notes_state: &crate::terminals::TerminalNotesState,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    match request.mode {
        ComposerMode::Message { agent_id } => {
            let Some(terminal_id) = runtime_index.primary_terminal(agent_id) else {
                return;
            };
            app_session.composer.open_message(agent_id, terminal_id);
        }
        ComposerMode::TaskEdit { agent_id } => {
            let Some(terminal_id) = runtime_index.primary_terminal(agent_id) else {
                return;
            };
            let existing = tasks
                .text(agent_id)
                .map(str::to_owned)
                .or_else(|| {
                    runtime_index
                        .session_name(agent_id)
                        .and_then(|session_name| notes_state.note_text(session_name))
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            app_session
                .composer
                .open_task_editor(agent_id, terminal_id, &existing);
        }
    }
    redraws.write(RequestRedraw);
}
