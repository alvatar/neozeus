use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex},
    app::AppSessionState,
    conversations::{AgentTaskStore, ConversationStore, MessageDeliveryState},
    terminals::{
        TerminalDisplayMode, TerminalFocusState, TerminalFontState, TerminalManager,
        TerminalPresentationStore, TerminalViewState,
    },
};
use bevy::prelude::*;

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

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugToolbarView {
    pub(crate) terminal_count: usize,
    pub(crate) active_terminal_display: String,
    pub(crate) active_status: String,
    pub(crate) zoom_distance_milli: i32,
    pub(crate) font_summary: String,
    pub(crate) key_events_seen: u64,
    pub(crate) updates_dropped: u64,
    pub(crate) dirty_rows_uploaded: u64,
    pub(crate) pixel_perfect_active: bool,
}

impl DebugToolbarView {
    /// Returns the current zoom distance in logical units.
    pub(crate) fn zoom_distance(&self) -> f32 {
        self.zoom_distance_milli as f32 / 1000.0
    }
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
    focus_state: Option<Res<TerminalFocusState>>,
    presentation_store: Option<Res<TerminalPresentationStore>>,
    view_state: Option<Res<TerminalViewState>>,
    font_state: Option<Res<TerminalFontState>>,
    task_store: Res<AgentTaskStore>,
    conversations: Res<ConversationStore>,
    mut agent_list: ResMut<AgentListView>,
    mut conversation_list: ResMut<ConversationListView>,
    mut thread_view: ResMut<ThreadView>,
    mut composer_view: ResMut<ComposerView>,
    debug_toolbar_view: Option<ResMut<DebugToolbarView>>,
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

    let Some(mut debug_toolbar_view) = debug_toolbar_view else {
        return;
    };
    debug_toolbar_view.terminal_count = terminal_manager.terminal_ids().len();
    debug_toolbar_view.active_terminal_display = focus_state
        .as_deref()
        .and_then(TerminalFocusState::active_id)
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    debug_toolbar_view.active_status = focus_state
        .as_deref()
        .and_then(|focus_state| focus_state.active_snapshot(&terminal_manager))
        .map(|snapshot| snapshot.runtime.status.as_str().to_owned())
        .unwrap_or_else(|| "no active terminal".to_owned());
    debug_toolbar_view.zoom_distance_milli = view_state
        .as_deref()
        .map(|view_state| (view_state.distance * 1000.0).round() as i32)
        .unwrap_or(10_000);
    debug_toolbar_view.font_summary = match font_state
        .as_deref()
        .and_then(|font_state| font_state.report.as_ref())
    {
        Some(Ok(report)) => format!("font {}", report.primary.family),
        Some(Err(error)) => format!("font error {error}"),
        None => "font loading".to_owned(),
    };
    let (key_events_seen, updates_dropped, dirty_rows_uploaded) = focus_state
        .as_deref()
        .map(|focus_state| {
            let stats = focus_state.active_debug_stats(&terminal_manager);
            (
                stats.key_events_seen,
                stats.updates_dropped,
                stats.dirty_rows_uploaded,
            )
        })
        .unwrap_or_default();
    debug_toolbar_view.key_events_seen = key_events_seen;
    debug_toolbar_view.updates_dropped = updates_dropped;
    debug_toolbar_view.dirty_rows_uploaded = dirty_rows_uploaded;
    debug_toolbar_view.pixel_perfect_active = focus_state
        .as_deref()
        .zip(presentation_store.as_deref())
        .is_some_and(|(focus_state, presentation_store)| {
            matches!(
                presentation_store.active_display_mode(focus_state.active_id()),
                Some(TerminalDisplayMode::PixelPerfect)
            )
        });
}

#[cfg(test)]
mod tests;
