use crate::{
    agents::{AgentId, AgentRuntimeIndex},
    conversations::{
        ConversationStore, MessageAuthor, MessageDeliveryState, MessageTransportAdapter,
    },
    terminals::TerminalManager,
};

use super::send_terminal_command;

#[allow(
    clippy::too_many_arguments,
    reason = "message send spans domain store, transport adapter, and runtime mapping"
)]
pub(crate) fn send_message(
    conversation_id: crate::conversations::ConversationId,
    sender: AgentId,
    body: String,
    conversations: &mut ConversationStore,
    _transport: &MessageTransportAdapter,
    runtime_index: &AgentRuntimeIndex,
    terminal_manager: &TerminalManager,
) {
    let message_id = conversations.push_message(
        conversation_id,
        MessageAuthor::User,
        body.clone(),
        MessageDeliveryState::Pending,
    );
    let Some(terminal_id) = runtime_index.primary_terminal(sender) else {
        conversations.set_delivery(
            message_id,
            MessageDeliveryState::Failed("no terminal linked".into()),
        );
        return;
    };
    send_terminal_command(terminal_id, &body, terminal_manager);
    conversations.set_delivery(message_id, MessageDeliveryState::Delivered);
}
