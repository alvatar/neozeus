//! Test submodule: `terminal_pointer` — extracted from the centralized test bucket.

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

/// Verifies that clicking empty background clears focus, restores `ShowAll`, resets view offset, and
/// clears focus/visibility without mutating recoverable snapshot persistence.
#[test]
fn background_click_hides_active_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), true, Vec2::ZERO);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        None
    );
    let manager = world.resource::<TerminalManager>();
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        crate::hud::TerminalVisibilityPolicy::ShowAll
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalViewState>()
            .offset,
        Vec2::ZERO
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_none());
    assert!(manager.get(terminal_id).is_some());
}


/// Verifies that clicks landing inside the visible active terminal panel do not trigger the
/// background-hide path.
#[test]
fn clicking_visible_terminal_does_not_hide_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(640.0, 360.0), true, Vec2::ZERO);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();

    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(terminal_id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_none());
}


/// Verifies that background-hit testing still respects terminal panel translation offsets.
#[test]
fn clicking_shifted_visible_terminal_does_not_hide_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let panel_position = Vec2::new(180.0, 120.0);
    let panel_center = Vec2::new(640.0 + panel_position.x, 360.0 - panel_position.y);
    let (mut world, terminal_id) = world_with_active_terminal(panel_center, true, panel_position);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();

    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(terminal_id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_none());
}


/// Verifies that dragging across a live terminal routes selection ownership into the terminal
/// backend instead of mirroring a parallel app-side screen-cell selection.
#[test]
fn dragging_over_terminal_panel_sends_live_terminal_selection_command() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(545.0, 305.0), true, Vec2::ZERO);
    let mut surface = crate::terminals::TerminalSurface::new(4, 2);
    surface.set_text_cell(0, 0, "A");
    surface.set_text_cell(1, 0, "B");
    surface.set_text_cell(2, 0, "C");
    world
        .resource_mut::<TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some(surface);

    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world
        .run_system_once(handle_terminal_text_selection)
        .unwrap();
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear_just_pressed(MouseButton::Left);

    world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .single_mut(&mut world)
        .expect("window should exist")
        .set_cursor_position(Some(Vec2::new(645.0, 305.0)));
    world
        .run_system_once(handle_terminal_text_selection)
        .unwrap();

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::SetSelection {
            anchor: crate::terminals::TerminalViewportPoint { col: 0, row: 0 },
            focus: crate::terminals::TerminalViewportPoint { col: 2, row: 0 },
        }
    );
    assert_eq!(
        world
            .resource::<crate::text_selection::TerminalTextSelectionState>()
            .owner(),
        Some(crate::text_selection::TerminalTextSelectionOwner::LiveTerminal(terminal_id))
    );
}


/// Verifies that panel clicks choose the highest-`z` visible terminal panel and emit focus+isolate
/// intents for it.
#[test]
fn clicking_terminal_panel_enqueues_focus_and_isolate_for_topmost_visible_panel() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.set_cursor_position(Some(Vec2::new(640.0, 360.0)));

    let mut catalog = AgentCatalog::default();
    let first_agent = catalog.create_agent(
        Some("agent-1".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentCapabilities::terminal_defaults(),
    );
    let second_agent = catalog.create_agent(
        Some("agent-2".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentCapabilities::terminal_defaults(),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(
        first_agent,
        crate::terminals::TerminalId(1),
        "session-1".into(),
        None,
    );
    runtime_index.link_terminal(
        second_agent,
        crate::terminals::TerminalId(2),
        "session-2".into(),
        None,
    );

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel {
            id: crate::terminals::TerminalId(1),
        },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::new(220.0, 140.0),
            target_size: Vec2::new(220.0, 140.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: -0.1,
            target_z: -0.1,
        },
        Visibility::Visible,
    ));
    world.spawn((
        TerminalPanel {
            id: crate::terminals::TerminalId(2),
        },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::new(220.0, 140.0),
            target_size: Vec2::new(220.0, 140.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.3,
            target_z: 0.3,
        },
        Visibility::Visible,
    ));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world
        .run_system_once(focus_terminal_on_panel_click)
        .unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Agent(AppAgentCommand::Inspect(second_agent))]
    );
}


/// Verifies that background-hide logic is suppressed when the click lands on HUD chrome instead of
/// empty background.
#[test]
fn clicking_hud_does_not_hide_active_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    let rect = crate::hud::HudRect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };
    hud_state.insert_default_module(crate::hud::HudWidgetKey::InfoBar);
    hud_state.set_module_shell_state(
        crate::hud::HudWidgetKey::InfoBar,
        true,
        rect,
        rect,
        1.0,
        1.0,
    );
    insert_test_hud_state(&mut world, hud_state);
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    world
        .run_system_once(hide_terminal_on_background_click)
        .unwrap();

    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(terminal_id)
    );
}
