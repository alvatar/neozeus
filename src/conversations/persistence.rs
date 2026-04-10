use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    shared::{
        persistence::{resolve_state_path_with, write_file_atomically},
        text_escape::{quote_escaped_string, unquote_escaped_string, BASIC_QUOTED_STRING_ESCAPES},
    },
};

use super::{ConversationStore, MessageAuthor, MessageDeliveryState};
use bevy::prelude::*;
use std::{env, fs, path::PathBuf};

const CONVERSATIONS_FILENAME: &str = "conversations.v1";
const CONVERSATIONS_VERSION_V1: &str = "version 1";
const CONVERSATIONS_VERSION_V2: &str = "version 2";
const CONVERSATIONS_SAVE_DEBOUNCE_SECS: f32 = 0.3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PersistedConversationMessage {
    pub(crate) author: MessageAuthor,
    pub(crate) body: String,
    pub(crate) delivery: MessageDeliveryState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PersistedConversationRecord {
    pub(crate) agent_uid: Option<String>,
    pub(crate) legacy_session_name: Option<String>,
    pub(crate) messages: Vec<PersistedConversationMessage>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct PersistedConversations {
    pub(crate) conversations: Vec<PersistedConversationRecord>,
}

#[derive(Resource, Default)]
pub(crate) struct ConversationPersistenceState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
}

impl ConversationPersistenceState {
    pub(crate) fn clear_runtime_state(&mut self) {
        self.dirty_since_secs = None;
    }
}

/// Resolves conversations path with.
pub(crate) fn resolve_conversations_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
) -> Option<PathBuf> {
    resolve_state_path_with(
        xdg_state_home,
        home,
        xdg_config_home,
        "neozeus",
        CONVERSATIONS_FILENAME,
    )
}

/// Resolves conversations path.
pub(crate) fn resolve_conversations_path() -> Option<PathBuf> {
    resolve_conversations_path_with(
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
}

/// Escapes one persisted conversation field for serialization.
fn quote(value: &str) -> String {
    quote_escaped_string(value, BASIC_QUOTED_STRING_ESCAPES)
}

/// Unescapes one persisted conversation field after parsing.
fn unquote(value: &str) -> Option<String> {
    unquote_escaped_string(value, BASIC_QUOTED_STRING_ESCAPES)
}

fn author_code(author: &MessageAuthor) -> &'static str {
    match author {
        MessageAuthor::User => "user",
        MessageAuthor::Aegis => "aegis",
    }
}

fn parse_author(code: &str) -> Option<MessageAuthor> {
    match code {
        "user" => Some(MessageAuthor::User),
        "aegis" => Some(MessageAuthor::Aegis),
        _ => None,
    }
}

/// Returns the persisted single-field code for a delivery state.
fn delivery_code(delivery: &MessageDeliveryState) -> &'static str {
    match delivery {
        MessageDeliveryState::Pending => "pending",
        MessageDeliveryState::Delivered => "delivered",
        MessageDeliveryState::Failed(_) => "failed",
    }
}

/// Parses one persisted delivery code back into a delivery state.
fn parse_delivery(code: &str, error: Option<String>) -> Option<MessageDeliveryState> {
    match code {
        "pending" => Some(MessageDeliveryState::Pending),
        "delivered" => Some(MessageDeliveryState::Delivered),
        "failed" => Some(MessageDeliveryState::Failed(error.unwrap_or_default())),
        _ => None,
    }
}

