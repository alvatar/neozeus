//! Test submodule: `global_shortcuts` — extracted from the centralized test bucket.

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

/// Verifies a few representative control-sequence mappings used by terminal keyboard translation.
#[test]
fn ctrl_sequence_maps_common_shortcuts() {
    assert_eq!(ctrl_sequence(KeyCode::KeyC), Some("\u{3}"));
    assert_eq!(ctrl_sequence(KeyCode::KeyL), Some("\u{c}"));
    assert_eq!(ctrl_sequence(KeyCode::Enter), None);
}


/// Verifies that ordinary printable key events become `InputText` terminal commands.
#[test]
fn plain_text_uses_text_payload() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyA, Some("a"));
    let command = keyboard_input_to_terminal_command(&event, &keys);
    match command {
        Some(TerminalCommand::InputText(text)) => assert_eq!(text, "a"),
        _ => panic!("expected text input command"),
    }
}


#[test]
fn widget_toggle_and_reset_work_in_full_keyboard_path() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut hud_state = crate::hud::HudState::default();
    hud_state.insert_default_module(crate::hud::HudWidgetKey::AgentList);
    insert_test_hud_state(&mut world, hud_state);

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_text(KeyCode::Digit1, Some("1")),
    );
    assert!(!world
        .resource::<crate::hud::HudLayoutState>()
        .module_enabled(crate::hud::HudWidgetKey::AgentList));

    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::Digit1, Key::Character("!".into())),
    );
    assert!(world
        .resource::<crate::hud::HudLayoutState>()
        .module_enabled(crate::hud::HudWidgetKey::AgentList));
}


/// Verifies that the global spawn shortcut is accepted only for an unmodified physical `z` key press.
#[test]
fn global_spawn_shortcut_only_uses_plain_z() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyZ, Some("z"));
    assert!(should_spawn_terminal_globally(&event, &keys));

    let capslock_like_event = pressed_key(KeyCode::KeyZ, Key::Character("Z".into()));
    assert!(should_spawn_terminal_globally(&capslock_like_event, &keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_spawn_terminal_globally(&event, &ctrl_keys));

    let mut shift_keys = ButtonInput::<KeyCode>::default();
    shift_keys.press(KeyCode::ShiftLeft);
    assert!(!should_spawn_terminal_globally(&event, &shift_keys));
}


/// Verifies that the global spawn shortcut opens the centered create-agent dialog even when another
/// terminal is already active.
#[test]
fn global_spawn_shortcut_opens_create_agent_dialog_even_with_active_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);

    let mut world = World::default();
    let window = Window {
        focused: true,
        ..Default::default()
    };
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyZ, Some("z")));

    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();

    let session = world.resource::<AppSessionState>();
    assert!(session.create_agent_dialog.visible);
    assert_eq!(session.create_agent_dialog.kind, CreateAgentKind::Pi);
    assert_eq!(session.create_agent_dialog.name_field.text, "");
    assert_eq!(session.create_agent_dialog.cwd_field.field.text, "~/code");
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Name
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


/// Verifies that the kill-active-terminal shortcut is accepted for `Ctrl+k`, regardless of Shift,
/// and still rejects unrelated modifier mixes.
#[test]
fn kill_active_terminal_shortcut_accepts_ctrl_k_even_with_shift() {
    let event = pressed_text(KeyCode::KeyK, Some("k"));
    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(should_kill_active_terminal(&event, &ctrl_keys));

    let mut shift_ctrl_keys = ButtonInput::<KeyCode>::default();
    shift_ctrl_keys.press(KeyCode::ControlLeft);
    shift_ctrl_keys.press(KeyCode::ShiftLeft);
    assert!(should_kill_active_terminal(&event, &shift_ctrl_keys));

    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(!should_kill_active_terminal(&event, &plain_keys));

    let mut alt_ctrl_keys = ButtonInput::<KeyCode>::default();
    alt_ctrl_keys.press(KeyCode::ControlLeft);
    alt_ctrl_keys.press(KeyCode::AltLeft);
    assert!(!should_kill_active_terminal(&event, &alt_ctrl_keys));
}


