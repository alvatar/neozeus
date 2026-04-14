use crate::{
    agents::{AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex, AgentStatusStore},
    app::AppSessionState,
    conversations::{AgentTaskStore, ConversationStore, MessageDeliveryState},
    shared::daemon_wire::DaemonSessionMetrics,
    terminals::{LiveSessionMetricsStore, OwnedTmuxSessionStore, TerminalManager},
    usage::{time_left, UsageFreshness, UsageSnapshot},
    visual_contract::{VisualAgentActivity, VisualContractState},
};
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

/// Derived UI row selection projected from [`crate::app::FocusIntentState`].
///
/// This resource exists so HUD systems can render and interact in row-space, but focus authority
/// lives in the session focus intent and is projected here.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum AgentListSelection {
    #[default]
    None,
    Agent(AgentId),
    OwnedTmux(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum AgentListRowKey {
    Agent(AgentId),
    OwnedTmux(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnedTmuxOwnerBinding {
    Bound(AgentId),
    Orphan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentListActivity {
    Idle,
    Working,
}

impl AgentListActivity {
    fn from_visual_activity(activity: VisualAgentActivity) -> Self {
        match activity {
            VisualAgentActivity::Working => AgentListActivity::Working,
            VisualAgentActivity::Idle => AgentListActivity::Idle,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentListRowKind {
    Agent {
        agent_id: AgentId,
        agent_kind: AgentKind,
        terminal_id: Option<crate::terminals::TerminalId>,
        has_tasks: bool,
        interactive: bool,
        activity: AgentListActivity,
        paused: bool,
        aegis_enabled: bool,
        context_pct_milli: Option<i32>,
        session_metrics: DaemonSessionMetrics,
    },
    OwnedTmux {
        session_uid: String,
        owner: OwnedTmuxOwnerBinding,
        tmux_name: String,
        cwd: String,
        attached: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentListRowView {
    pub(crate) key: AgentListRowKey,
    pub(crate) label: String,
    pub(crate) focused: bool,
    pub(crate) kind: AgentListRowKind,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentListView {
    pub(crate) rows: Vec<AgentListRowView>,
}

pub(crate) fn selected_agent_id(selection: &AgentListSelection) -> Option<AgentId> {
    match selection {
        AgentListSelection::Agent(agent_id) => Some(*agent_id),
        AgentListSelection::None | AgentListSelection::OwnedTmux(_) => None,
    }
}

pub(crate) fn selected_agent_list_row_key(
    selection: &AgentListSelection,
) -> Option<AgentListRowKey> {
    match selection {
        AgentListSelection::None => None,
        AgentListSelection::Agent(agent_id) => Some(AgentListRowKey::Agent(*agent_id)),
        AgentListSelection::OwnedTmux(session_uid) => {
            Some(AgentListRowKey::OwnedTmux(session_uid.clone()))
        }
    }
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
    pub(crate) header: String,
    pub(crate) empty_message: String,
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
    visual_contract: Res<VisualContractState>,
    status_store: Res<AgentStatusStore>,
    owned_tmux_sessions: Res<OwnedTmuxSessionStore>,
    live_session_metrics: Res<LiveSessionMetricsStore>,
    aegis_policy: Res<crate::aegis::AegisPolicyStore>,
    selection: Res<AgentListSelection>,
    mut agent_list: ResMut<AgentListView>,
    mut conversation_list: ResMut<ConversationListView>,
    mut thread_view: ResMut<ThreadView>,
    mut composer_view: ResMut<ComposerView>,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let mut tmux_by_owner = BTreeMap::<AgentId, Vec<crate::terminals::OwnedTmuxSessionInfo>>::new();
    let mut orphan_tmux = Vec::new();
    for session in &owned_tmux_sessions.sessions {
        if let Some(agent_id) = agent_catalog.find_by_uid(&session.owner_agent_uid) {
            tmux_by_owner
                .entry(agent_id)
                .or_default()
                .push(session.clone());
        } else {
            orphan_tmux.push(session.clone());
        }
    }
    for sessions in tmux_by_owner.values_mut() {
        sessions.sort_by(|left, right| {
            left.created_unix
                .cmp(&right.created_unix)
                .then_with(|| left.tmux_name.cmp(&right.tmux_name))
        });
    }
    orphan_tmux.sort_by(|left, right| {
        left.created_unix
            .cmp(&right.created_unix)
            .then_with(|| left.tmux_name.cmp(&right.tmux_name))
    });

    let selected_row_key = selected_agent_list_row_key(&selection);
    let mut rows = Vec::new();
    for (agent_id, label) in agent_catalog.iter() {
        let terminal_id = runtime_index.primary_terminal(agent_id);
        let terminal = terminal_id.and_then(|terminal_id| terminal_manager.get(terminal_id));
        let interactive =
            terminal.is_some_and(|terminal| terminal.snapshot.runtime.is_interactive());
        let context_pct_milli = status_store.context_pct_milli(agent_id);
        rows.push(AgentListRowView {
            key: AgentListRowKey::Agent(agent_id),
            label: if agent_catalog.is_workdir(agent_id) {
                format!("⎇ {label}")
            } else {
                label.to_owned()
            },
            focused: selected_row_key == Some(AgentListRowKey::Agent(agent_id)),
            kind: AgentListRowKind::Agent {
                agent_id,
                agent_kind: agent_catalog.kind(agent_id).unwrap_or(AgentKind::Terminal),
                terminal_id,
                has_tasks: task_store
                    .text(agent_id)
                    .is_some_and(|text| !text.trim().is_empty()),
                interactive,
                activity: AgentListActivity::from_visual_activity(
                    visual_contract.activity_for_agent(agent_id),
                ),
                paused: agent_catalog.is_paused(agent_id),
                aegis_enabled: agent_catalog
                    .uid(agent_id)
                    .is_some_and(|uid| aegis_policy.is_enabled(uid)),
                context_pct_milli,
                session_metrics: runtime_index
                    .session_name(agent_id)
                    .and_then(|session_id| live_session_metrics.metrics(session_id))
                    .cloned()
                    .unwrap_or_default(),
            },
        });
        for session in tmux_by_owner.remove(&agent_id).unwrap_or_default() {
            let session_uid = session.session_uid.clone();
            rows.push(AgentListRowView {
                key: AgentListRowKey::OwnedTmux(session_uid.clone()),
                label: session.display_name.clone(),
                focused: selected_row_key == Some(AgentListRowKey::OwnedTmux(session_uid)),
                kind: AgentListRowKind::OwnedTmux {
                    session_uid: session.session_uid,
                    owner: OwnedTmuxOwnerBinding::Bound(agent_id),
                    tmux_name: session.tmux_name,
                    cwd: session.cwd,
                    attached: session.attached,
                },
            });
        }
    }
    for session in orphan_tmux {
        let session_uid = session.session_uid.clone();
        rows.push(AgentListRowView {
            key: AgentListRowKey::OwnedTmux(session_uid.clone()),
            label: session.display_name,
            focused: selected_row_key == Some(AgentListRowKey::OwnedTmux(session_uid)),
            kind: AgentListRowKind::OwnedTmux {
                session_uid: session.session_uid,
                owner: OwnedTmuxOwnerBinding::Orphan,
                tmux_name: session.tmux_name,
                cwd: session.cwd,
                attached: session.attached,
            },
        });
    }
    agent_list.rows = rows;

    let selected_agent = selected_agent_id(&selection);

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
                selected: selected_agent == Some(agent_id),
            })
        })
        .collect();

    thread_view.header = selected_agent
        .and_then(|agent_id| agent_catalog.label(agent_id))
        .map(str::to_owned)
        .unwrap_or_else(|| "No thread selected".to_owned());
    thread_view.empty_message = "No messages yet".to_owned();
    thread_view.messages = selected_agent
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
    mut info_bar_view: ResMut<InfoBarView>,
) {
    let now_unix_secs = current_unix_secs();
    let claude_rate_limited = usage_snapshot.claude_state.rate_limited;

    if usage_snapshot.claude.available {
        info_bar_view.claude_session = UsageBarView {
            label: "Claude Session:".to_owned(),
            pct_milli: (usage_snapshot.claude.session_pct * 1000.0).round() as i32,
            detail_text: if claude_rate_limited {
                "RL".to_owned()
            } else {
                format_usage_reset_detail(time_left(
                    &usage_snapshot.claude.session_resets_at,
                    now_unix_secs,
                ))
            },
            available: true,
        };
        info_bar_view.claude_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: (usage_snapshot.claude.week_pct * 1000.0).round() as i32,
            detail_text: format_usage_reset_detail(time_left(
                &usage_snapshot.claude.week_resets_at,
                now_unix_secs,
            )),
            available: true,
        };
    } else {
        let detail = match usage_snapshot.claude_state.freshness {
            UsageFreshness::Malformed => "malformed".to_owned(),
            UsageFreshness::Missing | UsageFreshness::Parsed => "unavailable".to_owned(),
        };
        info_bar_view.claude_session = UsageBarView {
            label: "Claude Session:".to_owned(),
            pct_milli: 0,
            detail_text: detail.clone(),
            available: false,
        };
        info_bar_view.claude_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: 0,
            detail_text: detail,
            available: false,
        };
    }

    if usage_snapshot.openai.available {
        info_bar_view.openai_session = UsageBarView {
            label: "OpenAI Session:".to_owned(),
            pct_milli: usage_snapshot.openai.requests_pct_milli,
            detail_text: format_usage_reset_detail(time_left(
                &usage_snapshot.openai.requests_resets_at,
                now_unix_secs,
            )),
            available: true,
        };
        info_bar_view.openai_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: usage_snapshot.openai.tokens_pct_milli,
            detail_text: format_usage_reset_detail(time_left(
                &usage_snapshot.openai.tokens_resets_at,
                now_unix_secs,
            )),
            available: true,
        };
    } else {
        let detail = match usage_snapshot.openai_state.freshness {
            UsageFreshness::Malformed => "malformed".to_owned(),
            UsageFreshness::Missing | UsageFreshness::Parsed => "unavailable".to_owned(),
        };
        info_bar_view.openai_session = UsageBarView {
            label: "OpenAI Session:".to_owned(),
            pct_milli: 0,
            detail_text: detail.clone(),
            available: false,
        };
        info_bar_view.openai_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: 0,
            detail_text: detail,
            available: false,
        };
    }
}

fn format_usage_reset_detail(raw: String) -> String {
    let trimmed = raw.trim().trim_matches(|ch| ch == '(' || ch == ')').trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("({trimmed})")
    }
}

fn current_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
