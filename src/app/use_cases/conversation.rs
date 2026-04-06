use crate::{
    agents::{AgentId, AgentRuntimeIndex},
    conversations::{
        ConversationStore, MessageAuthor, MessageDeliveryState, MessageTransportAdapter,
    },
    terminals::TerminalRuntimeSpawner,
};

#[allow(
    clippy::too_many_arguments,
    reason = "message send spans domain store, transport adapter, and runtime mapping"
)]
/// Handles send message.
pub(crate) fn send_message(
    conversation_id: crate::conversations::ConversationId,
    sender: AgentId,
    body: String,
    conversations: &mut ConversationStore,
    _transport: &MessageTransportAdapter,
    runtime_index: &AgentRuntimeIndex,
    runtime_spawner: &TerminalRuntimeSpawner,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let message_id = conversations.push_message(
        conversation_id,
        MessageAuthor::User,
        body.clone(),
        MessageDeliveryState::Pending,
    );
    let Some(session_name) = runtime_index.session_name(sender) else {
        conversations.set_delivery(
            message_id,
            MessageDeliveryState::Failed("no terminal linked".into()),
        );
        return;
    };
    match runtime_spawner.send_command(
        session_name,
        crate::terminals::TerminalCommand::SendCommand(body),
    ) {
        Ok(()) => {
            conversations.set_delivery(message_id, MessageDeliveryState::Delivered);
        }
        Err(error) => {
            conversations.set_delivery(message_id, MessageDeliveryState::Failed(error));
        }
    }
}
