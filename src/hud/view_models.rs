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
    #[allow(dead_code, reason = "retained for existing render/tests while orphan rows are filtered from production view-models")]
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
    selection: Res<AgentListSelection>,
    mut agent_list: ResMut<AgentListView>,
    mut conversation_list: ResMut<ConversationListView>,
    mut thread_view: ResMut<ThreadView>,
    mut composer_view: ResMut<ComposerView>,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let mut tmux_by_owner = BTreeMap::<AgentId, Vec<crate::terminals::OwnedTmuxSessionInfo>>::new();
    for session in &owned_tmux_sessions.sessions {
        if let Some(agent_id) = agent_catalog.find_by_uid(&session.owner_agent_uid) {
            tmux_by_owner
                .entry(agent_id)
                .or_default()
                .push(session.clone());
        }
    }
    for sessions in tmux_by_owner.values_mut() {
        sessions.sort_by(|left, right| {
            left.created_unix
                .cmp(&right.created_unix)
                .then_with(|| left.tmux_name.cmp(&right.tmux_name))
        });
    }
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
mod tests {
    use super::*;
    use super::{
        selected_agent_id, selected_agent_list_row_key, sync_hud_view_models, sync_info_bar_view_model,
        AgentListActivity, AgentListRowKey, AgentListRowKind, AgentListSelection, AgentListView,
        ComposerView, ConversationListView, InfoBarView, ThreadView,
    };
    use crate::{
        agents::{
            parse_agent_context_pct_milli, AgentCatalog, AgentKind, AgentMetadata, AgentRecoverySpec,
            AgentRuntimeIndex, AgentStatusStore,
        },
        app::AppSessionState,
        conversations::{AgentTaskStore, ConversationStore, MessageAuthor, MessageDeliveryState},
        tests::{insert_terminal_manager_resources, surface_with_text, test_bridge},
        usage::{ClaudeUsageData, OpenAiUsageData, UsageSnapshot},
    };
    use bevy::ecs::system::RunSystemOnce;

    fn run_synced_hud_view_models(world: &mut World) {
        if !world.contains_resource::<Time>() {
            world.insert_resource(Time::<()>::default());
        }
        if !world.contains_resource::<crate::visual_contract::VisualContractState>() {
            world.insert_resource(crate::visual_contract::VisualContractState::default());
        }
        if !world.contains_resource::<crate::hud::HudInputCaptureState>() {
            world.insert_resource(crate::hud::HudInputCaptureState::default());
        }
        if !world.contains_resource::<crate::terminals::LiveSessionMetricsStore>() {
            world.insert_resource(crate::terminals::LiveSessionMetricsStore::default());
        }
        if world.contains_resource::<AgentCatalog>()
            && world.contains_resource::<AgentRuntimeIndex>()
            && world.contains_resource::<AgentStatusStore>()
            && world.contains_resource::<crate::terminals::TerminalManager>()
        {
            world
                .run_system_once(crate::agents::sync_agent_status)
                .unwrap();
            world
                .run_system_once(crate::visual_contract::sync_visual_contract_state)
                .unwrap();
        }
        world.run_system_once(sync_hud_view_models).unwrap();
    }

    #[test]
    fn selected_agent_list_row_key_returns_none_for_none_selection() {
        assert_eq!(selected_agent_list_row_key(&AgentListSelection::None), None);
    }

    #[test]
    fn selected_agent_id_returns_agent_only_for_agent_selection() {
        assert_eq!(
            selected_agent_id(&AgentListSelection::Agent(crate::agents::AgentId(7))),
            Some(crate::agents::AgentId(7))
        );
        assert_eq!(selected_agent_id(&AgentListSelection::None), None);
        assert_eq!(
            selected_agent_id(&AgentListSelection::OwnedTmux("tmux-7".into())),
            None
        );
    }

    #[test]
    fn selected_agent_list_row_key_returns_agent_row() {
        assert_eq!(
            selected_agent_list_row_key(&AgentListSelection::Agent(crate::agents::AgentId(7))),
            Some(AgentListRowKey::Agent(crate::agents::AgentId(7)))
        );
    }

