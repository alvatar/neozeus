//! Test submodule: `agent_sync` — extracted from the centralized test bucket.

#![allow(unused_imports)]

use super::super::{
    ensure_shared_app_command_test_resources, fake_runtime_spawner, init_git_repo,
    insert_default_hud_resources, insert_terminal_manager_resources, insert_test_hud_state,
    pressed_text, snapshot_test_hud_state, temp_dir, test_bridge, write_pi_session_file,
    FakeDaemonClient,
};
use crate::agents::{AgentCatalog, AgentRuntimeIndex};
use crate::terminals::{
    kill_active_terminal_session_and_remove as kill_active_terminal, TerminalFontState,
    TerminalGlyphCache, TerminalManager, TerminalNotesState, TerminalPanel, TerminalPanelFrame,
    TerminalPresentationStore, TerminalTextRenderer, TerminalViewState,
};
use crate::{
    app::{
        AgentCommand as AppAgentCommand, AppCommand, AppSessionState, AppStatePersistenceState,
        ComposerCommand as AppComposerCommand, CreateAgentDialogField,
        CreateAgentKind as AppCreateAgentKind, TaskCommand as AppTaskCommand, WidgetCommand,
    },
    app_config::DEFAULT_BG,
    composer::{
        clone_agent_name_field_rect, clone_agent_submit_button_rect, clone_agent_workdir_rect,
        create_agent_name_field_rect, message_box_action_buttons, message_box_rect,
        message_box_shortcut_button_rects, task_dialog_action_buttons,
    },
    hud::{
        handle_hud_module_shortcuts, handle_hud_pointer_input, AgentListDragState,
        AgentListUiState, AgentListView, HudDragState, HudRect, HudState, HudWidgetKey,
        TerminalVisibilityPolicy, TerminalVisibilityState,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    image::Image,
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::MouseWheel,
        ButtonState,
    },
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};


use super::support::*;

#[test]
fn terminal_focus_sync_does_not_rewrite_agent_list_selection() {
    let (bridge_a, _) = test_bridge();
    let (bridge_b, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_a = manager.create_terminal(bridge_a);
    let terminal_b = manager.create_terminal(bridge_b);
    manager.focus_terminal(terminal_b);

    let mut catalog = AgentCatalog::default();
    let agent_a = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_b = catalog.create_agent(
        Some("beta".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_a, terminal_a, "alpha-session".into(), None);
    runtime_index.link_terminal(agent_b, terminal_b, "beta-session".into(), None);

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(fake_runtime_spawner(Arc::new(FakeDaemonClient::default())));
    world.insert_resource(crate::hud::AgentListSelection::OwnedTmux(
        "tmux-session-1".into(),
    ));
    world.insert_resource(manager.clone_focus_state());
    world.insert_resource(manager);

    world
        .run_system_once(crate::app::sync_agents_from_terminals)
        .unwrap();

    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
}


#[test]
fn sync_agents_from_terminals_cleans_tasks_conversations_notes_and_persistence_for_disappeared_agent(
) {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "alpha-session".into(), None);
    let mut focus_state = crate::terminals::TerminalFocusState::default();
    focus_state.focus_terminal(&manager, terminal_id);
    let input_capture = crate::hud::HudInputCaptureState {
        direct_input_terminal: Some(terminal_id),
    };
    let mut view_state = TerminalViewState::default();
    view_state.focus_terminal(Some(terminal_id));
    let visibility_state = TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(terminal_id),
    };
    let mut active_terminal_content = crate::terminals::ActiveTerminalContentState::default();
    active_terminal_content.select_owned_tmux("tmux-session-1".into(), Some(terminal_id));
    let _ = manager.remove_terminal(terminal_id);

    let mut task_store = crate::conversations::AgentTaskStore::default();
    assert!(task_store.set_text(agent_id, "- [ ] task"));
    let mut conversations = crate::conversations::ConversationStore::default();
    let conversation_id = conversations.ensure_conversation(agent_id);
    let _ = conversations.push_message(
        conversation_id,
        crate::conversations::MessageAuthor::User,
        "hello".into(),
        crate::conversations::MessageDeliveryState::Delivered,
    );
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    assert!(notes_state.set_note_text_by_agent_uid(&agent_uid, "- [ ] task"));
    let mut aegis_policy = crate::aegis::AegisPolicyStore::default();
    assert!(aegis_policy.enable(&agent_uid, "continue cleanly".into()));
    let mut aegis_runtime = crate::aegis::AegisRuntimeStore::default();
    assert!(aegis_runtime.set_state(
        agent_id,
        crate::aegis::AegisRuntimeState::PostCheck {
            deadline_secs: 20.0
        }
    ));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    let mut app_session = AppSessionState::default();
    app_session
        .focus_intent
        .focus_agent(agent_id, crate::app::VisibilityMode::FocusedOnly);
    world.insert_resource(app_session);
    world.insert_resource(aegis_policy);
    world.insert_resource(aegis_runtime);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world.insert_resource(focus_state);
    world.insert_resource(input_capture);
    world.insert_resource(visibility_state);
    world.insert_resource(view_state);
    world.insert_resource(active_terminal_content);
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(task_store);
    world.insert_resource(conversations);
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(notes_state);
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(fake_runtime_spawner(Arc::new(FakeDaemonClient::default())));
    world.insert_resource(manager);

    world
        .run_system_once(crate::app::sync_agents_from_terminals)
        .unwrap();

    assert!(world.resource::<AgentCatalog>().order.is_empty());
    assert!(world
        .resource::<AgentRuntimeIndex>()
        .session_to_agent
        .is_empty());
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::None
    );
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .focus_intent
            .selected_agent(),
        None
    );
    assert_eq!(
        world.resource::<AppSessionState>().visibility_mode(),
        crate::app::VisibilityMode::ShowAll
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        None
    );
    assert_eq!(
        world
            .resource::<crate::hud::HudInputCaptureState>()
            .direct_input_terminal,
        None
    );
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::ShowAll
    );
    assert!(world
        .resource::<crate::terminals::ActiveTerminalContentState>()
        .selected_owned_tmux_session_uid()
        .is_none());
    assert_eq!(
        world
            .resource::<crate::conversations::AgentTaskStore>()
            .text(agent_id),
        None
    );
    assert!(world
        .resource::<crate::conversations::ConversationStore>()
        .conversation_for_agent(agent_id)
        .is_none());
    let notes_state = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(notes_state.note_text_by_agent_uid(&agent_uid), None);
    assert_eq!(notes_state.dirty_since_secs, Some(0.0));
    assert!(world
        .resource::<crate::aegis::AegisPolicyStore>()
        .policy(&agent_uid)
        .is_none());
    assert!(world
        .resource::<crate::aegis::AegisRuntimeStore>()
        .state(agent_id)
        .is_none());
    assert_eq!(
        world
            .resource::<crate::conversations::ConversationPersistenceState>()
            .dirty_since_secs,
        Some(0.0)
    );
    assert_eq!(
        world
            .resource::<AppStatePersistenceState>()
            .dirty_since_secs,
        Some(0.0)
    );
}


/// Verifies the enum default for terminal visibility policy is the non-isolating `ShowAll` mode.
#[test]
fn terminal_visibility_policy_defaults_to_show_all() {
    assert_eq!(
        TerminalVisibilityPolicy::default(),
        TerminalVisibilityPolicy::ShowAll
    );
}
