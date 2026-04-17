//! Test submodule: `message_box` — extracted from the centralized test bucket.

#![allow(unused_imports)]

use super::super::{
    ensure_shared_app_command_test_resources, fake_runtime_spawner, init_git_repo,
    insert_default_hud_resources, insert_terminal_manager_resources, insert_test_hud_state,
    pressed_text, snapshot_test_hud_state, temp_dir, test_bridge, write_pi_session_file,
    FakeDaemonClient,
};
use crate::agents::{AgentCatalog, AgentRuntimeIndex};
use crate::terminals::{
    kill_active_terminal_session_and_remove as kill_active_terminal, TerminalFontState,
    TerminalGlyphCache, TerminalManager, TerminalNotesState, TerminalPanel, TerminalPanelFrame,
    TerminalPresentationStore, TerminalTextRenderer, TerminalViewState,
};
use crate::{
    app::{
        AgentCommand as AppAgentCommand, AppCommand, AppSessionState, AppStatePersistenceState,
        ComposerCommand as AppComposerCommand, CreateAgentDialogField,
        CreateAgentKind as AppCreateAgentKind, TaskCommand as AppTaskCommand, WidgetCommand,
    },
    app_config::DEFAULT_BG,
    composer::{
        clone_agent_name_field_rect, clone_agent_submit_button_rect, clone_agent_workdir_rect,
        create_agent_name_field_rect, message_box_action_buttons, message_box_rect,
        message_box_shortcut_button_rects, task_dialog_action_buttons,
    },
    hud::{
        handle_hud_module_shortcuts, handle_hud_pointer_input, AgentListDragState,
        AgentListUiState, AgentListView, HudDragState, HudRect, HudState, HudWidgetKey,
        TerminalVisibilityPolicy, TerminalVisibilityState,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    image::Image,
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::MouseWheel,
        ButtonState,
    },
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};


use super::support::*;

/// Verifies the fixed proportional layout of the message-box modal within the window.
#[test]
fn message_box_rect_is_top_aligned_and_shorter() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };

    let rect = message_box_rect(&window);
    assert!((rect.w - 980.0).abs() < 0.01);
    assert!((rect.h - 342.0).abs() < 0.01);
    assert!((rect.x - 210.0).abs() < 0.01);
    assert!((rect.y - 8.0).abs() < 0.01);
}


/// Verifies that clicking the message-box append-task button turns the current draft into an
/// `AppendTerminalTask` intent and closes the modal.
#[test]
fn clicking_message_box_task_button_emits_append_task_intent() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("follow up");

    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let (_, append_rect, _) = message_box_action_buttons(&window)[0];
    window.set_cursor_position(Some(Vec2::new(append_rect.x + 4.0, append_rect.y + 4.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let emitted = world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(
        emitted,
        vec![AppCommand::Task(AppTaskCommand::Append {
            agent_id: crate::agents::AgentId(1),
            text: "follow up".into(),
        })]
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    assert!(!snapshot_test_hud_state(&world).message_box.visible);
}


/// Verifies that clicking a message-box shortcut button replaces the message draft with the
/// configured preset text.
#[test]
fn clicking_message_box_shortcut_button_populates_draft_from_config() {
    let mut world = World::default();
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("old text");

    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let shortcut_rect = message_box_shortcut_button_rects(&window)[0];
    window.set_cursor_position(Some(Vec2::new(shortcut_rect.x + 4.0, shortcut_rect.y + 4.0)));

    let config = crate::app_config::NeoZeusConfig::default();
    let expected_text = config.message_box_shortcuts()[0].text.clone();

    world.insert_resource(config);
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.message_box.text, expected_text);
    assert!(hud_state.message_box.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

