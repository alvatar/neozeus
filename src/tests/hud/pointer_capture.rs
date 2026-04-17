//! Test submodule: `pointer_capture` — extracted from the centralized test bucket.

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

/// Verifies that create-agent dialog pointer clicks persist field-focus and completion cleanup even
/// though the modal handler exits early after handling the click.
#[test]
fn create_agent_dialog_pointer_click_persists_focus_cleanup_and_redraw() {
    let mut world = World::default();
    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let name_rect = create_agent_name_field_rect(&window);
    window.set_cursor_position(Some(Vec2::new(name_rect.x + 4.0, name_rect.y + 4.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    init_hud_commands(&mut world);
    insert_default_hud_resources(&mut world);
    world.spawn((window, PrimaryWindow));

    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(AppCreateAgentKind::Pi);
        session.create_agent_dialog.focus = CreateAgentDialogField::Kind;
        session.create_agent_dialog.error = Some("stale error".into());
        session.create_agent_dialog.cwd_field.field.load_text("s");
        assert!(session
            .create_agent_dialog
            .cwd_field
            .start_or_cycle_completion(false));
    }

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let session = world.resource::<AppSessionState>();
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Name
    );
    assert_eq!(session.create_agent_dialog.error, None);
    assert!(session.create_agent_dialog.cwd_field.completion.is_none());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


/// Verifies that the direct-input capture branch still persists layout drag cleanup even though it
/// returns before general HUD interaction runs.
#[test]
fn direct_input_pointer_capture_persists_drag_cleanup() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::InfoBar);
    hud_state.drag = Some(HudDragState {
        module_id: HudWidgetKey::InfoBar,
        grab_offset: Vec2::new(7.0, 9.0),
    });
    hud_state.open_direct_terminal_input(crate::terminals::TerminalId(3));

    let window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    init_hud_commands(&mut world);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    world.spawn((window, PrimaryWindow));

    world.run_system_once(handle_hud_pointer_input).unwrap();

    assert!(snapshot_test_hud_state(&world).drag.is_none());
}


/// Verifies that a mouse release with no cursor still clears the transient agent-list drag state
/// before the pointer handler exits.
#[test]
fn releasing_pointer_without_cursor_clears_agent_drag_state() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    hud_state.drag = Some(HudDragState {
        module_id: HudWidgetKey::AgentList,
        grab_offset: Vec2::new(3.0, 4.0),
    });

    let window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    init_hud_commands(&mut world);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    world.spawn((window, PrimaryWindow));

    world.insert_resource(AgentListUiState {
        scroll_offset: 0.0,
        hovered_row: None,
        show_selected_context: false,
        drag: AgentListDragState {
            pressed_row: None,
            pressed_agent: Some(crate::agents::AgentId(11)),
            press_origin: Some(Vec2::new(10.0, 12.0)),
            dragging_agent: Some(crate::agents::AgentId(11)),
            drag_cursor: Some(Vec2::new(15.0, 20.0)),
            drag_grab_offset_y: 6.0,
            last_reorder_index: Some(2),
        },
    });
    {
        let buttons = &mut world.resource_mut::<ButtonInput<MouseButton>>();
        buttons.press(MouseButton::Left);
        buttons.release(MouseButton::Left);
    }

    world.run_system_once(handle_hud_pointer_input).unwrap();

    let hud_state = snapshot_test_hud_state(&world);
    let agent_list_state = world.resource::<AgentListUiState>();
    assert!(hud_state.drag.is_none());
    assert_eq!(agent_list_state.drag.pressed_agent, None);
    assert_eq!(agent_list_state.drag.press_origin, None);
    assert_eq!(agent_list_state.drag.dragging_agent, None);
    assert_eq!(agent_list_state.drag.drag_cursor, None);
    assert_eq!(agent_list_state.drag.drag_grab_offset_y, 0.0);
    assert_eq!(agent_list_state.drag.last_reorder_index, None);
}

