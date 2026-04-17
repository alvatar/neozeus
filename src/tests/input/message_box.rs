//! Test submodule: `message_box` — extracted from the centralized test bucket.

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

/// Verifies that plain `Enter` opens the message box for the active terminal when no other capture
/// mode is active.
#[test]
fn enter_opens_message_box_for_active_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Enter, None));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);

    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.message_box.visible);
    assert_eq!(hud_state.message_box.target_terminal, Some(terminal_id));
    assert!(hud_state.message_box.text.is_empty());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


#[test]
fn message_dialog_route_suppresses_primary_pause_shortcut_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    insert_test_hud_state(&mut world, hud_state);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyP, Some("p")));

    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "p");
    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
}


/// Verifies that direct-input mode forwards key events to the terminal bridge instead of opening the
/// message box.
#[test]
fn direct_input_mode_sends_keys_to_terminal_without_opening_message_box() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));
    dispatch_terminal_ui_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::InputText("a".into())
    );
    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::InputEvent("\r".into())
    );
    assert!(!snapshot_test_hud_state(&world).message_box.visible);
}


#[test]
fn message_box_typing_burst_stays_close_to_noop_baseline() {
    let (mut baseline_world, _baseline_terminal) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    init_hud_commands(&mut baseline_world);
    baseline_world.init_resource::<Messages<RequestRedraw>>();

    let baseline_started = Instant::now();
    for _ in 0..MESSAGE_BOX_TYPING_BURST_KEYS {
        dispatch_message_box_key(&mut baseline_world, pressed_text(KeyCode::KeyA, Some("a")));
    }
    let baseline_elapsed = baseline_started.elapsed();

    let (mut hot_world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    insert_test_hud_state(&mut hot_world, hud_state);
    init_hud_commands(&mut hot_world);
    hot_world.init_resource::<Messages<RequestRedraw>>();

    let hot_started = Instant::now();
    for _ in 0..MESSAGE_BOX_TYPING_BURST_KEYS {
        dispatch_message_box_key(&mut hot_world, pressed_text(KeyCode::KeyA, Some("a")));
    }
    let hot_elapsed = hot_started.elapsed();
    let baseline_nanos = baseline_elapsed.as_nanos().max(1) as f64;
    let overhead_ratio = hot_elapsed.as_nanos() as f64 / baseline_nanos;

    assert_eq!(
        hot_world
            .resource::<AppSessionState>()
            .composer
            .message_editor
            .text
            .len(),
        MESSAGE_BOX_TYPING_BURST_KEYS,
        "message editor should contain the full typing burst"
    );
    assert!(
        overhead_ratio <= MESSAGE_BOX_TYPING_OVERHEAD_RATIO_MAX,
        "message-box typing burst regressed: noop baseline={}µs hot-path={}µs ratio={:.2} max={:.2}",
        baseline_elapsed.as_micros(),
        hot_elapsed.as_micros(),
        overhead_ratio,
        MESSAGE_BOX_TYPING_OVERHEAD_RATIO_MAX
    );
}


/// Verifies that closing the message box preserves its draft per terminal and restores it on reopen.
#[test]
fn closing_message_box_preserves_draft_for_reopen() {
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = crate::hud::HudState::default();

    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("draft payload");
    hud_state.close_message_box();

    assert!(!hud_state.message_box.visible);
    assert_eq!(hud_state.message_box.target_terminal, None);

    hud_state.open_message_box(terminal_id);
    assert!(hud_state.message_box.visible);
    assert_eq!(hud_state.message_box.target_terminal, Some(terminal_id));
    assert_eq!(hud_state.message_box.text, "draft payload");
}


/// Verifies that message-box drafts are tracked independently per target terminal.
#[test]
fn message_box_keeps_separate_drafts_per_terminal() {
    let terminal_one = crate::terminals::TerminalId(7);
    let terminal_two = crate::terminals::TerminalId(9);
    let mut hud_state = crate::hud::HudState::default();

    hud_state.open_message_box(terminal_one);
    hud_state.message_box.insert_text("first draft");
    hud_state.close_message_box();

    hud_state.open_message_box(terminal_two);
    hud_state.message_box.insert_text("second draft");
    hud_state.close_message_box();

    hud_state.open_message_box(terminal_one);
    assert_eq!(hud_state.message_box.text, "first draft");

    hud_state.open_message_box(terminal_two);
    assert_eq!(hud_state.message_box.text, "second draft");
}


/// Verifies the core message-box editor/send flow: multiline typing, `Ctrl+S` send, modal close,
/// and clean reopen.
#[test]
fn message_box_supports_multiline_typing_and_ctrl_s_send() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    let (mut world, terminal_id, _input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyB, Some("b")));

    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "a\nb");

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyS, Some("s")));

    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .unwrap();
    let session_name = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .session_name(agent_id)
        .unwrap()
        .to_owned();
    assert_eq!(
        client.sent_commands.lock().unwrap().as_slice(),
        &[(session_name, TerminalCommand::SendCommand("a\nb".into()))]
    );
    {
        let hud_state = snapshot_test_hud_state(&world);
        assert!(!hud_state.message_box.visible);
        assert!(hud_state.message_box.text.is_empty());
    }

    world.insert_resource(ButtonInput::<KeyCode>::default());
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Enter, None));
    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.message_box.visible);
    assert!(hud_state.message_box.text.is_empty());
}


