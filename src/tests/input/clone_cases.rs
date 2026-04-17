//! Legacy test sibling rewired post-split: pulls imports + helpers via support.

#![allow(unused_imports)]

use super::super::{
    capturing_bridge, ensure_shared_app_command_test_resources, fake_runtime_spawner,
    init_git_repo, insert_default_hud_resources, insert_terminal_manager_resources,
    insert_test_hud_state, pressed_text, snapshot_test_hud_state, test_bridge,
    write_pi_session_file, FakeDaemonClient,
};
use crate::{
    aegis::DEFAULT_AEGIS_PROMPT,
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::{
        AegisDialogField, AgentCommand as AppAgentCommand, AppCommand, AppSessionState,
        AppStatePersistenceState, CloneAgentDialogField, CreateAgentDialogField, CreateAgentKind,
        RenameAgentDialogField, TaskCommand as AppTaskCommand,
    },
    composer::{
        aegis_prompt_field_rect, clone_agent_name_field_rect, create_agent_name_field_rect,
        create_agent_starting_folder_rect, message_box_rect, rename_agent_name_field_rect,
        task_dialog_rect, MessageDialogFocus, TaskDialogFocus,
    },
    conversations::{AgentTaskStore, ConversationStore, MessageTransportAdapter},
    hud::{handle_hud_module_shortcuts, TerminalVisibilityState},
    input::{
        ctrl_sequence, focus_terminal_on_panel_click, handle_global_terminal_spawn_shortcut,
        handle_keyboard_input, handle_terminal_direct_input_keyboard,
        handle_terminal_lifecycle_shortcuts, handle_terminal_message_box_keyboard,
        handle_terminal_text_selection, hide_terminal_on_background_click,
        keyboard_input_to_terminal_command, paste_into_aegis_dialog, paste_into_clone_agent_dialog,
        paste_into_create_agent_dialog, paste_into_direct_input_terminal,
        paste_into_message_dialog, paste_into_rename_agent_dialog, paste_into_task_dialog,
        scroll_terminal_with_mouse_wheel, should_exit_application, should_kill_active_terminal,
        should_spawn_terminal_globally, zoom_terminal_view,
    },
    terminals::{
        TerminalCommand, TerminalManager, TerminalNotesState, TerminalPanel, TerminalPresentation,
        TerminalUpdate,
    },
};
use bevy::{
    app::AppExit,
    ecs::system::RunSystemOnce,
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::{MouseScrollUnit, MouseWheel},
        ButtonInput, ButtonState,
    },
    prelude::{
        Entity, KeyCode, Messages, MouseButton, Query, Res, Single, Time, Vec2, Visibility, Window,
        With, World,
    },
    window::{PrimaryWindow, RequestRedraw},
};
use std::time::{Duration, Instant};


use super::support::*;

#[test]
fn global_clone_shortcut_opens_clone_agent_dialog_for_selected_pi_agent() {
    let mut world = World::default();
    let window = Window {
        focused: true,
        ..Default::default()
    };
    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent_with_metadata(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
        crate::agents::AgentMetadata {
            clone_source_session_path: Some("/tmp/pi-alpha.jsonl".into()),
            recovery: None,
        },
    );
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyC, Some("c")));

    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();

    let session = world.resource::<AppSessionState>();
    assert!(session.clone_agent_dialog.visible);
    assert_eq!(session.clone_agent_dialog.source_agent, Some(agent_id));
    assert_eq!(session.clone_agent_dialog.name_field.text, "ALPHA-CLONE");
    assert_eq!(
        session.clone_agent_dialog.focus,
        CloneAgentDialogField::Name
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn global_clone_shortcut_does_nothing_for_tmux_selection() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(crate::hud::AgentListSelection::OwnedTmux("tmux-1".into()));
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyC, Some("c")));

    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();

    assert!(
        !world
            .resource::<AppSessionState>()
            .clone_agent_dialog
            .visible
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 0);
}

#[test]
fn global_clone_shortcut_opens_clone_agent_dialog_for_recoverable_provider_agent() {
    for kind in [
        crate::agents::AgentKind::Claude,
        crate::agents::AgentKind::Codex,
    ] {
        let mut world = World::default();
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent_with_metadata(
            Some("alpha".into()),
            kind,
            kind.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: None,
                recovery: Some(match kind {
                    crate::agents::AgentKind::Claude => crate::agents::AgentRecoverySpec::Claude {
                        session_id: "claude-parent".into(),
                        cwd: "/tmp/demo".into(),
                        model: None,
                        profile: None,
                    },
                    crate::agents::AgentKind::Codex => crate::agents::AgentRecoverySpec::Codex {
                        session_id: "codex-parent".into(),
                        cwd: "/tmp/demo".into(),
                        model: None,
                        profile: None,
                    },
                    _ => unreachable!(),
                }),
            },
        );
        world.insert_resource(ButtonInput::<KeyCode>::default());
        world.insert_resource(catalog);
        world.insert_resource(AgentRuntimeIndex::default());
        world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
        insert_default_hud_resources(&mut world);
        world.init_resource::<Messages<RequestRedraw>>();
        world.init_resource::<Messages<KeyboardInput>>();
        world.spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            PrimaryWindow,
        ));
        world
            .resource_mut::<Messages<KeyboardInput>>()
            .write(pressed_text(KeyCode::KeyC, Some("c")));

        world
            .run_system_once(handle_global_terminal_spawn_shortcut)
            .unwrap();

        let session = world.resource::<AppSessionState>();
        assert!(session.clone_agent_dialog.visible);
        assert_eq!(session.clone_agent_dialog.source_agent, Some(agent_id));
        assert_eq!(session.clone_agent_dialog.source_kind, Some(kind));
        assert!(!session.clone_agent_dialog.supports_workdir());
    }
}

