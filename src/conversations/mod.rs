mod persistence;
mod store;
mod tasks;

pub(crate) use store::{ConversationId, ConversationStore, MessageAuthor, MessageDeliveryState};

pub(crate) use tasks::{sync_task_notes_projection, AgentTaskStore, MessageTransportAdapter};

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
