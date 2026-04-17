//! Test submodule: `dialog_task` — extracted from the centralized test bucket.

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

/// Verifies that plain `t` opens the task dialog and seeds it from persisted note text for the
/// active terminal.
#[test]
fn plain_t_opens_task_dialog_for_active_terminal_with_saved_text() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world
        .resource_mut::<AgentTaskStore>()
        .set_text(agent_id, "- [ ] first task\n  detail");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyT, Some("t")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);

    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.task_dialog.visible);
    assert_eq!(hud_state.task_dialog.target_terminal, Some(terminal_id));
    assert_eq!(hud_state.task_dialog.text, "- [ ] first task\n  detail");
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


#[test]
fn task_dialog_route_suppresses_primary_pause_shortcut_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "");
    insert_test_hud_state(&mut world, hud_state);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyP, Some("p")));

    assert_eq!(snapshot_test_hud_state(&world).task_dialog.text, "p");
    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
}


/// Verifies that `Ctrl+T` outside the task dialog enqueues the clear-done-tasks intent for the
/// active terminal.
#[test]
fn ctrl_t_clears_done_tasks_for_active_terminal_when_dialog_is_closed() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyT, Some("t")));

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Task(AppTaskCommand::ClearDone { agent_id })]
    );
}


/// Verifies that `Ctrl+U` cuts the entire task-dialog contents into the kill ring.
#[test]
fn task_dialog_ctrl_u_cuts_all_contents() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [ ] first\n- [ ] second");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyU, Key::Character("u".into())),
    );
    assert_eq!(snapshot_test_hud_state(&world).task_dialog.text, "");

    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyY, Key::Character("y".into())),
    );
    assert_eq!(
        snapshot_test_hud_state(&world).task_dialog.text,
        "- [ ] first\n- [ ] second"
    );
}


/// Verifies that `Ctrl+T` stays live inside the task dialog and emits a clear-done request without
/// closing the dialog.
#[test]
fn task_dialog_ctrl_t_emits_clear_done_request() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [x] done\n  detail\n- [ ] keep");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyT, Some("t")));

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Task(AppTaskCommand::ClearDone { agent_id })]
    );
    assert_eq!(
        snapshot_test_hud_state(&world).task_dialog.text,
        "- [x] done\n  detail\n- [ ] keep"
    );
    assert!(snapshot_test_hud_state(&world).task_dialog.visible);
}


/// Verifies that reopening a task dialog reseeds from persisted text and does not reuse transient
/// unsaved editor state from the previous open.
#[test]
fn reopening_task_dialog_uses_persisted_text_not_stale_editor_state() {
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = crate::hud::HudState::default();

    hud_state.open_task_dialog(terminal_id, "persisted one");
    hud_state.task_dialog.insert_text("\nunsaved");
    hud_state.close_task_dialog();

    hud_state.open_task_dialog(terminal_id, "persisted two");
    assert!(hud_state.task_dialog.visible);
    assert_eq!(hud_state.task_dialog.text, "persisted two");
}


/// Verifies that pressing `Escape` in the task dialog persists the edited text via
/// `SetTerminalTaskText` and then closes the modal.
#[test]
fn task_dialog_escape_persists_tasks_and_closes() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [ ] old");
    hud_state.task_dialog.insert_text("\n- [ ] new");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Escape, Key::Escape));

    assert_eq!(
        world.resource::<AgentTaskStore>().text(agent_id),
        Some("- [ ] old\n- [ ] new")
    );
    assert_eq!(
        world.resource::<AgentTaskStore>().text(agent_id),
        Some("- [ ] old\n- [ ] new")
    );
    let hud_state = snapshot_test_hud_state(&world);
    assert!(!hud_state.task_dialog.visible);
}


#[test]
fn middle_click_paste_in_task_dialog_inserts_clipboard_text() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [ ] keep");
    insert_test_hud_state(&mut world, hud_state);

    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(&world)
        .expect("primary window should exist")
        .clone();
    let rect = task_dialog_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_task_dialog(
        &mut app_session,
        &window,
        Vec2::new(rect.x + 12.0, rect.y + 24.0),
        "
- [ ] pasted",
    ));
    assert_eq!(
        app_session.composer.task_editor.text,
        "- [ ] keep
- [ ] pasted"
    );
}


/// Verifies that `Tab` in the task dialog cycles focus from the editor into the clear-done button.
#[test]
fn task_dialog_tab_cycles_focus_to_action_button() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [ ] keep");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));

    assert_eq!(
        world
            .resource::<AppSessionState>()
            .composer
            .task_dialog_focus,
        TaskDialogFocus::ClearDoneButton
    );
}


/// Verifies that pressing `Enter` on the focused task-dialog button triggers clear-done.
#[test]
fn task_dialog_enter_on_focused_button_emits_clear_done() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [x] done");
    insert_test_hud_state(&mut world, hud_state);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<AppSessionState>()
        .composer
        .task_dialog_focus = TaskDialogFocus::ClearDoneButton;

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Task(AppTaskCommand::ClearDone { agent_id })]
    );
}

