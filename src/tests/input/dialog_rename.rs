//! Test submodule: `dialog_rename` — extracted from the centralized test bucket.

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

/// Verifies that plain `r` opens the rename-agent dialog prefilled from the active agent label.
#[test]
fn plain_r_opens_rename_dialog_for_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyR, Some("r")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();

    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.rename_agent_dialog.visible);
    assert_eq!(app_session.rename_agent_dialog.target_agent, Some(agent_id));
    assert_eq!(app_session.rename_agent_dialog.name_field.text, "AGENT-1");
    assert_eq!(
        app_session.rename_agent_dialog.focus,
        RenameAgentDialogField::Name
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


/// Verifies that confirming the rename dialog renames the active agent and closes the modal.
#[test]
fn rename_dialog_enter_submits_agent_rename() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session
            .rename_agent_dialog
            .name_field
            .load_text("renamed");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::RenameButton;
    }
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        world
            .resource::<crate::agents::AgentCatalog>()
            .label(agent_id),
        Some("RENAMED")
    );
    assert!(
        !world
            .resource::<AppSessionState>()
            .rename_agent_dialog
            .visible
    );
}


#[test]
fn rename_dialog_updates_live_daemon_metadata() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let session_name = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .session_name(agent_id)
        .unwrap()
        .to_owned();
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session
            .rename_agent_dialog
            .name_field
            .load_text("renamed");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::RenameButton;
    }
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        client
            .session_metadata
            .lock()
            .unwrap()
            .get(&session_name)
            .and_then(|metadata| metadata.agent_label.as_deref()),
        Some("RENAMED")
    );
}


/// Verifies that typed rename values are uppercased immediately in the field.
#[test]
fn rename_dialog_typing_uppercases_name_field() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session.rename_agent_dialog.name_field.clear();
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
    }
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyB, Some("b")));

    assert_eq!(
        world
            .resource::<AppSessionState>()
            .rename_agent_dialog
            .name_field
            .text,
        "AB"
    );
}


#[test]
fn reset_dialog_escape_preempts_rename_dialog() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.reset_dialog.visible = true;
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
    }
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Escape, Key::Escape));

    let app_session = world.resource::<AppSessionState>();
    assert!(!app_session.reset_dialog.visible);
    assert!(app_session.rename_agent_dialog.visible);
}


#[test]
fn rename_dialog_typing_preempts_message_editor_typing() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session.rename_agent_dialog.name_field.clear();
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
        app_session.composer.message_editor.load_text("draft");
    }
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyZ, Some("z")));

    let app_session = world.resource::<AppSessionState>();
    assert_eq!(app_session.rename_agent_dialog.name_field.text, "Z");
    assert_eq!(app_session.composer.message_editor.text, "draft");
}


#[test]
fn rename_dialog_ctrl_u_clears_name_field() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session
            .rename_agent_dialog
            .name_field
            .load_text("RENAMED");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
    }
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyU, Key::Character("u".into())),
    );

    let app_session = world.resource::<AppSessionState>();
    assert_eq!(app_session.rename_agent_dialog.name_field.text, "");
    assert_eq!(app_session.rename_agent_dialog.name_field.cursor, 0);
}


#[test]
fn middle_click_paste_in_rename_dialog_uppercases_text() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session.rename_agent_dialog.name_field.clear();
    }

    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(&world)
        .expect("primary window should exist")
        .clone();
    let rect = rename_agent_name_field_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_rename_agent_dialog(
        &mut app_session,
        &window,
        Vec2::new(rect.x + 4.0, rect.y + 4.0),
        "renamed",
    ));
    assert_eq!(app_session.rename_agent_dialog.name_field.text, "RENAMED");
}


#[test]
fn rename_dialog_keeps_local_label_unchanged_when_metadata_sync_fails() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_update_session_metadata.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client));
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session
            .rename_agent_dialog
            .name_field
            .load_text("renamed");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::RenameButton;
    }
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        world
            .resource::<crate::agents::AgentCatalog>()
            .label(agent_id),
        Some("AGENT-1")
    );
    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.rename_agent_dialog.visible);
    assert_eq!(
        app_session.rename_agent_dialog.error.as_deref(),
        Some("update metadata failed")
    );
}


/// Verifies that duplicate rename targets are rejected and keep the rename dialog open.
#[test]
fn rename_dialog_rejects_duplicate_agent_name() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world
        .resource_mut::<crate::agents::AgentCatalog>()
        .create_agent(
            Some("beta".into()),
            crate::agents::AgentKind::Terminal,
            crate::agents::AgentCapabilities::terminal_defaults(),
        );
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session.rename_agent_dialog.name_field.load_text("beta");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::RenameButton;
    }
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        world
            .resource::<crate::agents::AgentCatalog>()
            .label(agent_id),
        Some("AGENT-1")
    );
    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.rename_agent_dialog.visible);
    assert_eq!(
        app_session.rename_agent_dialog.error.as_deref(),
        Some("agent `BETA` already exists")
    );
}

