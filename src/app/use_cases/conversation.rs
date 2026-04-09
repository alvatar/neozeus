use crate::{
    agents::{AgentId, AgentRuntimeIndex},
    conversations::{
        ConversationStore, MessageAuthor, MessageDeliveryState, MessageTransportAdapter,
    },
    terminals::TerminalRuntimeSpawner,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutboundMessageSource {
    User,
    Aegis,
}

impl OutboundMessageSource {
    fn author(self) -> MessageAuthor {
        match self {
            Self::User => MessageAuthor::User,
            Self::Aegis => MessageAuthor::Aegis,
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "message send spans domain store, transport adapter, and runtime mapping"
)]
/// Sends one outbound agent message through the canonical conversation/runtime path.
pub(crate) fn send_outbound_message(
    conversation_id: crate::conversations::ConversationId,
    sender: AgentId,
    body: String,
    source: OutboundMessageSource,
    conversations: &mut ConversationStore,
    _transport: &MessageTransportAdapter,
    runtime_index: &AgentRuntimeIndex,
    runtime_spawner: &TerminalRuntimeSpawner,
) -> Result<u64, String> {
    let message_id = conversations.push_message(
        conversation_id,
        source.author(),
        body.clone(),
        MessageDeliveryState::Pending,
    );
    let Some(session_name) = runtime_index.session_name(sender) else {
        let error = "no terminal linked".to_owned();
        conversations.set_delivery(message_id, MessageDeliveryState::Failed(error.clone()));
        return Err(error);
    };
    match runtime_spawner.send_command(
        session_name,
        crate::terminals::TerminalCommand::SendCommand(body),
    ) {
        Ok(()) => {
            conversations.set_delivery(message_id, MessageDeliveryState::Delivered);
            Ok(message_id)
        }
        Err(error) => {
            conversations.set_delivery(message_id, MessageDeliveryState::Failed(error.clone()));
            Err(error)
        }
    }
}

/// Handles send message.
pub(crate) fn send_message(
    conversation_id: crate::conversations::ConversationId,
    sender: AgentId,
    body: String,
    conversations: &mut ConversationStore,
    transport: &MessageTransportAdapter,
    runtime_index: &AgentRuntimeIndex,
    runtime_spawner: &TerminalRuntimeSpawner,
) {
    let _ = send_outbound_message(
        conversation_id,
        sender,
        body,
        OutboundMessageSource::User,
        conversations,
        transport,
        runtime_index,
        runtime_spawner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        conversations::MessageTransportAdapter,
        terminals::TerminalCommand,
        tests::{fake_runtime_spawner, FakeDaemonClient},
    };
    use std::sync::Arc;

    #[test]
    fn aegis_send_uses_canonical_runtime_message_path() {
        let client = Arc::new(FakeDaemonClient::default());
        client.set_session_runtime(
            "neozeus-session-a",
            crate::terminals::TerminalRuntimeState::running("ready"),
        );
        let runtime_spawner = fake_runtime_spawner(client.clone());
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(
            AgentId(1),
            crate::terminals::TerminalId(1),
            "neozeus-session-a".into(),
            None,
        );
        let mut conversations = ConversationStore::default();
        let conversation_id = conversations.ensure_conversation(AgentId(1));

        let message_id = send_outbound_message(
            conversation_id,
            AgentId(1),
            "continue cleanly".into(),
            OutboundMessageSource::Aegis,
            &mut conversations,
            &MessageTransportAdapter,
            &runtime_index,
            &runtime_spawner,
        )
        .expect("send should succeed");

        assert_eq!(message_id, 1);
        assert_eq!(
            conversations.message_authors_for(conversation_id),
            vec![MessageAuthor::Aegis]
        );
        assert_eq!(
            conversations.messages_for(conversation_id),
            vec![("continue cleanly".into(), MessageDeliveryState::Delivered)]
        );
        assert_eq!(
            client.sent_commands.lock().unwrap().clone(),
            vec![(
                "neozeus-session-a".into(),
                TerminalCommand::SendCommand("continue cleanly".into())
            )]
        );
    }

    #[test]
    fn outbound_send_failure_marks_message_failed() {
        let client = Arc::new(FakeDaemonClient::default());
        client.set_session_runtime(
            "neozeus-session-a",
            crate::terminals::TerminalRuntimeState::running("ready"),
        );
        *client.fail_send.lock().unwrap() = true;
        let runtime_spawner = fake_runtime_spawner(client);
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(
            AgentId(1),
            crate::terminals::TerminalId(1),
            "neozeus-session-a".into(),
            None,
        );
        let mut conversations = ConversationStore::default();
        let conversation_id = conversations.ensure_conversation(AgentId(1));

        let error = send_outbound_message(
            conversation_id,
            AgentId(1),
            "continue cleanly".into(),
            OutboundMessageSource::Aegis,
            &mut conversations,
            &MessageTransportAdapter,
            &runtime_index,
            &runtime_spawner,
        )
        .expect_err("send should fail");

        assert_eq!(error, "send failed");
        assert_eq!(
            conversations.messages_for(conversation_id),
            vec![(
                "continue cleanly".into(),
                MessageDeliveryState::Failed("send failed".into())
            )]
        );
    }
}