#[test]
fn middle_click_paste_in_message_box_inserts_clipboard_text() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    insert_test_hud_state(&mut world, hud_state);

    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(&world)
        .expect("primary window should exist")
        .clone();
    let rect = message_box_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_message_dialog(
        &mut app_session,
        &window,
        Vec2::new(rect.x + 12.0, rect.y + 24.0),
        "hello
world",
    ));
    assert_eq!(
        app_session.composer.message_editor.text,
        "hello
world"
    );
}


/// Verifies that `Tab` in the message box cycles focus from the editor into the action buttons.
#[test]
fn message_box_tab_cycles_focus_to_action_buttons() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .composer
            .message_dialog_focus,
        MessageDialogFocus::AppendButton
    );

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .composer
            .message_dialog_focus,
        MessageDialogFocus::PrependButton
    );
}


/// Verifies that pressing `Enter` on a focused message-box action button triggers that action.
#[test]
fn message_box_enter_on_focused_button_emits_action_command() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("follow up");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<AppSessionState>()
        .composer
        .message_dialog_focus = MessageDialogFocus::AppendButton;

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Task(AppTaskCommand::Append {
            agent_id,
            text: "follow up".into(),
        })]
    );
}


/// Verifies that `Ctrl+T` inside the message box is treated as editor input/no-op rather than as the
/// global clear-done task shortcut.
#[test]
fn message_box_ctrl_t_does_not_enqueue_task_shortcuts() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("follow up\n  details");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyT, Some("t")));

    assert!(drain_hud_commands(&mut world).is_empty());
    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.message_box.visible);
    assert_eq!(hud_state.message_box.text, "follow up\n  details");
}


/// Verifies a representative set of control-key editor bindings over multiline message-box text:
/// line motion, kill/yank, and vertical movement.
#[test]
fn message_box_ctrl_bindings_edit_multiline_text() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("alpha\nbeta");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyA, Key::Character("a".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world)
            .message_box
            .cursor_line_and_column(),
        (1, 0)
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyK, Key::Character("k".into())),
    );
    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "alpha\n");

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world).message_box.text,
        "alpha\nbeta"
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyP, Key::Character("p".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world)
            .message_box
            .cursor_line_and_column(),
        (0, 4)
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyE, Key::Character("e".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world)
            .message_box
            .cursor_line_and_column(),
        (0, 5)
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyN, Key::Character("n".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world)
            .message_box
            .cursor_line_and_column(),
        (1, 4)
    );
}


/// Verifies region mark/kill/yank behavior in the message-box editor, including region growth via
/// word motion.
#[test]
fn message_box_mark_region_ctrl_w_and_ctrl_y_work() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("alpha beta gamma");
    hud_state.message_box.cursor = 6;
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    assert_eq!(snapshot_test_hud_state(&world).message_box.mark, Some(6));

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    world.insert_resource(alt_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyF, Key::Character("f".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world).message_box.region_bounds(),
        Some((6, 10))
    );

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyW, Key::Character("w".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world).message_box.text,
        "alpha  gamma"
    );
    assert_eq!(snapshot_test_hud_state(&world).message_box.mark, None);

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world).message_box.text,
        "alpha beta gamma"
    );
}


/// Verifies the Alt-bound editor operations: copy-region, kill-ring rotation, and backward kill-word
/// behavior.
#[test]
fn message_box_meta_copy_kill_ring_history_and_backward_kill_word_work() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("one two three");
    hud_state.message_box.cursor = 4;
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    world.insert_resource(alt_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyF, Key::Character("f".into())),
    );
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyW, Key::Character("w".into())),
    );

    world
        .resource_mut::<crate::app::AppSessionState>()
        .composer
        .message_editor
        .cursor = 8;
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyD, Key::Character("d".into())),
    );
    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "one two ");

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world).message_box.text,
        "one two three"
    );

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    world.insert_resource(alt_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world).message_box.text,
        "one two two"
    );

    world
        .resource_mut::<crate::app::AppSessionState>()
        .composer
        .message_editor
        .cursor = world
        .resource::<crate::app::AppSessionState>()
        .composer
        .message_editor
        .text
        .len();
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Backspace, None));
    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "one two ");
}


/// Verifies that `Ctrl+U` cuts the entire message-box contents into the kill ring.
#[test]
fn message_box_ctrl_u_cuts_all_contents() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("alpha\nbeta");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyU, Key::Character("u".into())),
    );
    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "");

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world).message_box.text,
        "alpha\nbeta"
    );
}


/// Verifies the editor's `Ctrl+O` open-line and `Ctrl+J` newline-and-indent behaviors.
#[test]
fn message_box_ctrl_o_and_ctrl_j_work() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("ab");
    hud_state.message_box.cursor = 1;
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyO, Key::Character("o".into())),
    );
    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "a\nb");
    assert_eq!(snapshot_test_hud_state(&world).message_box.cursor, 1);

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyJ, Key::Character("j".into())),
    );
    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "a\n\nb");
    assert_eq!(snapshot_test_hud_state(&world).message_box.cursor, 2);
}


/// Verifies the combination of Alt word-motion commands and `Ctrl+D` forward-delete in the
/// message-box editor.
#[test]
fn message_box_alt_word_motion_and_ctrl_d_work() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("one two");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    world.insert_resource(alt_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyB, Key::Character("b".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world)
            .message_box
            .cursor_line_and_column(),
        (0, 4)
    );

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyF, Key::Character("f".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world)
            .message_box
            .cursor_line_and_column(),
        (0, 7)
    );

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyB, Key::Character("b".into())),
    );
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyD, Key::Character("d".into())),
    );
    assert_eq!(snapshot_test_hud_state(&world).message_box.text, "one tw");
}

