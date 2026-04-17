//! Test submodule: `terminal_keys` — extracted from the centralized test bucket.

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

/// Verifies that plain `n` enqueues the consume-next-task intent for the active terminal.
#[test]
fn plain_n_enqueues_consume_next_task_for_active_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyN, Some("n")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Task(AppTaskCommand::ConsumeNext { agent_id })]
    );
}


#[test]
fn plain_p_toggles_paused_state_for_active_terminal_agent() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyP, Some("p")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);
    assert!(world.resource::<AgentCatalog>().is_paused(agent_id));

    world.insert_resource(Messages::<AppCommand>::default());
    world.insert_resource(Messages::<KeyboardInput>::default());
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyP, Key::Character("P".into())));
    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);
    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
}


#[test]
fn plain_p_toggles_only_once_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<AppSessionState>()
        .focus_intent
        .focus_agent(agent_id, crate::app::VisibilityMode::ShowAll);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyP, Some("p")));

    assert!(
        world.resource::<AgentCatalog>().is_paused(agent_id),
        "plain p should toggle once even when terminal and HUD shortcut systems both run"
    );
}


#[test]
fn plain_p_toggles_selected_focus_agent_without_active_terminal_target() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let _ = world
        .resource_mut::<crate::terminals::TerminalFocusState>()
        .clear_active_terminal();
    world
        .resource_mut::<AppSessionState>()
        .focus_intent
        .focus_agent(agent_id, crate::app::VisibilityMode::ShowAll);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyP, Some("p")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert!(
        world.resource::<AgentCatalog>().is_paused(agent_id),
        "plain p should still toggle the selected focused agent when no interactive terminal owns the shortcut"
    );
}


#[test]
fn shift_p_does_not_toggle_paused_state_for_active_terminal_agent() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyP, Key::Character("P".into())));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);
    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
}


#[test]
fn shift_p_does_not_toggle_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::KeyP, Key::Character("P".into())),
    );

    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
}


#[test]
fn plain_i_toggles_selected_agent_context_box_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));

    assert!(
        !world
            .resource::<crate::hud::AgentListUiState>()
            .show_selected_context,
        "precondition: selected-agent context box should start disabled"
    );
    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyI, Some("i")));
    assert!(
        world
            .resource::<crate::hud::AgentListUiState>()
            .show_selected_context
    );

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::KeyI, Key::Character("I".into())),
    );
    assert!(
        !world
            .resource::<crate::hud::AgentListUiState>()
            .show_selected_context
    );
}


#[test]
fn shift_i_does_not_toggle_selected_agent_context_box() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::KeyI, Key::Character("I".into())),
    );

    assert!(
        !world
            .resource::<crate::hud::AgentListUiState>()
            .show_selected_context
    );
}

