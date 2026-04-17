//! Test submodule: `scroll_wheel` — extracted from the centralized test bucket.

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
fn wheel_scroll_sends_scrollback_to_focused_terminal_in_visual_mode() {
    let (mut world, _terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);

    dispatch_terminal_wheel(&mut world, wheel_lines(2.0));
    dispatch_terminal_wheel(&mut world, wheel_lines(-3.0));

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(2)
    );
    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(-3)
    );
}


#[test]
fn wheel_scroll_sends_scrollback_to_direct_input_terminal() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);

    dispatch_terminal_wheel(&mut world, wheel_lines(1.0));

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(1)
    );
}


#[test]
fn wheel_scroll_accumulates_fractional_pixel_deltas() {
    let (mut world, _terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);

    dispatch_terminal_wheel(&mut world, wheel_pixels(10.0));
    dispatch_terminal_wheel(&mut world, wheel_pixels(10.0));
    assert!(input_rx.try_recv().is_err());

    dispatch_terminal_wheel(&mut world, wheel_pixels(10.0));
    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(1)
    );
}


#[test]
fn control_v_scrolls_many_rows_down_when_terminal_is_not_captured() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);

    dispatch_terminal_ui_key(
        &mut world,
        pressed_key(KeyCode::KeyV, Key::Character("v".into())),
    );

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(-39)
    );
}


#[test]
fn control_shift_v_still_scrolls_many_rows_down_when_terminal_is_not_captured() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);

    dispatch_terminal_ui_key(
        &mut world,
        pressed_key(KeyCode::KeyV, Key::Character("V".into())),
    );

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(-39)
    );
}


#[test]
fn alt_v_scrolls_many_rows_up_when_terminal_is_not_captured() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);

    dispatch_terminal_ui_key(
        &mut world,
        pressed_key(KeyCode::KeyV, Key::Character("v".into())),
    );

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(39)
    );
}


#[test]
fn alt_shift_v_still_scrolls_many_rows_up_when_terminal_is_not_captured() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);

    dispatch_terminal_ui_key(
        &mut world,
        pressed_key(KeyCode::KeyV, Key::Character("V".into())),
    );

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(39)
    );
}


#[test]
fn shift_wheel_keeps_zoom_and_does_not_send_scrollback() {
    let (mut world, _terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    let starting_distance = world
        .resource::<crate::terminals::TerminalViewState>()
        .distance;

    dispatch_terminal_wheel(&mut world, wheel_lines(2.0));

    assert!(input_rx.try_recv().is_err());
    assert!(
        world
            .resource::<crate::terminals::TerminalViewState>()
            .distance
            < starting_distance
    );
}

