mod persistence;
mod store;
mod tasks;

pub(crate) use store::{ConversationId, ConversationStore, MessageAuthor, MessageDeliveryState};

pub(crate) use tasks::{sync_task_notes_projection, AgentTaskStore, MessageTransportAdapter};

pub(crate) use persistence::{
    mark_conversations_dirty, resolve_conversations_path,
    restore_persisted_conversations_from_path, save_conversations_if_dirty,
    ConversationPersistenceState,
};

#[cfg(test)]
mod tests;
