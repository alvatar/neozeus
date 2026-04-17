//! Test submodule: `direct_input` — extracted from the centralized test bucket.

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
fn direct_input_route_suppresses_primary_pause_shortcut_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    assert_eq!(
        world
            .resource::<crate::hud::HudInputCaptureState>()
            .direct_input_terminal,
        Some(terminal_id)
    );

    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyP, Some("p")));

    assert_eq!(world.resource::<Messages<AppCommand>>().len(), 0);
    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
    assert!(!snapshot_test_hud_state(&world).message_box.visible);
}


/// Verifies that repeated `Ctrl+Enter` presses toggle direct terminal input mode on and off for the
/// active terminal.
#[test]
fn ctrl_enter_toggles_direct_input_mode_for_active_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<RequestRedraw>>();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);

    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.direct_input_terminal, Some(terminal_id));
    assert!(!hud_state.message_box.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);

    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.direct_input_terminal, None);
    assert!(!hud_state.message_box.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 2);
}


/// Verifies that the real scheduled keyboard pipeline also opens and closes direct input with
/// `Ctrl+Enter` instead of dropping the shortcut in the primary route.
#[test]
fn ctrl_enter_toggles_direct_input_mode_in_full_keyboard_pipeline() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<RequestRedraw>>();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.direct_input_terminal, Some(terminal_id));
    assert!(!hud_state.message_box.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.direct_input_terminal, None);
    assert!(!hud_state.message_box.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 2);
}


#[test]
fn direct_input_echo_can_be_polled_before_raster_in_same_cycle() {
    let (mut world, terminal_id, input_rx, mailbox) =
        world_with_active_terminal_and_receiver_and_mailbox(
            Vec2::new(10.0, 10.0),
            false,
            Vec2::ZERO,
        );
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    world.init_resource::<Messages<RequestRedraw>>();

    world.insert_resource(Messages::<KeyboardInput>::default());
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyA, Some("a")));
    world
        .run_system_once(handle_terminal_direct_input_keyboard)
        .unwrap();

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::InputText("a".into())
    );

    assert!(mailbox.push(TerminalUpdate::Status {
        runtime: crate::terminals::TerminalRuntimeState::running("echoed"),
        surface: Some(crate::tests::surface_with_text(4, 1, 0, "a")),
    }));

    world.init_resource::<Messages<RequestRedraw>>();
    world
        .run_system_once(crate::terminals::poll_terminal_snapshots)
        .unwrap();

    let terminal_manager = world.resource::<TerminalManager>();
    let terminal = terminal_manager
        .get(terminal_id)
        .expect("terminal should exist");
    assert_eq!(terminal.surface_revision, 1);
    assert_eq!(
        terminal.pending_damage,
        Some(crate::terminals::TerminalDamage::Full)
    );
    assert!(terminal.snapshot.surface.is_some());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


#[test]
fn direct_input_typing_burst_stays_close_to_noop_baseline() {
    let (mut baseline_world, _baseline_terminal) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    init_hud_commands(&mut baseline_world);
    baseline_world.init_resource::<Messages<RequestRedraw>>();

    let baseline_started = Instant::now();
    for _ in 0..DIRECT_INPUT_TYPING_BURST_KEYS {
        dispatch_terminal_ui_key(&mut baseline_world, pressed_text(KeyCode::KeyA, Some("a")));
    }
    let baseline_elapsed = baseline_started.elapsed();

    let (mut hot_world, terminal_id, _input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut hot_world, hud_state);
    init_hud_commands(&mut hot_world);
    hot_world.init_resource::<Messages<RequestRedraw>>();

    let hot_started = Instant::now();
    for _ in 0..DIRECT_INPUT_TYPING_BURST_KEYS {
        dispatch_terminal_ui_key(&mut hot_world, pressed_text(KeyCode::KeyA, Some("a")));
    }
    let hot_elapsed = hot_started.elapsed();
    let baseline_nanos = baseline_elapsed.as_nanos().max(1) as f64;
    let overhead_ratio = hot_elapsed.as_nanos() as f64 / baseline_nanos;

    assert!(
        overhead_ratio <= DIRECT_INPUT_TYPING_OVERHEAD_RATIO_MAX,
        "direct terminal typing burst regressed: noop baseline={}µs hot-path={}µs ratio={:.2} max={:.2}",
        baseline_elapsed.as_micros(),
        hot_elapsed.as_micros(),
        overhead_ratio,
        DIRECT_INPUT_TYPING_OVERHEAD_RATIO_MAX
    );
}


#[test]
fn direct_input_end_scrolls_terminal_to_bottom_without_new_wire_command() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    {
        let mut manager = world.resource_mut::<TerminalManager>();
        let terminal = manager.get_mut(terminal_id).expect("terminal should exist");
        terminal
            .snapshot
            .surface
            .as_mut()
            .expect("surface should exist")
            .display_offset = 11;
    }

    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::End, Key::End));

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(-11)
    );
}


#[test]
fn direct_input_page_keys_jump_by_visible_rows() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    set_terminal_surface_rows(&mut world, terminal_id, 40);

    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::PageUp, Key::PageUp));
    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::PageDown, Key::PageDown));

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(39)
    );
    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(-39)
    );
}


#[test]
fn middle_click_paste_sends_clipboard_to_direct_input_terminal() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(640.0, 360.0), true, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);

    world
        .run_system_once(
            |primary_window: Single<&Window, With<PrimaryWindow>>,
             layout_state: Res<crate::hud::HudLayoutState>,
             terminal_manager: Res<TerminalManager>,
             focus_state: Res<crate::terminals::TerminalFocusState>,
             input_capture: Res<crate::hud::HudInputCaptureState>,
             panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>| {
                assert!(paste_into_direct_input_terminal(
                    &primary_window,
                    Vec2::new(640.0, 360.0),
                    &layout_state,
                    &terminal_manager,
                    &focus_state,
                    &input_capture,
                    &panels,
                    "hello from paste",
                ));
            },
        )
        .unwrap();

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::InputText("hello from paste".into())
    );
}


/// Verifies that `Ctrl+Enter` refuses to open direct-input mode for a disconnected terminal.
#[test]
fn ctrl_enter_does_not_open_direct_input_for_disconnected_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<RequestRedraw>>();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    world
        .resource_mut::<crate::terminals::TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");

    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.direct_input_terminal, None);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 0);
}


/// Verifies that direct-input mode self-cancels and requests redraw when the target terminal becomes
/// disconnected.
#[test]
fn direct_input_mode_closes_when_terminal_becomes_disconnected() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<crate::terminals::TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    assert!(input_rx.try_recv().is_err());
    assert_eq!(snapshot_test_hud_state(&world).direct_input_terminal, None);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

