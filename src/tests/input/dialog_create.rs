//! Test submodule: `dialog_create` — extracted from the centralized test bucket.

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

/// Verifies that `Tab` advances through every create-agent control, including the create button.
#[test]
fn create_agent_dialog_tab_advances_focus() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .focus,
        CreateAgentDialogField::Kind
    );

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .focus,
        CreateAgentDialogField::StartingFolder
    );

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .focus,
        CreateAgentDialogField::CreateButton
    );
}


/// Verifies that pressing `Space` toggles the selected type while the type row is focused.
#[test]
fn create_agent_dialog_space_toggles_type() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session.create_agent_dialog.focus = CreateAgentDialogField::Kind;
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));

    let session = world.resource::<AppSessionState>();
    assert_eq!(session.create_agent_dialog.kind, CreateAgentKind::Claude);
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Kind
    );
}


/// Verifies that `Ctrl+Space` in the cwd field starts completion and cycles matching directories.
#[test]
fn create_agent_dialog_ctrl_space_cycles_cwd_completions() {
    let root = unique_temp_dir("cwd-cycle");
    std::fs::create_dir_all(root.join("code")).unwrap();
    std::fs::create_dir_all(root.join("configs")).unwrap();

    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session.create_agent_dialog.focus = CreateAgentDialogField::StartingFolder;
        session
            .create_agent_dialog
            .cwd_field
            .load_text(&format!("{}/co", root.display()));
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    {
        let session = world.resource::<AppSessionState>();
        assert_eq!(
            session.create_agent_dialog.cwd_field.field.text,
            format!("{}/code/", root.display())
        );
        assert!(session.create_agent_dialog.cwd_field.completion.is_some());
    }

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    let session = world.resource::<AppSessionState>();
    assert_eq!(
        session.create_agent_dialog.cwd_field.field.text,
        format!("{}/configs/", root.display())
    );

    let _ = std::fs::remove_dir_all(root);
}


/// Verifies that `Enter` in the cwd field accepts the current completion and opens the next level.
#[test]
fn create_agent_dialog_enter_descends_into_selected_cwd_completion() {
    let root = unique_temp_dir("cwd-enter");
    std::fs::create_dir_all(root.join("code").join("alpha")).unwrap();
    std::fs::create_dir_all(root.join("configs")).unwrap();

    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session.create_agent_dialog.focus = CreateAgentDialogField::StartingFolder;
        session
            .create_agent_dialog
            .cwd_field
            .load_text(&format!("{}/co", root.display()));
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    world.insert_resource(ButtonInput::<KeyCode>::default());
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let session = world.resource::<AppSessionState>();
    assert_eq!(
        session.create_agent_dialog.cwd_field.field.text,
        format!("{}/code/", root.display())
    );
    let completion = session
        .create_agent_dialog
        .cwd_field
        .completion
        .as_ref()
        .expect("next-level completion should stay open");
    assert!(!completion.preview_active);
    assert_eq!(
        completion.items[0].completion_text,
        format!("{}/code/alpha/", root.display())
    );

    let _ = std::fs::remove_dir_all(root);
}


/// Verifies that `Ctrl+U` clears the create-agent name field.
#[test]
fn create_agent_dialog_ctrl_u_clears_name_field() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session.create_agent_dialog.name_field.load_text("ORACLE");
        session.create_agent_dialog.focus = CreateAgentDialogField::Name;
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyU, Key::Character("u".into())),
    );

    let session = world.resource::<AppSessionState>();
    assert_eq!(session.create_agent_dialog.name_field.text, "");
    assert_eq!(session.create_agent_dialog.name_field.cursor, 0);
}


/// Verifies that typed create-agent names are uppercased immediately in the field.
#[test]
fn create_agent_dialog_typing_uppercases_name_field() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyB, Some("b")));

    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .name_field
            .text,
        "AB"
    );
}


#[test]
fn middle_click_paste_in_create_agent_dialog_inserts_into_text_fields() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
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
    let name_rect = create_agent_name_field_rect(&window);
    let cwd_rect = create_agent_starting_folder_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_create_agent_dialog(
        &mut app_session,
        &window,
        Vec2::new(name_rect.x + 4.0, name_rect.y + 4.0),
        "mixedCase",
    ));
    assert!(paste_into_create_agent_dialog(
        &mut app_session,
        &window,
        Vec2::new(cwd_rect.x + 4.0, cwd_rect.y + 4.0),
        "/tmp/work",
    ));

    assert_eq!(app_session.create_agent_dialog.name_field.text, "MIXEDCASE");
    assert_eq!(
        app_session.create_agent_dialog.cwd_field.field.text,
        "~/code/tmp/work"
    );
}


/// Verifies that `Escape` cancels the create-agent dialog without spawning anything.
#[test]
fn create_agent_dialog_escape_closes_without_spawning() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
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
            .create_agent_dialog
            .visible
    );
}


/// Verifies that submitting the create-agent dialog emits the configured agent-create command.
#[test]
fn create_agent_dialog_submit_emits_create_command() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Terminal);
        session.create_agent_dialog.name_field.load_text("oracle");
        session.create_agent_dialog.cwd_field.load_text("~/code");
        session.create_agent_dialog.focus = CreateAgentDialogField::CreateButton;
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
        vec![AppCommand::Agent(AppAgentCommand::Create {
            label: Some("ORACLE".into()),
            kind: crate::agents::AgentKind::Terminal,
            working_directory: "~/code".into(),
        })]
    );
}


/// Verifies that `Ctrl+U` clears the create-agent cwd field.
#[test]
fn create_agent_dialog_ctrl_u_clears_cwd_field() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session
            .create_agent_dialog
            .cwd_field
            .load_text("~/code/project");
        session.create_agent_dialog.focus = CreateAgentDialogField::StartingFolder;
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyU, Key::Character("u".into())),
    );

    let session = world.resource::<AppSessionState>();
    assert_eq!(session.create_agent_dialog.cwd_field.field.text, "");
    assert_eq!(session.create_agent_dialog.cwd_field.field.cursor, 0);
}

