//! Test submodule: `task_dialog` — extracted from the centralized test bucket.

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

/// Verifies that clicking the task-dialog `Clear done` button emits the clear-done intent but leaves
/// the dialog/editor state open for the subsequent persistence update.
#[test]
fn clicking_task_dialog_clear_done_button_persists_updated_text() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [x] done\n- [ ] keep");

    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let (_, clear_done_rect, _) = task_dialog_action_buttons(&window)[0];
    window.set_cursor_position(Some(Vec2::new(
        clear_done_rect.x + 4.0,
        clear_done_rect.y + 4.0,
    )));

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
        vec![AppCommand::Task(AppTaskCommand::ClearDone {
            agent_id: crate::agents::AgentId(1),
        })]
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.task_dialog.visible);
    assert_eq!(hud_state.task_dialog.text, "- [x] done\n- [ ] keep");
}


/// Verifies that clearing done tasks through the app-command path refreshes the open task editor
/// from authoritative task state rather than leaving stale local text behind.
#[test]
fn clear_done_task_request_updates_open_dialog_from_persisted_state() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _, _) = super::super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text("session-a", "- [x] done\n- [ ] keep"));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(notes_state);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "stale local");
    insert_test_hud_state(&mut world, hud_state);
    {
        let mut tasks = world.resource_mut::<crate::conversations::AgentTaskStore>();
        let _ = tasks.set_text(agent_id, "- [x] done\n- [ ] keep");
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Task(AppTaskCommand::ClearDone { agent_id }));

    run_app_commands(&mut world);

    let agent_uid = {
        let catalog = world.resource::<AgentCatalog>();
        catalog.uid(agent_id).unwrap().to_owned()
    };
    {
        let notes_state = world.resource::<TerminalNotesState>();
        assert_eq!(
            notes_state.note_text_by_agent_uid(&agent_uid),
            Some("- [ ] keep")
        );
    }
    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.task_dialog.text, "- [ ] keep");
    assert!(hud_state.task_dialog.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}


/// Verifies that submitting an empty task editor clears persisted note state instead of storing an
/// empty note blob.
#[test]
fn set_task_text_request_clears_persisted_task_presence_when_empty() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _, _) = super::super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let agent_uid = world
        .resource::<AgentCatalog>()
        .uid(agent_id)
        .unwrap()
        .to_owned();
    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text_by_agent_uid(&agent_uid, "- [x] done"));
    assert!(notes_state
        .note_text_by_agent_uid(&agent_uid)
        .is_some_and(|text| !text.trim().is_empty()));
    world.insert_resource(notes_state);
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "");
    insert_test_hud_state(&mut world, hud_state);
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Composer(AppComposerCommand::Submit));

    run_app_commands(&mut world);

    let notes_state = world.resource::<TerminalNotesState>();
    assert_eq!(notes_state.note_text_by_agent_uid(&agent_uid), None);
    assert!(notes_state
        .note_text_by_agent_uid(&agent_uid)
        .is_none_or(|text| text.trim().is_empty()));
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    let hud_state = snapshot_test_hud_state(&world);
    assert!(!hud_state.task_dialog.visible);
}


/// Verifies that consuming the next task through the app-command path sends the task payload to
/// the terminal and marks that task done in persisted notes.
#[test]
fn consume_next_task_request_sends_message_and_marks_task_done() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, input_rx, _) = super::super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let agent_uid = world
        .resource::<AgentCatalog>()
        .uid(agent_id)
        .unwrap()
        .to_owned();
    let mut notes_state = TerminalNotesState::default();
    assert!(
        notes_state.set_note_text_by_agent_uid(&agent_uid, "- [ ] first\n  detail\n- [ ] second")
    );
    world.insert_resource(notes_state);
    {
        let mut tasks = world.resource_mut::<crate::conversations::AgentTaskStore>();
        let _ = tasks.set_text(agent_id, "- [ ] first\n  detail\n- [ ] second");
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Task(AppTaskCommand::ConsumeNext { agent_id }));

    run_app_commands(&mut world);

    assert_eq!(
        input_rx.try_recv().unwrap(),
        crate::terminals::TerminalCommand::SendCommand("first\n  detail".into())
    );
    assert_eq!(
        world
            .resource::<TerminalNotesState>()
            .note_text_by_agent_uid(&agent_uid),
        Some("- [x] first\n  detail\n- [ ] second")
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