#[test]
fn ctrl_alt_r_opens_reset_dialog_without_emitting_command() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyR, Some("r")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(world.resource::<AppSessionState>().reset_dialog.visible);
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset requested: confirmation required")
    );
    assert!(world.resource::<Messages<AppCommand>>().is_empty());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


#[test]
fn ctrl_alt_shift_r_still_opens_reset_dialog_without_emitting_command() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    keys.press(KeyCode::ShiftLeft);
    world.insert_resource(keys);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyR, Key::Character("R".into())));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(world.resource::<AppSessionState>().reset_dialog.visible);
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset requested: confirmation required")
    );
    assert!(world.resource::<Messages<AppCommand>>().is_empty());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


#[test]
fn ctrl_alt_r_opens_reset_dialog_in_full_keyboard_path() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyR, Some("r")));

    assert!(world.resource::<AppSessionState>().reset_dialog.visible);
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset requested: confirmation required")
    );
    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}


#[test]
fn ctrl_alt_r_is_suppressed_while_other_modal_has_keyboard_capture() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyR, Some("r")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(!world.resource::<AppSessionState>().reset_dialog.visible);
    assert!(world
        .resource::<AppSessionState>()
        .recovery_status
        .title
        .is_none());
    assert!(world.resource::<Messages<AppCommand>>().is_empty());
}


/// Verifies that the application-exit shortcut ignores Shift and only rejects Ctrl/Alt/Super.
#[test]
fn exit_application_shortcut_ignores_shift() {
    let event = pressed_text(KeyCode::F10, None);
    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(should_exit_application(&event, &plain_keys));

    let mut shift_keys = ButtonInput::<KeyCode>::default();
    shift_keys.press(KeyCode::ShiftLeft);
    assert!(should_exit_application(&event, &shift_keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_exit_application(&event, &ctrl_keys));

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    assert!(!should_exit_application(&event, &alt_keys));
}


/// Verifies that one plain `Ctrl+k` removes a disconnected active terminal in one shot.
#[test]
fn ctrl_k_removes_disconnected_active_terminal_in_one_press() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client));
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        None
    );
}


#[test]
fn ctrl_k_removes_disconnected_active_terminal_in_full_keyboard_path() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client));
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyK, Some("k")));

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        None
    );
}


/// Verifies that one plain `Ctrl+k` still removes the active terminal when the local runtime
/// snapshot is stale but the daemon already reports that session as disconnected.
#[test]
fn ctrl_k_removes_terminal_when_daemon_runtime_is_disconnected_but_local_snapshot_is_stale() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let session_name = world
        .resource::<TerminalManager>()
        .get(terminal_id)
        .expect("terminal should exist")
        .session_name
        .clone();
    client.set_session_runtime(
        &session_name,
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );
    world.insert_resource(fake_runtime_spawner(client));
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        None
    );
}


/// Verifies that the lifecycle shortcut handler turns `F10` into an app-exit message.
#[test]
fn f10_enqueues_app_exit() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::F10, None));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert_eq!(world.resource::<Messages<AppExit>>().len(), 1);
    assert!(drain_hud_commands(&mut world).is_empty());
}


/// Verifies that agent-list keyboard navigation lands on owned tmux child rows.
#[test]
fn ctrl_k_kills_selected_agent_without_hidden_session_state() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::None
    );
}


/// Verifies that global lifecycle shortcuts are ignored while the message box owns keyboard capture.
#[test]
fn lifecycle_shortcuts_are_suppressed_while_message_box_is_open() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(crate::terminals::TerminalId(1));
    insert_test_hud_state(&mut world, hud_state);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}


/// Verifies that `Ctrl+k` kills the selected owned tmux row before touching the active agent.
#[test]
fn ctrl_k_kills_selected_owned_tmux_session_before_selected_agent_row() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), None);
    world.insert_resource(crate::hud::AgentListSelection::OwnedTmux(
        "tmux-session-1".into(),
    ));
    init_hud_commands(&mut world);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::KillSelected
        )]
    );
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}


#[test]
fn lifecycle_shortcuts_are_suppressed_while_direct_input_is_open() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(crate::terminals::TerminalId(1));
    insert_test_hud_state(&mut world, hud_state);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}

