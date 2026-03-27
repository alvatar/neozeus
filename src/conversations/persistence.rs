use crate::{
    agents::AgentRuntimeIndex,
    conversations::{ConversationStore, MessageAuthor, MessageDeliveryState},
};
use bevy::prelude::*;
use std::{env, fs, path::PathBuf};

const CONVERSATIONS_FILENAME: &str = "conversations.v1";
const CONVERSATIONS_VERSION: &str = "version 1";
const CONVERSATIONS_SAVE_DEBOUNCE_SECS: f32 = 0.3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PersistedConversationMessage {
    pub(crate) body: String,
    pub(crate) delivery: MessageDeliveryState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PersistedConversationRecord {
    pub(crate) session_name: String,
    pub(crate) messages: Vec<PersistedConversationMessage>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PersistedConversations {
    pub(crate) conversations: Vec<PersistedConversationRecord>,
}

#[derive(Resource, Default)]
pub(crate) struct ConversationPersistenceState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
}

pub(crate) fn resolve_conversations_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(xdg) = xdg_state_home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg)
                .join("neozeus")
                .join(CONVERSATIONS_FILENAME),
        );
    }
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(home)
                .join(".local/state/neozeus")
                .join(CONVERSATIONS_FILENAME),
        );
    }
    xdg_config_home
        .filter(|value| !value.is_empty())
        .map(|config| {
            PathBuf::from(config)
                .join("neozeus")
                .join(CONVERSATIONS_FILENAME)
        })
}

pub(crate) fn resolve_conversations_path() -> Option<PathBuf> {
    resolve_conversations_path_with(
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
}

fn quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            _ => output.push(ch),
        }
    }
    output.push('"');
    output
}

fn unquote(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut output = String::with_capacity(inner.len());
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            match ch {
                '\\' => output.push('\\'),
                '"' => output.push('"'),
                'n' => output.push('\n'),
                _ => return None,
            }
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else {
            output.push(ch);
        }
    }
    (!escaped).then_some(output)
}

fn delivery_code(delivery: &MessageDeliveryState) -> &'static str {
    match delivery {
        MessageDeliveryState::Pending => "pending",
        MessageDeliveryState::Delivered => "delivered",
        MessageDeliveryState::Failed(_) => "failed",
    }
}

fn parse_delivery(code: &str, error: Option<String>) -> Option<MessageDeliveryState> {
    match code {
        "pending" => Some(MessageDeliveryState::Pending),
        "delivered" => Some(MessageDeliveryState::Delivered),
        "failed" => Some(MessageDeliveryState::Failed(error.unwrap_or_default())),
        _ => None,
    }
}

pub(crate) fn serialize_persisted_conversations(persisted: &PersistedConversations) -> String {
    let mut output = String::from(CONVERSATIONS_VERSION);
    output.push('\n');
    for conversation in &persisted.conversations {
        output.push_str("[conversation]\n");
        output.push_str("session=");
        output.push_str(&quote(&conversation.session_name));
        output.push('\n');
        for message in &conversation.messages {
            output.push_str("[message]\n");
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

pub(crate) fn parse_persisted_conversations(text: &str) -> PersistedConversations {
    let mut persisted = PersistedConversations::default();
    let mut current: Option<PersistedConversationRecord> = None;
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
                    session_name: String::new(),
                    messages: Vec::new(),
                });
                pending_delivery = None;
                pending_error = None;
            }
            "[message]" => {
                pending_delivery = None;
                pending_error = None;
            }
            _ => {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key {
                    "session" => {
                        if let (Some(current), Some(session_name)) =
                            (current.as_mut(), unquote(value))
                        {
                            current.session_name = session_name;
                        }
                    }
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
                        let Some(delivery) = parse_delivery(delivery, pending_error.take()) else {
                            continue;
                        };
                        current
                            .messages
                            .push(PersistedConversationMessage { body, delivery });
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(current) = current.take() {
        persisted.conversations.push(current);
    }
    persisted
        .conversations
        .retain(|conversation| !conversation.session_name.is_empty());
    persisted
}

pub(crate) fn load_persisted_conversations_from(path: &PathBuf) -> PersistedConversations {
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

pub(crate) fn build_persisted_conversations(
    conversations: &ConversationStore,
    runtime_index: &AgentRuntimeIndex,
) -> PersistedConversations {
    PersistedConversations {
        conversations: conversations
            .conversations
            .values()
            .filter_map(|conversation| {
                let session_name = runtime_index
                    .session_name(conversation.agent_id)?
                    .to_owned();
                let messages = conversation
                    .message_ids
                    .iter()
                    .filter_map(|message_id| conversations.messages.get(message_id))
                    .map(|message| PersistedConversationMessage {
                        body: message.body.clone(),
                        delivery: message.delivery.clone(),
                    })
                    .collect::<Vec<_>>();
                Some(PersistedConversationRecord {
                    session_name,
                    messages,
                })
            })
            .collect(),
    }
}

pub(crate) fn restore_persisted_conversations(
    persisted: &PersistedConversations,
    runtime_index: &AgentRuntimeIndex,
    conversations: &mut ConversationStore,
) {
    *conversations = ConversationStore::default();
    for conversation in &persisted.conversations {
        let Some(agent_id) = runtime_index.agent_for_session(&conversation.session_name) else {
            continue;
        };
        let conversation_id = conversations.ensure_conversation(agent_id);
        for message in &conversation.messages {
            let _ = conversations.push_message(
                conversation_id,
                MessageAuthor::User,
                message.body.clone(),
                message.delivery.clone(),
            );
        }
    }
}

pub(crate) fn mark_conversations_dirty(
    persistence_state: &mut ConversationPersistenceState,
    time: Option<&Time>,
) {
    if persistence_state.dirty_since_secs.is_none() {
        persistence_state.dirty_since_secs = Some(time.map(Time::elapsed_secs).unwrap_or(0.0));
    }
}

pub(crate) fn save_conversations_if_dirty(
    time: Res<Time>,
    conversations: Res<ConversationStore>,
    runtime_index: Res<AgentRuntimeIndex>,
    mut persistence_state: ResMut<ConversationPersistenceState>,
) {
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

    let persisted = build_persisted_conversations(&conversations, &runtime_index);
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
    if let Err(error) = fs::write(path, serialized) {
        crate::terminals::append_debug_log(format!(
            "conversations save failed {}: {error}",
            path.display()
        ));
    }
    persistence_state.dirty_since_secs = None;
}
