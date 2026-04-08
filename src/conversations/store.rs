use crate::agents::AgentId;
use bevy::prelude::Resource;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ConversationId(pub(crate) u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct MessageId(u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MessageAuthor {
    User,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MessageDeliveryState {
    Pending,
    Delivered,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MessageRecord {
    pub(crate) id: MessageId,
    pub(crate) conversation_id: ConversationId,
    pub(crate) author: MessageAuthor,
    pub(crate) body: String,
    pub(crate) delivery: MessageDeliveryState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConversationRecord {
    pub(crate) id: ConversationId,
    pub(crate) agent_id: AgentId,
    pub(crate) message_ids: Vec<MessageId>,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationStore {
    next_conversation_id: u64,
    next_message_id: u64,
    pub(super) conversations: BTreeMap<ConversationId, ConversationRecord>,
    pub(super) messages: BTreeMap<MessageId, MessageRecord>,
    pub(super) agent_to_conversation: BTreeMap<AgentId, ConversationId>,
}

impl ConversationStore {
    /// Returns the conversation id for agent when one exists.
    pub(crate) fn conversation_for_agent(&self, agent_id: AgentId) -> Option<ConversationId> {
        self.agent_to_conversation.get(&agent_id).copied()
    }

    /// Ensures conversation exists and returns its identifier.
    pub(crate) fn ensure_conversation(&mut self, agent_id: AgentId) -> ConversationId {
        if let Some(conversation_id) = self.agent_to_conversation.get(&agent_id).copied() {
            return conversation_id;
        }
        let conversation_id = ConversationId(self.next_conversation_id.max(1));
        self.next_conversation_id = conversation_id.0 + 1;
        self.conversations.insert(
            conversation_id,
            ConversationRecord {
                id: conversation_id,
                agent_id,
                message_ids: Vec::new(),
            },
        );
        self.agent_to_conversation.insert(agent_id, conversation_id);
        conversation_id
    }

    /// Appends message.
    pub(crate) fn push_message(
        &mut self,
        conversation_id: ConversationId,
        author: MessageAuthor,
        body: String,
        delivery: MessageDeliveryState,
    ) -> u64 {
        let message_id = MessageId(self.next_message_id.max(1));
        self.next_message_id = message_id.0 + 1;
        self.messages.insert(
            message_id,
            MessageRecord {
                id: message_id,
                conversation_id,
                author,
                body,
                delivery,
            },
        );
        if let Some(conversation) = self.conversations.get_mut(&conversation_id) {
            conversation.message_ids.push(message_id);
        }
        message_id.0
    }

    /// Sets delivery.
    pub(crate) fn set_delivery(&mut self, message_id: u64, delivery: MessageDeliveryState) -> bool {
        let Some(message) = self.messages.get_mut(&MessageId(message_id)) else {
            return false;
        };
        if message.delivery == delivery {
            return false;
        }
        message.delivery = delivery;
        true
    }

    pub(crate) fn remove_agent(&mut self, agent_id: AgentId) -> bool {
        let Some(conversation_id) = self.agent_to_conversation.remove(&agent_id) else {
            return false;
        };
        let Some(conversation) = self.conversations.remove(&conversation_id) else {
            return false;
        };
        for message_id in conversation.message_ids {
            self.messages.remove(&message_id);
        }
        true
    }

    /// Returns cloned message bodies and delivery states for one conversation in append order.
    pub(crate) fn messages_for(
        &self,
        conversation_id: ConversationId,
    ) -> Vec<(String, MessageDeliveryState)> {
        self.conversations
            .get(&conversation_id)
            .into_iter()
            .flat_map(|conversation| conversation.message_ids.iter())
            .filter_map(|message_id| self.messages.get(message_id))
            .map(|message| (message.body.clone(), message.delivery.clone()))
            .collect()
    }
}
