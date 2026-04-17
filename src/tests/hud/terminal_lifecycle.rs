//! Test submodule: `terminal_lifecycle` — extracted from the centralized test bucket.

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

/// Verifies that removing a middle active terminal promotes the previous surviving terminal in
/// creation order to active/isolate state.
#[test]
fn killing_active_terminal_selects_previous_terminal_in_creation_order() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client.sessions.lock().unwrap().extend([
        "neozeus-session-a".to_owned(),
        "neozeus-session-b".to_owned(),
        "neozeus-session-c".to_owned(),
    ]);

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let (bridge_three, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    let id_three = manager.create_terminal_with_session(bridge_three, "neozeus-session-c".into());
    manager.focus_terminal(id_two);

    let mut store = TerminalPresentationStore::default();
    for id in [id_one, id_two, id_three] {
        store.register(
            id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_two),
    });
    world.insert_resource(TerminalViewState::default());
    for id in [id_one, id_two, id_three] {
        let panel_entity = world.spawn((TerminalPanel { id },)).id();
        let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    let focus = manager.clone_focus_state();
    assert_eq!(manager.terminal_ids(), &[id_one, id_three]);
    assert_eq!(focus.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_two)
    );
}


/// Verifies that removing the first active terminal promotes the next surviving terminal to
/// active/isolate state.
#[test]
fn killing_first_active_terminal_selects_next_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client.sessions.lock().unwrap().extend([
        "neozeus-session-a".to_owned(),
        "neozeus-session-b".to_owned(),
    ]);

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_one);

    let mut store = TerminalPresentationStore::default();
    for id in [id_one, id_two] {
        store.register(
            id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_one),
    });
    world.insert_resource(TerminalViewState::default());
    for id in [id_one, id_two] {
        let panel_entity = world.spawn((TerminalPanel { id },)).id();
        let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    let focus = manager.clone_focus_state();
    assert_eq!(manager.terminal_ids(), &[id_two]);
    assert_eq!(focus.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_one)
    );
}


/// Verifies that a successful active-terminal kill removes terminal-manager state, presentation
/// state, labels, spawned panel entities, and resets visibility/persistence bookkeeping.
#[test]
fn killing_active_terminal_removes_runtime_presentation_and_labels() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_some());
    assert!(client.sessions.lock().unwrap().is_empty());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}


/// Verifies that duplicate agent names are rejected before any daemon session is created.
#[test]
fn create_agent_rejects_duplicate_name_without_creating_session() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.init_resource::<Messages<AppCommand>>();
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    let mut catalog = crate::agents::AgentCatalog::default();
    catalog.create_agent(
        Some("oracle".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentCapabilities::terminal_defaults(),
    );
    world.insert_resource(catalog);
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(crate::app::CreateAgentKind::Pi);
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Create {
            label: Some("oracle".into()),
            kind: crate::agents::AgentKind::Pi,
            working_directory: "~/code".into(),
        }));

    run_app_commands(&mut world);

    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 0);
    assert!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .visible
    );
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .error
            .as_deref(),
        Some("agent `ORACLE` already exists")
    );
    assert!(client.created_sessions.lock().unwrap().is_empty());
}