/// Serializes persisted conversations.
pub(super) fn serialize_persisted_conversations(persisted: &PersistedConversations) -> String {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let mut output = String::from(CONVERSATIONS_VERSION_V2);
    output.push('\n');
    for conversation in &persisted.conversations {
        output.push_str("[conversation]\n");
        if let Some(agent_uid) = &conversation.agent_uid {
            output.push_str("agent_uid=");
            output.push_str(&quote(agent_uid));
            output.push('\n');
        }
        if let Some(session_name) = &conversation.legacy_session_name {
            output.push_str("runtime_session_name=");
            output.push_str(&quote(session_name));
            output.push('\n');
        }
        for message in &conversation.messages {
            output.push_str("[message]\n");
            output.push_str("author=");
            output.push_str(&quote(author_code(&message.author)));
            output.push('\n');
            output.push_str("delivery=");
            output.push_str(&quote(delivery_code(&message.delivery)));
            output.push('\n');
            if let MessageDeliveryState::Failed(error) = &message.delivery {
                output.push_str("error=");
                output.push_str(&quote(error));
                output.push('\n');
            }
            output.push_str("body=");
            output.push_str(&quote(&message.body));
            output.push('\n');
        }
    }
    output
}

/// Parses persisted conversations.
pub(super) fn parse_persisted_conversations(text: &str) -> PersistedConversations {
    let version = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    match version {
        CONVERSATIONS_VERSION_V1 => parse_persisted_conversations_with(text, true),
        CONVERSATIONS_VERSION_V2 => parse_persisted_conversations_with(text, false),
        _ => PersistedConversations::default(),
    }
}

