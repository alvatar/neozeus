//! Test submodule: `agent_cascade` — extracted from the centralized test bucket.

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

/// Verifies that killing an agent also cascade-kills any owned tmux child sessions.
#[test]
fn killing_agent_cascade_kills_owned_tmux_children() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(terminal_id);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "neozeus-session-a".into(), None);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));

    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: agent_uid.clone(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    assert!(world
        .resource_mut::<crate::aegis::AegisPolicyStore>()
        .enable(&agent_uid, "continue cleanly".into()));
    assert!(world
        .resource_mut::<crate::aegis::AegisRuntimeStore>()
        .set_state(
            agent_id,
            crate::aegis::AegisRuntimeState::PendingDelay { deadline_secs: 6.0 }
        ));
    assert!(world
        .resource_mut::<crate::conversations::AgentTaskStore>()
        .set_text(agent_id, "- [ ] task"));
    let conversation_id = world
        .resource_mut::<crate::conversations::ConversationStore>()
        .ensure_conversation(agent_id);
    let _ = world
        .resource_mut::<crate::conversations::ConversationStore>()
        .push_message(
            conversation_id,
            crate::conversations::MessageAuthor::User,
            "hello".into(),
            crate::conversations::MessageDeliveryState::Delivered,
        );
    assert!(world
        .resource_mut::<crate::terminals::TerminalNotesState>()
        .set_note_text_by_agent_uid(&agent_uid, "- [ ] task"));

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
    run_app_commands(&mut world);

    assert!(world.resource::<AgentCatalog>().order.is_empty());
    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(client.owned_tmux_sessions.lock().unwrap().is_empty());
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
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalNotesState>()
            .note_text_by_agent_uid(&agent_uid),
        None
    );
    assert_eq!(
        world
            .resource::<crate::conversations::ConversationPersistenceState>()
            .dirty_since_secs,
        Some(1.0)
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalNotesState>()
            .dirty_since_secs,
        Some(1.0)
    );
    assert!(world
        .resource::<crate::aegis::AegisPolicyStore>()
        .policy(&agent_uid)
        .is_none());
    assert!(world
        .resource::<crate::aegis::AegisRuntimeStore>()
        .state(agent_id)
        .is_none());
}


/// Verifies that parent deletion cascades only the active agent's owned tmux children and leaves
/// other agents plus orphan rows untouched.
#[test]
fn killing_agent_cascade_kills_only_selected_agent_owned_tmux_children() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .extend(["neozeus-session-a".into(), "neozeus-session-b".into()]);

    let (bridge_a, _) = test_bridge();
    let (bridge_b, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_a = manager.create_terminal_with_session(bridge_a, "neozeus-session-a".into());
    let terminal_b = manager.create_terminal_with_session(bridge_b, "neozeus-session-b".into());
    manager.focus_terminal(terminal_a);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

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
    let agent_a_uid = catalog.uid(agent_a).unwrap().to_owned();
    let agent_b_uid = catalog.uid(agent_b).unwrap().to_owned();
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_a, terminal_a, "neozeus-session-a".into(), None);
    runtime_index.link_terminal(agent_b, terminal_b, "neozeus-session-b".into(), None);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_a));

    client.owned_tmux_sessions.lock().unwrap().extend([
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a1".into(),
            owner_agent_uid: agent_a_uid.clone(),
            tmux_name: "neozeus-tmux-a1".into(),
            display_name: "A-1".into(),
            cwd: "/tmp/a1".into(),
            attached: false,
            created_unix: 1,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a2".into(),
            owner_agent_uid: agent_a_uid,
            tmux_name: "neozeus-tmux-a2".into(),
            display_name: "A-2".into(),
            cwd: "/tmp/a2".into(),
            attached: false,
            created_unix: 2,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-b1".into(),
            owner_agent_uid: agent_b_uid,
            tmux_name: "neozeus-tmux-b1".into(),
            display_name: "B-1".into(),
            cwd: "/tmp/b1".into(),
            attached: false,
            created_unix: 3,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-orphan".into(),
            owner_agent_uid: "missing-agent".into(),
            tmux_name: "neozeus-tmux-orphan".into(),
            display_name: "ORPHAN".into(),
            cwd: "/tmp/orphan".into(),
            attached: false,
            created_unix: 4,
        },
    ]);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
    run_app_commands(&mut world);

    let remaining = client.owned_tmux_sessions.lock().unwrap().clone();
    assert_eq!(remaining.len(), 2);
    assert!(remaining
        .iter()
        .any(|session| session.session_uid == "tmux-b1"));
    assert!(remaining
        .iter()
        .any(|session| session.session_uid == "tmux-orphan"));
    assert!(!remaining
        .iter()
        .any(|session| session.session_uid == "tmux-a1"));
    assert!(!remaining
        .iter()
        .any(|session| session.session_uid == "tmux-a2"));
    assert_eq!(world.resource::<AgentCatalog>().order, vec![agent_b]);
    assert_eq!(
        world.resource::<TerminalManager>().terminal_ids(),
        &[terminal_b]
    );
}


/// Verifies that parent deletion aborts when owned tmux child cleanup fails and the child still exists.
#[test]
fn killing_agent_aborts_when_owned_tmux_child_kill_fails() {
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_owned_tmux_kill.lock().unwrap() = true;
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(terminal_id);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "neozeus-session-a".into(), None);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));

    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: agent_uid,
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
    run_app_commands(&mut world);

    assert!(!world.resource::<AgentCatalog>().order.is_empty());
    assert_eq!(
        world.resource::<TerminalManager>().terminal_ids(),
        &[terminal_id]
    );
    assert_eq!(client.owned_tmux_sessions.lock().unwrap().len(), 1);
}


#[test]
fn killing_selected_agent_targets_selected_agent_even_when_focus_differs() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .extend(["neozeus-session-a".into(), "neozeus-session-b".into()]);

    let (bridge_a, _) = test_bridge();
    let (bridge_b, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_a = manager.create_terminal_with_session(bridge_a, "neozeus-session-a".into());
    let terminal_b = manager.create_terminal_with_session(bridge_b, "neozeus-session-b".into());
    manager.focus_terminal(terminal_a);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

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
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_a, terminal_a, "neozeus-session-a".into(), None);
    runtime_index.link_terminal(agent_b, terminal_b, "neozeus-session-b".into(), None);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_b));

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
    run_app_commands(&mut world);

    assert_eq!(world.resource::<AgentCatalog>().order, vec![agent_a]);
    assert_eq!(
        world.resource::<TerminalManager>().terminal_ids(),
        &[terminal_a]
    );
    assert!(client
        .sessions
        .lock()
        .unwrap()
        .contains("neozeus-session-a"));
    assert!(!client
        .sessions
        .lock()
        .unwrap()
        .contains("neozeus-session-b"));
}