#[test]
fn global_clone_shortcut_does_nothing_for_non_pi_agent() {
    let mut world = World::default();
    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyC, Some("c")));

    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();

    assert!(
        !world
            .resource::<AppSessionState>()
            .clone_agent_dialog
            .visible
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 0);
}

#[test]
fn global_clone_shortcut_does_nothing_while_modal_has_keyboard_capture() {
    let mut world = World::default();
    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent_with_metadata(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
        crate::agents::AgentMetadata {
            clone_source_session_path: Some("/tmp/pi-alpha.jsonl".into()),
            recovery: None,
        },
    );
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    insert_default_hud_resources(&mut world);
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyC, Some("c")));

    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();

    assert!(
        !world
            .resource::<AppSessionState>()
            .clone_agent_dialog
            .visible
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 0);
}

#[test]
fn end_to_end_clone_shortcut_plain_clone_creates_agent() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    let source_root = std::path::PathBuf::from("/tmp").join(format!(
        "neozeus-input-clone-source-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&source_root).unwrap();
    let source_session = source_root.join("source.jsonl");
    write_pi_session_file(&source_session, source_root.to_str().unwrap());

    ensure_app_command_world_resources(&mut world);
    insert_default_hud_resources(&mut world);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("alpha".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );
    world.insert_resource(crate::hud::AgentListSelection::Agent(source_agent));
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyC, Some("c")));
    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));
    run_app_command_cycle(&mut world);

    let catalog = world.resource::<AgentCatalog>();
    assert_eq!(catalog.order.len(), 2);
    let clone_agent = *catalog.order.last().unwrap();
    assert_eq!(catalog.label(clone_agent), Some("ALPHA-CLONE"));
    assert!(!catalog.is_workdir(clone_agent));
    assert_eq!(client.created_sessions.lock().unwrap().len(), 1);
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(clone_agent)
    );
}

#[test]
fn end_to_end_clone_shortcut_workdir_clone_creates_agent() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    let repo = init_git_repo("neozeus-input-clone-worktree");
    let source_session = repo.join("source.jsonl");
    write_pi_session_file(&source_session, repo.to_str().unwrap());

    ensure_app_command_world_resources(&mut world);
    insert_default_hud_resources(&mut world);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("alpha".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );
    world.insert_resource(crate::hud::AgentListSelection::Agent(source_agent));
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyC, Some("c")));
    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));
    run_app_command_cycle(&mut world);

    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().unwrap();
    assert_eq!(catalog.label(clone_agent), Some("ALPHA-CLONE"));
    assert!(catalog.is_workdir(clone_agent));
    let clone_session_path = catalog
        .clone_source_session_path(clone_agent)
        .unwrap()
        .to_owned();
    let clone_header =
        crate::shared::pi_session_files::read_session_header(&clone_session_path).unwrap();
    assert_eq!(
        std::path::PathBuf::from(clone_header.cwd),
        repo.join(".worktrees").join("ALPHA-CLONE")
    );
    assert_eq!(client.created_sessions.lock().unwrap().len(), 1);
}

#[test]
fn clone_agent_dialog_tab_advances_focus() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .clone_agent_dialog
        .open(
            crate::agents::AgentId(7),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world.resource::<AppSessionState>().clone_agent_dialog.focus,
        CloneAgentDialogField::Workdir
    );

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world.resource::<AppSessionState>().clone_agent_dialog.focus,
        CloneAgentDialogField::CloneButton
    );
}

#[test]
fn clone_agent_dialog_space_toggles_workdir() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.clone_agent_dialog.open(
            crate::agents::AgentId(7),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
        session.clone_agent_dialog.focus = CloneAgentDialogField::Workdir;
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));

    assert!(
        world
            .resource::<AppSessionState>()
            .clone_agent_dialog
            .workdir
    );
}

#[test]
fn clone_agent_dialog_escape_closes_without_emitting_command() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .clone_agent_dialog
        .open(
            crate::agents::AgentId(7),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
    ensure_app_command_world_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Escape, Key::Escape));

    assert!(
        !world
            .resource::<AppSessionState>()
            .clone_agent_dialog
            .visible
    );
    assert!(drain_hud_commands(&mut world).is_empty());
}

#[test]
fn clone_agent_dialog_submit_emits_clone_command() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.clone_agent_dialog.open(
            crate::agents::AgentId(7),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
        session.clone_agent_dialog.name_field.load_text("child");
        session.clone_agent_dialog.workdir = true;
        session.clone_agent_dialog.focus = CloneAgentDialogField::CloneButton;
    }
    ensure_app_command_world_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::Enter, Key::Enter));
    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: crate::agents::AgentId(7),
            label: "CHILD".into(),
            workdir: true,
        })]
    );
}

#[test]
fn paste_into_clone_agent_dialog_inserts_into_name_field() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .clone_agent_dialog
        .open(
            crate::agents::AgentId(7),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(&world)
        .expect("primary window should exist")
        .clone();
    let name_rect = clone_agent_name_field_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_clone_agent_dialog(
        &mut app_session,
        &window,
        Vec2::new(name_rect.x + 4.0, name_rect.y + 4.0),
        "child",
    ));
    assert_eq!(
        app_session.clone_agent_dialog.name_field.text,
        "ALPHA-CLONECHILD"
    );
}
