use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex},
    app::AppSessionState,
    conversations::{AgentTaskStore, ConversationStore, MessageDeliveryState},
    terminals::TerminalManager,
    usage::{claude_backoff_active, time_left, UsagePersistenceState, UsageSnapshot},
};
use bevy::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentListRowView {
    pub(crate) agent_id: AgentId,
    pub(crate) terminal_id: Option<crate::terminals::TerminalId>,
    pub(crate) label: String,
    pub(crate) focused: bool,
    pub(crate) has_tasks: bool,
    pub(crate) interactive: bool,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentListView {
    pub(crate) rows: Vec<AgentListRowView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationListRowView {
    pub(crate) agent_id: AgentId,
    pub(crate) terminal_id: Option<crate::terminals::TerminalId>,
    pub(crate) conversation_id: crate::conversations::ConversationId,
    pub(crate) label: String,
    pub(crate) message_count: usize,
    pub(crate) selected: bool,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationListView {
    pub(crate) rows: Vec<ConversationListRowView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ThreadMessageView {
    pub(crate) body: String,
    pub(crate) delivered: bool,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ThreadView {
    pub(crate) agent_id: Option<AgentId>,
    messages: Vec<ThreadMessageView>,
}

impl ThreadView {
    /// Returns whether the current thread has any messages.
    pub(crate) fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Returns cloned `(body, delivered)` pairs in display order.
    pub(crate) fn message_rows(&self) -> Vec<(String, bool)> {
        self.messages
            .iter()
            .map(|message| (message.body.clone(), message.delivered))
            .collect()
    }
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerView {
    pub(crate) title: Option<String>,
    pub(crate) text: String,
    pub(crate) visible: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UsageBarView {
    pub(crate) label: String,
    pub(crate) pct_milli: i32,
    pub(crate) detail_text: String,
    pub(crate) available: bool,
}

impl UsageBarView {
    /// Returns the current usage percentage in logical units.
    pub(crate) fn pct(&self) -> f32 {
        self.pct_milli as f32 / 1000.0
    }
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct InfoBarView {
    pub(crate) claude_session: UsageBarView,
    pub(crate) claude_week: UsageBarView,
    pub(crate) openai_session: UsageBarView,
    pub(crate) openai_week: UsageBarView,
}

#[allow(
    clippy::too_many_arguments,
    reason = "view-model derivation reads the authoritative stores and writes the derived UI projections"
)]
/// Handles sync hud view models.
pub(crate) fn sync_hud_view_models(
    agent_catalog: Res<AgentCatalog>,
    runtime_index: Res<AgentRuntimeIndex>,
    app_session: Res<AppSessionState>,
    terminal_manager: Res<TerminalManager>,
    task_store: Res<AgentTaskStore>,
    conversations: Res<ConversationStore>,
    mut agent_list: ResMut<AgentListView>,
    mut conversation_list: ResMut<ConversationListView>,
    mut thread_view: ResMut<ThreadView>,
    mut composer_view: ResMut<ComposerView>,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    agent_list.rows = agent_catalog
        .iter()
        .map(|(agent_id, label)| {
            let terminal_id = runtime_index.primary_terminal(agent_id);
            let interactive = terminal_id
                .and_then(|terminal_id| terminal_manager.get(terminal_id))
                .is_some_and(|terminal| terminal.snapshot.runtime.is_interactive());
            AgentListRowView {
                agent_id,
                terminal_id,
                label: label.to_owned(),
                focused: app_session.active_agent == Some(agent_id),
                has_tasks: task_store
                    .text(agent_id)
                    .is_some_and(|text| !text.trim().is_empty()),
                interactive,
            }
        })
        .collect();

    conversation_list.rows = agent_catalog
        .iter()
        .filter_map(|(agent_id, label)| {
            let conversation_id = conversations.conversation_for_agent(agent_id)?;
            Some(ConversationListRowView {
                agent_id,
                terminal_id: runtime_index.primary_terminal(agent_id),
                conversation_id,
                label: label.to_owned(),
                message_count: conversations.messages_for(conversation_id).len(),
                selected: app_session.active_agent == Some(agent_id),
            })
        })
        .collect();

    thread_view.agent_id = app_session.active_agent;
    thread_view.messages = app_session
        .active_agent
        .and_then(|agent_id| conversations.conversation_for_agent(agent_id))
        .map(|conversation_id| {
            conversations
                .messages_for(conversation_id)
                .into_iter()
                .map(|(body, delivery)| ThreadMessageView {
                    body,
                    delivered: matches!(delivery, MessageDeliveryState::Delivered),
                })
                .collect()
        })
        .unwrap_or_default();

    composer_view.visible = app_session.composer.session.is_some();
    composer_view.title = app_session
        .composer
        .session
        .as_ref()
        .map(|session| match session.mode {
            crate::composer::ComposerMode::Message { agent_id } => {
                format!(
                    "Message {}",
                    agent_catalog.label(agent_id).unwrap_or("agent")
                )
            }
            crate::composer::ComposerMode::TaskEdit { agent_id } => {
                format!("Tasks {}", agent_catalog.label(agent_id).unwrap_or("agent"))
            }
        });
    composer_view.text = if app_session.composer.message_editor.visible {
        app_session.composer.message_editor.text.clone()
    } else if app_session.composer.task_editor.visible {
        app_session.composer.task_editor.text.clone()
    } else {
        String::new()
    };
}

/// Derives the render-ready info-bar usage rows from the normalized usage snapshot.
pub(crate) fn sync_info_bar_view_model(
    usage_snapshot: Res<UsageSnapshot>,
    persistence_state: Res<UsagePersistenceState>,
    mut info_bar_view: ResMut<InfoBarView>,
) {
    let now_unix_secs = current_unix_secs();
    let claude_rate_limited = claude_backoff_active(
        &persistence_state.claude_backoff_until_path,
        now_unix_secs as u64,
    );

    if usage_snapshot.claude.available {
        info_bar_view.claude_session = UsageBarView {
            label: "Claude Session:".to_owned(),
            pct_milli: milli_percent(usage_snapshot.claude.session_pct),
            detail_text: countdown_or_empty(
                &usage_snapshot.claude.session_resets_at,
                now_unix_secs,
            ),
            available: true,
        };
        info_bar_view.claude_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: milli_percent(usage_snapshot.claude.week_pct),
            detail_text: countdown_or_empty(&usage_snapshot.claude.week_resets_at, now_unix_secs),
            available: true,
        };
    } else {
        info_bar_view.claude_session = UsageBarView {
            label: "Claude Session:".to_owned(),
            pct_milli: 0,
            detail_text: if claude_rate_limited {
                "(rate limited)".to_owned()
            } else {
                "(unavailable)".to_owned()
            },
            available: false,
        };
        info_bar_view.claude_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: 0,
            detail_text: String::new(),
            available: false,
        };
    }

    if usage_snapshot.openai.available {
        info_bar_view.openai_session = UsageBarView {
            label: "OpenAI Session:".to_owned(),
            pct_milli: usage_snapshot.openai.requests_pct_milli.clamp(0, 100_000),
            detail_text: countdown_or_empty(
                &usage_snapshot.openai.requests_resets_at,
                now_unix_secs,
            ),
            available: true,
        };
        info_bar_view.openai_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: usage_snapshot.openai.tokens_pct_milli.clamp(0, 100_000),
            detail_text: countdown_or_empty(&usage_snapshot.openai.tokens_resets_at, now_unix_secs),
            available: true,
        };
    } else {
        info_bar_view.openai_session = UsageBarView {
            label: "OpenAI Session:".to_owned(),
            pct_milli: 0,
            detail_text: "(unavailable)".to_owned(),
            available: false,
        };
        info_bar_view.openai_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: 0,
            detail_text: String::new(),
            available: false,
        };
    }
}

fn milli_percent(value: f32) -> i32 {
    (value.clamp(0.0, 100.0) * 1000.0).round() as i32
}

fn countdown_or_empty(value: &str, now_unix_secs: i64) -> String {
    let countdown = time_left(value, now_unix_secs);
    if countdown.is_empty() {
        String::new()
    } else {
        format!("({countdown})")
    }
}

fn current_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