/// Verifies that creating agent sessions bootstraps the selected CLI command.
#[test]
fn create_agent_request_bootstraps_selected_cli_command() {
    if !sqlite3_available() {
        return;
    }
    let _home_lock = home_env_test_lock().lock().unwrap();
    let previous_home = std::env::var_os("HOME");
    let codex_home = temp_dir("neozeus-codex-create-test-home");
    std::env::set_var("HOME", &codex_home);
    write_codex_state_db(
        &codex_home.join(".codex").join("state_5.sqlite"),
        &[("thread-old", "/tmp/other", 10, "old")],
    );
    std::thread::spawn({
        let codex_home = codex_home.clone();
        move || {
            std::thread::sleep(Duration::from_millis(150));
            write_codex_state_db(
                &codex_home.join(".codex").join("state_6.sqlite"),
                &[
                    ("thread-old", "/tmp/other", 10, "old"),
                    (
                        "thread-new",
                        codex_home.join("code").to_string_lossy().as_ref(),
                        20,
                        "new",
                    ),
                ],
            );
        }
    });

    let client = Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    for (label, kind) in [
        ("pi-agent", crate::agents::AgentKind::Pi),
        ("claude-agent", crate::agents::AgentKind::Claude),
        ("codex-agent", crate::agents::AgentKind::Codex),
    ] {
        world
            .resource_mut::<Messages<AppCommand>>()
            .write(AppCommand::Agent(AppAgentCommand::Create {
                label: Some(label.into()),
                kind,
                working_directory: "~/code".into(),
            }));
    }

    run_app_commands(&mut world);

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 3);
    for (_, _, env_overrides) in &created_sessions {
        assert!(env_overrides
            .iter()
            .any(|(key, _)| key == "NEOZEUS_AGENT_UID"));
        assert!(env_overrides
            .iter()
            .any(|(key, _)| key == "NEOZEUS_AGENT_LABEL"));
        assert!(env_overrides
            .iter()
            .any(|(key, _)| key == "NEOZEUS_AGENT_KIND"));
    }

    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 3);
    assert!(commands.iter().any(|(_, command)| {
        matches!(command, crate::terminals::TerminalCommand::SendCommand(value) if value.starts_with("pi --session "))
    }));
    assert!(commands.iter().any(|(_, command)| {
        matches!(command, crate::terminals::TerminalCommand::SendCommand(value) if value.starts_with("claude --session-id "))
    }));
    assert!(commands.iter().any(|(_, command)| {
        matches!(command, crate::terminals::TerminalCommand::SendCommand(value) if value == "codex")
    }));

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let pi_agent = catalog
        .iter()
        .find_map(|(agent_id, label)| (label == "PI-AGENT").then_some(agent_id))
        .expect("Pi agent should exist");
    let session_path = catalog
        .clone_source_session_path(pi_agent)
        .expect("Pi agent should persist clone provenance");
    assert!(session_path.ends_with(".jsonl"));
    assert!(!catalog.is_workdir(pi_agent));
    let claude_agent = catalog
        .iter()
        .find_map(|(agent_id, label)| (label == "CLAUDE-AGENT").then_some(agent_id))
        .expect("Claude agent should exist");
    assert!(matches!(
        catalog.recovery_spec(claude_agent),
        Some(crate::agents::AgentRecoverySpec::Claude { session_id, cwd, .. })
            if !session_id.trim().is_empty() && cwd.ends_with("/code")
    ));
    let codex_agent = catalog
        .iter()
        .find_map(|(agent_id, label)| (label == "CODEX-AGENT").then_some(agent_id))
        .expect("Codex agent should exist");
    assert!(matches!(
        catalog.recovery_spec(codex_agent),
        Some(crate::agents::AgentRecoverySpec::Codex { session_id, cwd, .. })
            if session_id == "thread-new"
                && cwd == codex_home.join("code").to_string_lossy().as_ref()
    ));
    if let Some(previous_home) = previous_home {
        std::env::set_var("HOME", previous_home);
    } else {
        std::env::remove_var("HOME");
    }
}


/// Verifies that creating a terminal session does not inject any agent bootstrap command payload.
#[test]
fn create_terminal_agent_request_does_not_send_bootstrap_command() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(crate::app::CreateAgentKind::Terminal);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Create {
            label: Some("shell".into()),
            kind: crate::agents::AgentKind::Terminal,
            working_directory: "~/code".into(),
        }));

    run_app_commands(&mut world);

    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
    assert!(
        !world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .visible
    );
    assert!(client.sent_commands.lock().unwrap().is_empty());
    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].0, "neozeus-session-0");
    assert_eq!(created_sessions[0].1.as_deref(), Some("~/code"));
    assert!(created_sessions[0]
        .2
        .iter()
        .any(|(key, value)| key == "NEOZEUS_AGENT_LABEL" && value == "SHELL"));
    assert!(created_sessions[0]
        .2
        .iter()
        .any(|(key, value)| key == "NEOZEUS_AGENT_KIND" && value == "terminal"));
    assert!(created_sessions[0]
        .2
        .iter()
        .any(|(key, _)| key == "NEOZEUS_AGENT_UID"));
}


/// Verifies the special-case cleanup path for disconnected terminals: local state is removed even if
/// daemon-side kill returns an error.
#[test]
fn killing_disconnected_active_terminal_removes_local_state_even_if_daemon_kill_fails() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager
        .get_mut(id)
        .expect("missing terminal")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_some());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}


/// Verifies the stale-snapshot cleanup path: if the local terminal still looks interactive but the
/// daemon already reports the session as disconnected, one kill still removes the local terminal.
#[test]
fn killing_active_terminal_removes_local_state_when_daemon_already_reports_disconnected() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager
        .get_mut(id)
        .expect("missing terminal")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::running("stale local snapshot");
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_some());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}


/// Verifies that a kill failure for an otherwise live terminal preserves all local state instead of
/// tearing presentation/labels down prematurely.
#[test]
fn killing_active_terminal_preserves_local_state_when_tmux_kill_fails() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    assert_eq!(world.resource::<TerminalManager>().terminal_ids(), &[id]);
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_some());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_none());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 1);
    assert_eq!(frame_count, 1);
}