fn parse_persisted_conversations_with(
    text: &str,
    legacy_session_key: bool,
) -> PersistedConversations {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let mut persisted = PersistedConversations::default();
    let mut current: Option<PersistedConversationRecord> = None;
    let mut pending_author: Option<String> = None;
    let mut pending_delivery: Option<String> = None;
    let mut pending_error: Option<String> = None;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 {
            continue;
        }
        match line {
            "[conversation]" => {
                if let Some(current) = current.take() {
                    persisted.conversations.push(current);
                }
                current = Some(PersistedConversationRecord {
                    agent_uid: None,
                    legacy_session_name: None,
                    messages: Vec::new(),
                });
                pending_author = None;
                pending_delivery = None;
                pending_error = None;
            }
            "[message]" => {
                pending_author = None;
                pending_delivery = None;
                pending_error = None;
            }
            _ => {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key {
                    "agent_uid" => {
                        if let (Some(current), Some(agent_uid)) = (current.as_mut(), unquote(value))
                        {
                            current.agent_uid = Some(agent_uid);
                        }
                    }
                    "runtime_session_name" => {
                        if let (Some(current), Some(session_name)) =
                            (current.as_mut(), unquote(value))
                        {
                            current.legacy_session_name = Some(session_name);
                        }
                    }
                    "session" if legacy_session_key => {
                        if let (Some(current), Some(session_name)) =
                            (current.as_mut(), unquote(value))
                        {
                            current.legacy_session_name = Some(session_name);
                        }
                    }
                    "author" => pending_author = unquote(value),
                    "delivery" => pending_delivery = unquote(value),
                    "error" => pending_error = unquote(value),
                    "body" => {
                        let (Some(current), Some(body), Some(delivery)) = (
                            current.as_mut(),
                            unquote(value),
                            pending_delivery.as_deref(),
                        ) else {
                            continue;
                        };
                        let author = pending_author
                            .take()
                            .as_deref()
                            .and_then(parse_author)
                            .unwrap_or(MessageAuthor::User);
                        let Some(delivery) = parse_delivery(delivery, pending_error.take()) else {
                            continue;
                        };
                        current.messages.push(PersistedConversationMessage {
                            author,
                            body,
                            delivery,
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(current) = current.take() {
        persisted.conversations.push(current);
    }
    persisted.conversations.retain(|conversation| {
        conversation.agent_uid.is_some() || conversation.legacy_session_name.is_some()
    });
    persisted
}

/// Loads persisted conversations from.
fn load_persisted_conversations_from(path: &PathBuf) -> PersistedConversations {
    match fs::read_to_string(path) {
        Ok(text) => parse_persisted_conversations(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            PersistedConversations::default()
        }
        Err(error) => {
            crate::terminals::append_debug_log(format!(
                "conversations load failed {}: {error}",
                path.display()
            ));
            PersistedConversations::default()
        }
    }
}

/// Builds persisted conversations.
pub(super) fn build_persisted_conversations(
    conversations: &ConversationStore,
    agent_catalog: &AgentCatalog,
) -> PersistedConversations {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    PersistedConversations {
        conversations: conversations
            .conversations
            .values()
            .filter_map(|conversation| {
                let agent_uid = agent_catalog.uid(conversation.agent_id)?.to_owned();
                let messages = conversation
                    .message_ids
                    .iter()
                    .filter_map(|message_id| conversations.messages.get(message_id))
                    .map(|message| PersistedConversationMessage {
                        author: message.author.clone(),
                        body: message.body.clone(),
                        delivery: message.delivery.clone(),
                    })
                    .collect::<Vec<_>>();
                Some(PersistedConversationRecord {
                    agent_uid: Some(agent_uid),
                    legacy_session_name: None,
                    messages,
                })
            })
            .collect(),
    }
}

/// Restores persisted conversations.
pub(super) fn restore_persisted_conversations(
    persisted: &PersistedConversations,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    conversations: &mut ConversationStore,
) {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    *conversations = ConversationStore::default();
    for conversation in &persisted.conversations {
        let agent_id = conversation
            .agent_uid
            .as_deref()
            .and_then(|agent_uid| agent_catalog.find_by_uid(agent_uid))
            .or_else(|| {
                conversation
                    .legacy_session_name
                    .as_deref()
                    .and_then(|session_name| runtime_index.agent_for_session(session_name))
            });
        let Some(agent_id) = agent_id else {
            continue;
        };
        let conversation_id = conversations.ensure_conversation(agent_id);
        for message in &conversation.messages {
            let _ = conversations.push_message(
                conversation_id,
                message.author.clone(),
                message.body.clone(),
                message.delivery.clone(),
            );
        }
    }
}

/// Loads persisted conversations from disk and restores them directly into the live store.
///
/// Startup uses this wrapper so the on-disk schema stays local to this module.
pub(crate) fn restore_persisted_conversations_from_path(
    path: &PathBuf,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    conversations: &mut ConversationStore,
) {
    let persisted = load_persisted_conversations_from(path);
    restore_persisted_conversations(&persisted, agent_catalog, runtime_index, conversations);
}

/// Marks conversations dirty.
pub(crate) fn mark_conversations_dirty(
    persistence_state: &mut ConversationPersistenceState,
    time: Option<&Time>,
) {
    if persistence_state.dirty_since_secs.is_none() {
        persistence_state.dirty_since_secs = Some(time.map(Time::elapsed_secs).unwrap_or(0.0));
    }
}

/// Saves conversations if dirty.
pub(crate) fn save_conversations_if_dirty(
    time: Res<Time>,
    conversations: Res<ConversationStore>,
    agent_catalog: Res<AgentCatalog>,
    mut persistence_state: ResMut<ConversationPersistenceState>,
) {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let Some(dirty_since) = persistence_state.dirty_since_secs else {
        return;
    };
    if time.elapsed_secs() - dirty_since < CONVERSATIONS_SAVE_DEBOUNCE_SECS {
        return;
    }
    let Some(path) = persistence_state.path.as_ref() else {
        persistence_state.dirty_since_secs = None;
        return;
    };

    let persisted = build_persisted_conversations(&conversations, &agent_catalog);
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            crate::terminals::append_debug_log(format!(
                "conversations mkdir failed {}: {error}",
                parent.display()
            ));
            persistence_state.dirty_since_secs = None;
            return;
        }
    }

    let serialized = serialize_persisted_conversations(&persisted);
    if let Err(error) = write_file_atomically(path, &serialized) {
        crate::terminals::append_debug_log(format!(
            "conversations save failed {}: {error}",
            path.display()
        ));
    }
    persistence_state.dirty_since_secs = None;
}
