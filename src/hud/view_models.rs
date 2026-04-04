use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex, AgentStatus, AgentStatusStore},
    app::AppSessionState,
    conversations::{AgentTaskStore, ConversationStore, MessageDeliveryState},
    terminals::{
        ActiveTerminalContentState, OwnedTmuxSessionStore, TerminalManager, TerminalSurface,
    },
    usage::{claude_backoff_active, time_left, UsagePersistenceState, UsageSnapshot},
};
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AgentListRowKey {
    Agent(AgentId),
    OwnedTmux(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnedTmuxOwnerBinding {
    Bound(AgentId),
    Orphan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentListRowKind {
    Agent {
        agent_id: AgentId,
        terminal_id: Option<crate::terminals::TerminalId>,
        has_tasks: bool,
        interactive: bool,
        status: AgentStatus,
        context_pct_milli: Option<i32>,
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
    status_store: Res<AgentStatusStore>,
    owned_tmux_sessions: Res<OwnedTmuxSessionStore>,
    active_terminal_content: Res<ActiveTerminalContentState>,
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

    let selected_tmux_session = active_terminal_content.selected_owned_tmux_session_uid();
    let mut rows = Vec::new();
    for (agent_id, label) in agent_catalog.iter() {
        let terminal_id = runtime_index.primary_terminal(agent_id);
        let terminal = terminal_id.and_then(|terminal_id| terminal_manager.get(terminal_id));
        let interactive =
            terminal.is_some_and(|terminal| terminal.snapshot.runtime.is_interactive());
        let context_pct_milli = terminal
            .and_then(|terminal| terminal.snapshot.surface.as_ref())
            .and_then(parse_agent_context_pct_milli);
        rows.push(AgentListRowView {
            key: AgentListRowKey::Agent(agent_id),
            label: label.to_owned(),
            focused: selected_tmux_session.is_none() && app_session.active_agent == Some(agent_id),
            kind: AgentListRowKind::Agent {
                agent_id,
                terminal_id,
                has_tasks: task_store
                    .text(agent_id)
                    .is_some_and(|text| !text.trim().is_empty()),
                interactive,
                status: status_store.status(agent_id),
                context_pct_milli,
            },
        });
        for session in tmux_by_owner.remove(&agent_id).unwrap_or_default() {
            rows.push(AgentListRowView {
                key: AgentListRowKey::OwnedTmux(session.session_uid.clone()),
                label: session.display_name.clone(),
                focused: selected_tmux_session == Some(session.session_uid.as_str()),
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
        rows.push(AgentListRowView {
            key: AgentListRowKey::OwnedTmux(session.session_uid.clone()),
            label: session.display_name,
            focused: selected_tmux_session == Some(session.session_uid.as_str()),
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

    thread_view.header = app_session
        .active_agent
        .and_then(|agent_id| agent_catalog.label(agent_id))
        .map(str::to_owned)
        .unwrap_or_else(|| "No thread selected".to_owned());
    thread_view.empty_message = "No messages yet".to_owned();
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

fn parse_agent_context_pct_milli(surface: &TerminalSurface) -> Option<i32> {
    (0..surface.rows)
        .rev()
        .take(8)
        .find_map(|row| parse_context_pct_milli(&row_text(surface, row)))
}

fn parse_context_pct_milli(line: &str) -> Option<i32> {
    parse_pi_footer_context_pct_milli(line).or_else(|| parse_codex_footer_context_pct_milli(line))
}

fn parse_pi_footer_context_pct_milli(line: &str) -> Option<i32> {
    let ctx_start = line.find("Ctx(")?;
    let tail = &line[ctx_start..];
    let pct_end = tail.find("%)")?;
    let pct_start = tail[..pct_end].rfind('(')? + 1;
    parse_percent_milli(&tail[pct_start..pct_end])
}

fn parse_codex_footer_context_pct_milli(line: &str) -> Option<i32> {
    let pct_end = line.find("% left")?;
    let prefix = line[..pct_end].trim_end();
    let pct_start = prefix
        .rfind(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .map_or(0, |index| index + 1);
    let remaining = parse_percent_milli(prefix[pct_start..].trim())?;
    Some((100_000 - remaining).clamp(0, 100_000))
}

fn parse_percent_milli(raw: &str) -> Option<i32> {
    let pct = raw.trim().parse::<f32>().ok()?;
    ((0.0..=100.0).contains(&pct)).then_some((pct * 1000.0).round() as i32)
}

fn row_text(surface: &TerminalSurface, row: usize) -> String {
    let mut text = String::new();
    for col in 0..surface.cols {
        let cell = surface.cell(col, row);
        if cell.width == 0 {
            continue;
        }
        text.push_str(&cell.content.to_owned_string());
    }
    text.trim_end().to_owned()
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
            pct_milli: (usage_snapshot.claude.session_pct * 1000.0).round() as i32,
            detail_text: if claude_rate_limited {
                "RATE LIMITED".to_owned()
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
        info_bar_view.claude_session = UsageBarView {
            label: "Claude Session:".to_owned(),
            pct_milli: 0,
            detail_text: "unavailable".to_owned(),
            available: false,
        };
        info_bar_view.claude_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: 0,
            detail_text: "unavailable".to_owned(),
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
        info_bar_view.openai_session = UsageBarView {
            label: "OpenAI Session:".to_owned(),
            pct_milli: 0,
            detail_text: "unavailable".to_owned(),
            available: false,
        };
        info_bar_view.openai_week = UsageBarView {
            label: "Week:".to_owned(),
            pct_milli: 0,
            detail_text: "unavailable".to_owned(),
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
