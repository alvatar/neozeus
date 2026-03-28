mod persistence;

use crate::{
    agents::{AgentId, AgentRuntimeIndex},
    terminals::{mark_terminal_notes_dirty, TerminalNotesState},
};
use bevy::prelude::{Res, ResMut, Resource, Time};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ConversationId(pub(crate) u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct MessageId(pub(crate) u64);

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
pub(crate) struct MessageRecord {
    pub(crate) id: MessageId,
    pub(crate) conversation_id: ConversationId,
    pub(crate) author: MessageAuthor,
    pub(crate) body: String,
    pub(crate) delivery: MessageDeliveryState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationRecord {
    pub(crate) id: ConversationId,
    pub(crate) agent_id: AgentId,
    pub(crate) message_ids: Vec<MessageId>,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationStore {
    next_conversation_id: u64,
    next_message_id: u64,
    pub(crate) conversations: BTreeMap<ConversationId, ConversationRecord>,
    pub(crate) messages: BTreeMap<MessageId, MessageRecord>,
    pub(crate) agent_to_conversation: BTreeMap<AgentId, ConversationId>,
}

impl ConversationStore {
    pub(crate) fn conversation_for_agent(&self, agent_id: AgentId) -> Option<ConversationId> {
        self.agent_to_conversation.get(&agent_id).copied()
    }

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

    pub(crate) fn push_message(
        &mut self,
        conversation_id: ConversationId,
        author: MessageAuthor,
        body: String,
        delivery: MessageDeliveryState,
    ) -> MessageId {
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
        message_id
    }

    pub(crate) fn set_delivery(
        &mut self,
        message_id: MessageId,
        delivery: MessageDeliveryState,
    ) -> bool {
        let Some(message) = self.messages.get_mut(&message_id) else {
            return false;
        };
        if message.delivery == delivery {
            return false;
        }
        message.delivery = delivery;
        true
    }

    pub(crate) fn messages_for(&self, conversation_id: ConversationId) -> Vec<&MessageRecord> {
        self.conversations
            .get(&conversation_id)
            .into_iter()
            .flat_map(|conversation| conversation.message_ids.iter())
            .filter_map(|message_id| self.messages.get(message_id))
            .collect()
    }
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentTaskStore {
    tasks_by_agent: BTreeMap<AgentId, String>,
}

impl AgentTaskStore {
    pub(crate) fn text(&self, agent_id: AgentId) -> Option<&str> {
        self.tasks_by_agent.get(&agent_id).map(String::as_str)
    }

    pub(crate) fn remove_agent(&mut self, agent_id: AgentId) -> bool {
        self.tasks_by_agent.remove(&agent_id).is_some()
    }

    pub(crate) fn set_text(&mut self, agent_id: AgentId, text: &str) -> bool {
        let trimmed = text.trim_end();
        if trimmed.is_empty() {
            return self.tasks_by_agent.remove(&agent_id).is_some();
        }
        match self.tasks_by_agent.get_mut(&agent_id) {
            Some(existing) if existing == trimmed => false,
            Some(existing) => {
                existing.clear();
                existing.push_str(trimmed);
                true
            }
            None => {
                self.tasks_by_agent.insert(agent_id, trimmed.to_owned());
                true
            }
        }
    }

    pub(crate) fn append_task(&mut self, agent_id: AgentId, text: &str) -> bool {
        let Some(task_entry) = crate::terminals::task_entry_from_text(text) else {
            return false;
        };
        let existing = self
            .text(agent_id)
            .unwrap_or_default()
            .trim_end()
            .to_owned();
        let updated = if existing.is_empty() {
            task_entry
        } else {
            format!("{existing}\n{task_entry}")
        };
        self.set_text(agent_id, &updated)
    }

    pub(crate) fn prepend_task(&mut self, agent_id: AgentId, text: &str) -> bool {
        let Some(task_entry) = crate::terminals::task_entry_from_text(text) else {
            return false;
        };
        let existing = self
            .text(agent_id)
            .unwrap_or_default()
            .trim_end()
            .to_owned();
        let updated = if existing.is_empty() {
            task_entry
        } else {
            format!("{task_entry}\n{existing}")
        };
        self.set_text(agent_id, &updated)
    }

    pub(crate) fn clear_done(&mut self, agent_id: AgentId) -> bool {
        let Some(text) = self.text(agent_id) else {
            return false;
        };
        let (updated, removed) = crate::terminals::clear_done_tasks(text);
        removed != 0 && self.set_text(agent_id, &updated)
    }

    pub(crate) fn consume_next(&mut self, agent_id: AgentId) -> Option<String> {
        let text = self.text(agent_id)?;
        let (message, updated) = crate::terminals::extract_next_task(text)?;
        if !message.trim().is_empty() {
            let _ = self.set_text(agent_id, &updated);
            return Some(message);
        }
        None
    }
}

#[derive(Resource, Default, Clone, Debug)]
pub(crate) struct MessageTransportAdapter;

pub(crate) fn sync_task_notes_projection(
    time: Res<Time>,
    runtime_index: Res<AgentRuntimeIndex>,
    task_store: Res<AgentTaskStore>,
    mut notes_state: ResMut<TerminalNotesState>,
) {
    let mut changed = false;
    for (agent_id, link) in &runtime_index.agent_to_runtime {
        let Some(session_name) = link.session_name.as_deref() else {
            continue;
        };
        let next_text = task_store.text(*agent_id).unwrap_or_default();
        changed |= notes_state.set_note_text(session_name, next_text);
    }
    if changed {
        mark_terminal_notes_dirty(&mut notes_state, Some(&time));
    }
}

pub(crate) use persistence::{
    load_persisted_conversations_from, mark_conversations_dirty, resolve_conversations_path,
    restore_persisted_conversations, save_conversations_if_dirty, ConversationPersistenceState,
};

#[cfg(test)]
pub(crate) use persistence::{
    build_persisted_conversations, parse_persisted_conversations, resolve_conversations_path_with,
    serialize_persisted_conversations,
};

#[cfg(test)]
mod tests;
