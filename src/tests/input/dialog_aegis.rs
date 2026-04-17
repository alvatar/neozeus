//! Test submodule: `dialog_aegis` — extracted from the centralized test bucket.

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
fn plain_a_opens_aegis_dialog_for_active_terminal_with_default_prompt() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.aegis_dialog.visible);
    assert_eq!(app_session.aegis_dialog.target_agent, Some(agent_id));
    assert_eq!(
        app_session.aegis_dialog.prompt_editor.text,
        DEFAULT_AEGIS_PROMPT
    );
    assert_eq!(app_session.aegis_dialog.focus, AegisDialogField::Prompt);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


#[test]
fn disabled_agent_reopens_aegis_dialog_with_saved_prompt() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    ensure_app_command_world_resources(&mut world);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let agent_uid = world
        .resource::<crate::agents::AgentCatalog>()
        .uid(agent_id)
        .expect("agent uid should exist")
        .to_owned();
    world
        .resource_mut::<crate::aegis::AegisPolicyStore>()
        .upsert_disabled_prompt(&agent_uid, "saved custom prompt".into());

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    assert_eq!(
        world
            .resource::<AppSessionState>()
            .aegis_dialog
            .prompt_editor
            .text,
        "saved custom prompt"
    );
}


#[test]
fn aegis_dialog_enable_button_persists_custom_text_and_closes() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    ensure_app_command_world_resources(&mut world);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let agent_uid = world
        .resource::<crate::agents::AgentCatalog>()
        .uid(agent_id)
        .expect("agent uid should exist")
        .to_owned();
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session
            .aegis_dialog
            .open(agent_id, DEFAULT_AEGIS_PROMPT);
        app_session
            .aegis_dialog
            .prompt_editor
            .load_text("custom aegis prompt");
        app_session.aegis_dialog.focus = AegisDialogField::EnableButton;
    }
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert!(!world.resource::<AppSessionState>().aegis_dialog.visible);
    assert!(world
        .resource::<crate::aegis::AegisPolicyStore>()
        .is_enabled(&agent_uid));
    assert_eq!(
        world
            .resource::<crate::aegis::AegisPolicyStore>()
            .prompt_text(&agent_uid),
        Some("custom aegis prompt")
    );
    assert_eq!(
        world
            .resource::<crate::aegis::AegisRuntimeStore>()
            .state(agent_id),
        Some(crate::aegis::AegisRuntimeState::Armed)
    );
}


#[test]
fn aegis_dialog_rejects_empty_prompt() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session
            .aegis_dialog
            .open(agent_id, DEFAULT_AEGIS_PROMPT);
        app_session.aegis_dialog.prompt_editor.load_text("");
        app_session.aegis_dialog.focus = AegisDialogField::EnableButton;
    }

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.aegis_dialog.visible);
    assert_eq!(
        app_session.aegis_dialog.error.as_deref(),
        Some("Aegis prompt is required")
    );
}


#[test]
fn aegis_dialog_prompt_accepts_multiline_text_without_submitting() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.aegis_dialog.open(agent_id, "line one");
        app_session.aegis_dialog.focus = AegisDialogField::Prompt;
    }

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.aegis_dialog.visible);
    assert_eq!(app_session.aegis_dialog.prompt_editor.text, "line one\na");
}


#[test]
fn plain_a_disables_enabled_aegis_for_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    ensure_app_command_world_resources(&mut world);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let agent_uid = world
        .resource::<crate::agents::AgentCatalog>()
        .uid(agent_id)
        .expect("agent uid should exist")
        .to_owned();
    world
        .resource_mut::<crate::aegis::AegisPolicyStore>()
        .enable(&agent_uid, "custom aegis prompt".into());
    world
        .resource_mut::<crate::aegis::AegisRuntimeStore>()
        .set_state(agent_id, crate::aegis::AegisRuntimeState::Armed);

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    assert!(!world
        .resource::<crate::aegis::AegisPolicyStore>()
        .is_enabled(&agent_uid));
    assert!(world
        .resource::<crate::aegis::AegisRuntimeStore>()
        .state(agent_id)
        .is_none());
}


#[test]
fn middle_click_paste_in_aegis_dialog_inserts_prompt_text() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.aegis_dialog.open(agent_id, "");
        app_session.aegis_dialog.prompt_editor.load_text("");
    }

    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(&world)
        .expect("primary window should exist")
        .clone();
    let rect = aegis_prompt_field_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_aegis_dialog(
        &mut app_session,
        &window,
        Vec2::new(rect.x + 4.0, rect.y + 4.0),
        "continue cleanly"
    ));
    assert_eq!(
        app_session.aegis_dialog.prompt_editor.text,
        "continue cleanly"
    );
}