    #[test]
    fn selected_agent_list_row_key_returns_owned_tmux_row() {
        assert_eq!(
            selected_agent_list_row_key(&AgentListSelection::OwnedTmux("tmux-7".into())),
            Some(AgentListRowKey::OwnedTmux("tmux-7".into()))
        );
    }

    #[test]
    fn sync_hud_view_models_derives_agent_rows_and_threads() {
        let (bridge, _) = test_bridge();
        let mut manager = crate::terminals::TerminalManager::default();
        let terminal_id = manager.create_terminal(bridge);

        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("alpha".into()),
            crate::agents::AgentKind::Terminal,
            crate::agents::AgentCapabilities::terminal_defaults(),
        );
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

        let mut app_session = AppSessionState::default();
        app_session.composer.session = Some(crate::composer::ComposerSession {
            mode: crate::composer::ComposerMode::Message { agent_id },
        });
        app_session.composer.message_editor.visible = true;
        app_session.composer.message_editor.text = "hello".into();

        let mut tasks = AgentTaskStore::default();
        tasks.set_text(agent_id, "- [ ] follow up");

        let mut conversations = ConversationStore::default();
        let conversation_id = conversations.ensure_conversation(agent_id);
        conversations.push_message(
            conversation_id,
            MessageAuthor::User,
            "hello".into(),
            MessageDeliveryState::Delivered,
        );

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
        world.insert_resource(app_session);
        world.insert_resource(tasks);
        world.insert_resource(conversations);
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::Agent(agent_id));
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
        insert_terminal_manager_resources(&mut world, manager);

        run_synced_hud_view_models(&mut world);

        let agent_list = world.resource::<AgentListView>();
        assert_eq!(agent_list.rows.len(), 1);
        assert_eq!(agent_list.rows[0].label, "ALPHA");
        assert!(agent_list.rows[0].focused);
        match &agent_list.rows[0].kind {
            AgentListRowKind::Agent {
                has_tasks,
                activity,
                context_pct_milli,
                agent_kind,
                session_metrics,
                ..
            } => {
                assert!(*has_tasks);
                assert_eq!(*activity, AgentListActivity::Idle);
                assert_eq!(*context_pct_milli, None);
                assert_eq!(*agent_kind, AgentKind::Terminal);
                assert_eq!(
                    session_metrics,
                    &crate::shared::daemon_wire::DaemonSessionMetrics::default()
                );
            }
            other => panic!("expected agent row, got {other:?}"),
        }

        let thread = world.resource::<ThreadView>();
        assert_eq!(thread.header, "ALPHA");
        let rows = thread.message_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "hello");

        let composer = world.resource::<ComposerView>();
        assert!(composer.visible);
        assert_eq!(composer.title.as_deref(), Some("Message ALPHA"));
        assert_eq!(composer.text, "hello");
    }

    #[test]
    fn sync_hud_view_models_projects_session_metrics_into_agent_rows() {
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Pi,
            AgentKind::Pi.capabilities(),
        );
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(
            agent_id,
            crate::terminals::TerminalId(1),
            "neozeus-session-1".into(),
            None,
        );

        let mut live_session_metrics = crate::terminals::LiveSessionMetricsStore::default();
        live_session_metrics.set_metrics_for_tests(
            "neozeus-session-1",
            crate::shared::daemon_wire::DaemonSessionMetrics {
                cpu_pct_milli: Some(42_500),
                ram_bytes: Some(128 * 1024 * 1024),
                net_rx_bytes_per_sec: Some(4096),
                net_tx_bytes_per_sec: Some(2048),
            },
        );

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::Agent(agent_id));
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
        world.insert_resource(live_session_metrics);
        insert_terminal_manager_resources(&mut world, crate::terminals::TerminalManager::default());

        run_synced_hud_view_models(&mut world);

        match &world.resource::<AgentListView>().rows[0].kind {
            AgentListRowKind::Agent {
                agent_kind,
                session_metrics,
                ..
            } => {
                assert_eq!(*agent_kind, AgentKind::Pi);
                assert_eq!(session_metrics.cpu_pct_milli, Some(42_500));
                assert_eq!(session_metrics.ram_bytes, Some(128 * 1024 * 1024));
                assert_eq!(session_metrics.net_rx_bytes_per_sec, Some(4096));
                assert_eq!(session_metrics.net_tx_bytes_per_sec, Some(2048));
            }
            other => panic!("expected agent row, got {other:?}"),
        }
    }

    #[test]
    fn sync_hud_view_models_projects_paused_agents_after_active_rows() {
        let mut catalog = AgentCatalog::default();
        let alpha = catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let beta = catalog.create_agent(
            Some("beta".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let _ = catalog.set_paused(alpha, true);

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::Agent(alpha));
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
        insert_terminal_manager_resources(&mut world, crate::terminals::TerminalManager::default());

        run_synced_hud_view_models(&mut world);

        let rows = &world.resource::<AgentListView>().rows;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "BETA");
        assert_eq!(rows[1].label, "ALPHA");
        match &rows[1].kind {
            AgentListRowKind::Agent {
                paused, agent_id, ..
            } => {
                assert!(*paused);
                assert_eq!(*agent_id, alpha);
            }
            other => panic!("expected paused agent row, got {other:?}"),
        }
        match &rows[0].kind {
            AgentListRowKind::Agent {
                paused, agent_id, ..
            } => {
                assert!(!paused);
                assert_eq!(*agent_id, beta);
            }
            other => panic!("expected active agent row, got {other:?}"),
        }
    }

    #[test]
    fn sync_hud_view_models_places_owned_tmux_rows_under_matching_agent() {
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let agent_uid = catalog.uid(agent_id).unwrap().to_owned();

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::None);
        let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
        owned_tmux
            .sessions
            .push(crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-1".into(),
                owner_agent_uid: agent_uid,
                tmux_name: "neozeus-tmux-1".into(),
                display_name: "BUILD".into(),
                cwd: "/tmp/work".into(),
                attached: false,
                created_unix: 1,
            });
        world.insert_resource(owned_tmux);
        world.insert_resource(crate::terminals::TerminalManager::default());

        run_synced_hud_view_models(&mut world);
        let rows = &world.resource::<AgentListView>().rows;
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0].key, super::AgentListRowKey::Agent(_)));
        assert!(matches!(rows[1].key, super::AgentListRowKey::OwnedTmux(_)));
    }

    #[test]
    fn sync_hud_view_models_prefixes_workdir_agents_with_marker() {
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent_with_metadata(
            Some("alpha".into()),
            AgentKind::Pi,
            AgentKind::Pi.capabilities(),
            AgentMetadata {
                clone_source_session_path: Some("/tmp/pi-alpha.jsonl".into()),
                recovery: Some(AgentRecoverySpec::Pi {
                    session_path: "/tmp/pi-alpha.jsonl".into(),
                    cwd: "/tmp/demo".into(),
                    is_workdir: true,
                    workdir_slug: None,
                }),
            },
        );

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::Agent(agent_id));
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
        world.insert_resource(crate::terminals::TerminalManager::default());

        run_synced_hud_view_models(&mut world);

        let rows = &world.resource::<AgentListView>().rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "⎇ ALPHA");
    }

    #[test]
    fn sync_hud_view_models_orders_multiple_owned_tmux_rows_and_marks_selected_child() {
        let mut catalog = AgentCatalog::default();
        let alpha = catalog.create_agent(
            Some("ALPHA".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let beta = catalog.create_agent(
            Some("BETA".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let alpha_uid = catalog.uid(alpha).unwrap().to_owned();
        let beta_uid = catalog.uid(beta).unwrap().to_owned();

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::OwnedTmux("tmux-a2".into()));
        world.insert_resource(crate::terminals::TerminalManager::default());
        let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
        owned_tmux.sessions = vec![
            crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-b1".into(),
                owner_agent_uid: beta_uid,
                tmux_name: "neozeus-tmux-b1".into(),
                display_name: "BETA BUILD".into(),
                cwd: "/tmp/beta".into(),
                attached: false,
                created_unix: 3,
            },
            crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-a2".into(),
                owner_agent_uid: alpha_uid.clone(),
                tmux_name: "neozeus-tmux-a2".into(),
                display_name: "ALPHA TEST".into(),
                cwd: "/tmp/alpha-2".into(),
                attached: true,
                created_unix: 2,
            },
            crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-orphan".into(),
                owner_agent_uid: "missing-agent".into(),
                tmux_name: "neozeus-tmux-orphan".into(),
                display_name: "BUILD".into(),
                cwd: "/tmp/orphan".into(),
                attached: false,
                created_unix: 4,
            },
            crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-a1".into(),
                owner_agent_uid: alpha_uid,
                tmux_name: "neozeus-tmux-a1".into(),
                display_name: "ALPHA BUILD".into(),
                cwd: "/tmp/alpha-1".into(),
                attached: false,
                created_unix: 1,
            },
        ];
        world.insert_resource(owned_tmux);

        run_synced_hud_view_models(&mut world);
        let rows = &world.resource::<AgentListView>().rows;
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].label, "ALPHA");
        assert_eq!(rows[1].label, "ALPHA BUILD");
        assert_eq!(rows[2].label, "ALPHA TEST");
        assert_eq!(rows[3].label, "BETA");
        assert_eq!(rows[4].label, "BETA BUILD");
        assert!(!rows[0].focused);
        assert!(!rows[1].focused);
        assert!(rows[2].focused);
        assert_eq!(rows.iter().filter(|row| row.focused).count(), 1);
    }

    #[test]
    fn sync_hud_view_models_clears_thread_and_conversation_selection_for_tmux_rows() {
        let mut catalog = AgentCatalog::default();
        let alpha = catalog.create_agent(
            Some("ALPHA".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let alpha_uid = catalog.uid(alpha).unwrap().to_owned();
        let mut conversations = ConversationStore::default();
        let conversation_id = conversations.ensure_conversation(alpha);
        conversations.push_message(
            conversation_id,
            MessageAuthor::User,
            "hello".into(),
            MessageDeliveryState::Delivered,
        );

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(conversations);
        world.insert_resource(AgentListSelection::OwnedTmux("tmux-a1".into()));
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
        owned_tmux
            .sessions
            .push(crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-a1".into(),
                owner_agent_uid: alpha_uid,
                tmux_name: "neozeus-tmux-a1".into(),
                display_name: "BUILD".into(),
                cwd: "/tmp/alpha-1".into(),
                attached: false,
                created_unix: 1,
            });
        world.insert_resource(owned_tmux);

        run_synced_hud_view_models(&mut world);

        let conversations = &world.resource::<ConversationListView>().rows;
        assert_eq!(conversations.len(), 1);
        assert!(!conversations[0].selected);
        let thread = world.resource::<ThreadView>();
        assert_eq!(thread.header, "No thread selected");
        assert!(thread.message_rows().is_empty());
    }

    #[test]
    fn sync_hud_view_models_projects_selected_tmux_row_only() {
        let mut catalog = AgentCatalog::default();
        let alpha = catalog.create_agent(
            Some("ALPHA".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let alpha_uid = catalog.uid(alpha).unwrap().to_owned();

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListSelection::OwnedTmux("tmux-a1".into()));
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
        owned_tmux
            .sessions
            .push(crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-a1".into(),
                owner_agent_uid: alpha_uid,
                tmux_name: "neozeus-tmux-a1".into(),
                display_name: "BUILD".into(),
                cwd: "/tmp/alpha-1".into(),
                attached: false,
                created_unix: 1,
            });
        world.insert_resource(owned_tmux);

        run_synced_hud_view_models(&mut world);
        let rows = &world.resource::<AgentListView>().rows;
        assert_eq!(rows.iter().filter(|row| row.focused).count(), 1);
        assert!(matches!(rows[0].key, AgentListRowKey::Agent(_)));
        assert!(!rows[0].focused);
        assert!(matches!(rows[1].key, AgentListRowKey::OwnedTmux(_)));
        assert!(rows[1].focused);
    }

    #[test]
    fn sync_hud_view_models_tmux_rows_have_no_activity_state() {
        let mut catalog = AgentCatalog::default();
        let alpha = catalog.create_agent(
            Some("ALPHA".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let alpha_uid = catalog.uid(alpha).unwrap().to_owned();

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListSelection::None);
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(crate::terminals::TerminalManager::default());
        let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
        owned_tmux
            .sessions
            .push(crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-a1".into(),
                owner_agent_uid: alpha_uid,
                tmux_name: "neozeus-tmux-a1".into(),
                display_name: "BUILD".into(),
                cwd: "/tmp/alpha-1".into(),
                attached: false,
                created_unix: 1,
            });
        world.insert_resource(owned_tmux);

        run_synced_hud_view_models(&mut world);
        let rows = &world.resource::<AgentListView>().rows;
        assert!(matches!(rows[1].kind, AgentListRowKind::OwnedTmux { .. }));
    }

    #[test]
    fn sync_hud_view_models_filters_unknown_owned_tmux_rows() {
        let mut world = World::default();
        world.insert_resource(AgentCatalog::default());
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::None);
        let mut owned_tmux = crate::terminals::OwnedTmuxSessionStore::default();
        owned_tmux
            .sessions
            .push(crate::terminals::OwnedTmuxSessionInfo {
                session_uid: "tmux-orphan".into(),
                owner_agent_uid: "missing-agent".into(),
                tmux_name: "neozeus-tmux-orphan".into(),
                display_name: "BUILD".into(),
                cwd: "/tmp/work".into(),
                attached: false,
                created_unix: 1,
            });
        world.insert_resource(owned_tmux);
        world.insert_resource(crate::terminals::TerminalManager::default());

        run_synced_hud_view_models(&mut world);
        let rows = &world.resource::<AgentListView>().rows;
        assert!(rows.is_empty(), "unknown/orphan tmux rows must not be rendered at all");
    }

    #[test]
    fn sync_hud_view_models_carries_agent_working_status_into_rows() {
        let (bridge, _) = test_bridge();
        let mut manager = crate::terminals::TerminalManager::default();
        let terminal_id = manager.create_terminal(bridge);
        manager
            .get_mut(terminal_id)
            .expect("terminal should exist")
            .snapshot
            .surface = Some({
            let mut surface = crate::tests::surface_with_text(8, 120, 0, "header");
            surface.set_text_cell(1, 3, "⠋ Working...");
            surface
        });

        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("alpha".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
        );
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

        let mut world = World::default();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs(1));
        world.insert_resource(time);
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::None);
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
        insert_terminal_manager_resources(&mut world, manager);

        world
            .run_system_once(crate::agents::sync_agent_status)
            .unwrap();
        run_synced_hud_view_models(&mut world);

        let agent_list = world.resource::<AgentListView>();
        match &agent_list.rows[0].kind {
            AgentListRowKind::Agent { activity, .. } => {
                assert_eq!(*activity, AgentListActivity::Working)
            }
            other => panic!("expected agent row, got {other:?}"),
        }
    }

    #[test]
    fn sync_hud_view_models_leaves_missing_context_empty() {
        assert_eq!(
            synced_context_pct(AgentKind::Terminal, surface_with_text(8, 120, 0, "header")),
            None
        );
    }

    #[test]
    fn parse_agent_context_pct_milli_parses_pi_footer_context_percentage() {
        let mut surface = surface_with_text(8, 120, 0, "header");
        surface.set_text_cell(
            0,
            7,
            "claude-opus-4-6 (high) Ctx(auto):░░░░░░░░░░(42.5%) Session:██████░░░░(59.0%) Week:█░░░░░░░░░(14.0%) ↑0 ↓0",
        );

        assert_eq!(parse_agent_context_pct_milli(&surface), Some(42_500));
    }

    #[test]
    fn parse_agent_context_pct_milli_parses_codex_footer_remaining_context() {
        let mut surface = surface_with_text(8, 120, 0, "header");
        surface.set_text_cell(0, 7, "  gpt-5.4 default · 83% left · ~/code");

        assert_eq!(parse_agent_context_pct_milli(&surface), Some(17_000));
    }

    fn synced_context_pct(kind: AgentKind, surface: crate::terminals::TerminalSurface) -> Option<i32> {
        let (bridge, _) = test_bridge();
        let mut manager = crate::terminals::TerminalManager::default();
        let terminal_id = manager.create_terminal(bridge);
        manager
            .get_mut(terminal_id)
            .expect("terminal should exist")
            .snapshot
            .surface = Some(surface);

        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(Some("alpha".into()), kind, kind.capabilities());
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
        world.insert_resource(Time::<()>::default());
        world.insert_resource(AppSessionState::default());
        world.insert_resource(AgentTaskStore::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(AgentListView::default());
        world.insert_resource(ConversationListView::default());
        world.insert_resource(ThreadView::default());
        world.insert_resource(ComposerView::default());
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(AgentListSelection::None);
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
        insert_terminal_manager_resources(&mut world, manager);

        run_synced_hud_view_models(&mut world);
        match &world.resource::<AgentListView>().rows[0].kind {
            AgentListRowKind::Agent {
                context_pct_milli, ..
            } => *context_pct_milli,
            other => panic!("expected agent row, got {other:?}"),
        }
    }

    #[test]
    fn sync_info_bar_view_model_derives_usage_rows() {
        let mut world = World::default();
        world.insert_resource(UsageSnapshot {
            claude: ClaudeUsageData {
                session_pct: 42.0,
                week_pct: 10.0,
                session_resets_at: "5m".into(),
                week_resets_at: "2h".into(),
                available: true,
                ..Default::default()
            },
            openai: OpenAiUsageData {
                requests_pct_milli: 40_000,
                tokens_pct_milli: 75_000,
                requests_limit: 100,
                requests_remaining: 60,
                tokens_limit: 1_000,
                tokens_remaining: 250,
                requests_resets_at: "4h55m".into(),
                tokens_resets_at: "4d00h".into(),
                available: true,
            },
            ..Default::default()
        });
        world.insert_resource(InfoBarView::default());

        world.run_system_once(sync_info_bar_view_model).unwrap();

        let info = world.resource::<InfoBarView>();
        assert_eq!(info.claude_session.label, "Claude Session:");
        assert_eq!(info.claude_session.pct_milli, 42_000);
        assert_eq!(info.claude_session.detail_text, "(5m)");
        assert_eq!(info.claude_week.label, "Week:");
        assert_eq!(info.claude_week.pct_milli, 10_000);
        assert_eq!(info.claude_week.detail_text, "(2h00m)");
        assert_eq!(info.openai_session.label, "OpenAI Session:");
        assert_eq!(info.openai_session.pct_milli, 40_000);
        assert_eq!(info.openai_session.detail_text, "(4h55m)");
        assert_eq!(info.openai_week.label, "Week:");
        assert_eq!(info.openai_week.pct_milli, 75_000);
        assert_eq!(info.openai_week.detail_text, "(4d00h)");
    }

    #[test]
    fn sync_info_bar_view_model_handles_unavailable_sources() {
        let mut world = World::default();
        world.insert_resource(UsageSnapshot::default());
        world.insert_resource(InfoBarView::default());

        world.run_system_once(sync_info_bar_view_model).unwrap();

        let info = world.resource::<InfoBarView>();
        assert!(!info.claude_session.available);
        assert!(!info.openai_session.available);
    }

    #[test]
    fn sync_info_bar_view_model_reports_claude_backoff() {
        let mut world = World::default();
        world.insert_resource(UsageSnapshot {
            claude: ClaudeUsageData {
                session_pct: 12.0,
                available: true,
                ..Default::default()
            },
            claude_state: crate::usage::UsageProviderState {
                freshness: crate::usage::UsageFreshness::Parsed,
                rate_limited: true,
                detail: None,
            },
            ..Default::default()
        });
        world.insert_resource(InfoBarView::default());

        world.run_system_once(sync_info_bar_view_model).unwrap();
        assert_eq!(
            world.resource::<InfoBarView>().claude_session.detail_text,
            "RL"
        );
    }
}
