use crate::{
    composer::ComposerMode,
    conversations::{
        mark_conversations_dirty, AgentTaskStore, ConversationPersistenceState, ConversationStore,
    },
    hud::HudInputCaptureState,
};

use super::super::{commands::ComposerRequest, session::AppSessionState};
use bevy::window::RequestRedraw;

use super::{send_message, set_task_text};

pub(crate) struct ComposerSubmitContext<'a, 'w> {
    pub(crate) app_session: &'a mut AppSessionState,
    pub(crate) conversations: &'a mut ConversationStore,
    pub(crate) conversation_persistence: &'a mut ConversationPersistenceState,
    pub(crate) tasks: &'a mut AgentTaskStore,
    pub(crate) runtime_index: &'a crate::agents::AgentRuntimeIndex,
    pub(crate) runtime_spawner: &'a crate::terminals::TerminalRuntimeSpawner,
    pub(crate) time: &'a bevy::prelude::Time,
    pub(crate) redraws: &'a mut bevy::prelude::MessageWriter<'w, RequestRedraw>,
}

/// Handles submit composer.
pub(crate) fn submit_composer(ctx: &mut ComposerSubmitContext<'_, '_>) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let Some(session) = ctx.app_session.composer.session.clone() else {
        return;
    };
    match session.mode {
        ComposerMode::Message { agent_id } => {
            let body = ctx.app_session.composer.message_editor.text.clone();
            if !body.trim().is_empty() {
                send_message(
                    ctx.conversations.ensure_conversation(agent_id),
                    agent_id,
                    body,
                    ctx.conversations,
                    ctx.runtime_index,
                    ctx.runtime_spawner,
                );
                mark_conversations_dirty(ctx.conversation_persistence, Some(ctx.time));
            }
            ctx.app_session.composer.discard_current_message();
        }
        ComposerMode::TaskEdit { agent_id } => {
            let text = ctx.app_session.composer.task_editor.text.clone();
            let _ = set_task_text(agent_id, &text, ctx.tasks);
            ctx.app_session.composer.close_task_editor();
        }
    }
    ctx.redraws.write(RequestRedraw);
}

/// Handles cancel composer.
pub(crate) fn cancel_composer(
    app_session: &mut AppSessionState,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    app_session.composer.cancel_preserving_draft();
    redraws.write(RequestRedraw);
}

/// Clears any direct terminal input capture and closes the active composer/editor session.
pub(crate) fn clear_composer_and_direct_input(
    app_session: &mut AppSessionState,
    input_capture: &mut HudInputCaptureState,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    app_session.composer.cancel_preserving_draft();
    input_capture.close_direct_terminal_input();
    redraws.write(RequestRedraw);
}

/// Opens composer.
pub(crate) fn open_composer(
    request: &ComposerRequest,
    app_session: &mut AppSessionState,
    input_capture: &mut HudInputCaptureState,
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
            input_capture.close_direct_terminal_input();
            app_session.composer.open_message(agent_id);
        }
        ComposerMode::TaskEdit { agent_id } => {
            if runtime_index.primary_terminal(agent_id).is_none() {
                return;
            }
            input_capture.close_direct_terminal_input();
            let existing = tasks.text(agent_id).unwrap_or_default().to_owned();
            app_session.composer.open_task_editor(agent_id, &existing);
        }
    }
    redraws.write(RequestRedraw);
}
