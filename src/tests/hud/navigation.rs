//! Test submodule: `navigation` — extracted from the centralized test bucket.

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

/// Verifies the plain `j` agent-list navigation shortcut emits focus+isolate for the next terminal.
#[test]
fn plain_j_navigates_to_next_agent_and_isolates_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    insert_terminal_manager_resources(&mut world, manager);
    let next_agent = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_two)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyJ, Some("j")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(next_agent)),
            AppCommand::Agent(AppAgentCommand::Inspect(next_agent)),
        ]
    );
}


#[test]
fn plain_p_is_ignored_by_hud_shortcuts_to_avoid_double_toggle() {
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let _id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    insert_terminal_manager_resources(&mut world, manager);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_one)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyP, Some("p")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();
    run_app_commands(&mut world);
    assert!(!world
        .resource::<crate::agents::AgentCatalog>()
        .is_paused(agent_id));
}


#[test]
fn shift_p_is_ignored_by_hud_shortcuts() {
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let _id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    insert_terminal_manager_resources(&mut world, manager);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_one)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    insert_default_hud_resources(&mut world);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyP, Key::Character("P".into())));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();
    run_app_commands(&mut world);
    assert!(!world
        .resource::<crate::agents::AgentCatalog>()
        .is_paused(agent_id));
}


/// Verifies that the down-arrow shortcut uses the same next-agent focus+isolate behavior as `j`.
#[test]
fn down_arrow_navigates_to_next_agent_and_isolates_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    insert_terminal_manager_resources(&mut world, manager);
    let next_agent = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_two)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::ArrowDown, None));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(next_agent)),
            AppCommand::Agent(AppAgentCommand::Inspect(next_agent)),
        ]
    );
}


#[test]
fn hud_navigation_jk_across_agents_and_tmux_keeps_exactly_one_selected_row_after_each_step() {
    let (mut world, agent_one, agent_two) = build_agent_list_navigation_world();
    assert_exactly_one_selected_row(&world, crate::hud::AgentListRowKey::Agent(agent_one));

    dispatch_agent_list_nav_step(
        &mut world,
        pressed_key(KeyCode::KeyJ, Key::Character("j".into())),
    );
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
    assert_exactly_one_selected_row(
        &world,
        crate::hud::AgentListRowKey::OwnedTmux("tmux-session-1".into()),
    );

    dispatch_agent_list_nav_step(
        &mut world,
        pressed_key(KeyCode::KeyJ, Key::Character("j".into())),
    );
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(agent_two)
    );
    assert_exactly_one_selected_row(&world, crate::hud::AgentListRowKey::Agent(agent_two));

    dispatch_agent_list_nav_step(
        &mut world,
        pressed_key(KeyCode::KeyK, Key::Character("k".into())),
    );
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
    assert_exactly_one_selected_row(
        &world,
        crate::hud::AgentListRowKey::OwnedTmux("tmux-session-1".into()),
    );

    dispatch_agent_list_nav_step(
        &mut world,
        pressed_key(KeyCode::KeyK, Key::Character("k".into())),
    );
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(agent_one)
    );
    assert_exactly_one_selected_row(&world, crate::hud::AgentListRowKey::Agent(agent_one));
}


#[test]
fn hud_navigation_arrow_keys_across_agents_and_tmux_keeps_exactly_one_selected_row_after_each_step()
{
    let (mut world, agent_one, agent_two) = build_agent_list_navigation_world();
    assert_exactly_one_selected_row(&world, crate::hud::AgentListRowKey::Agent(agent_one));

    dispatch_agent_list_nav_step(&mut world, pressed_key(KeyCode::ArrowDown, Key::ArrowDown));
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
    assert_exactly_one_selected_row(
        &world,
        crate::hud::AgentListRowKey::OwnedTmux("tmux-session-1".into()),
    );

    dispatch_agent_list_nav_step(&mut world, pressed_key(KeyCode::ArrowDown, Key::ArrowDown));
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(agent_two)
    );
    assert_exactly_one_selected_row(&world, crate::hud::AgentListRowKey::Agent(agent_two));

    dispatch_agent_list_nav_step(&mut world, pressed_key(KeyCode::ArrowUp, Key::ArrowUp));
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
    assert_exactly_one_selected_row(
        &world,
        crate::hud::AgentListRowKey::OwnedTmux("tmux-session-1".into()),
    );

    dispatch_agent_list_nav_step(&mut world, pressed_key(KeyCode::ArrowUp, Key::ArrowUp));
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(agent_one)
    );
    assert_exactly_one_selected_row(&world, crate::hud::AgentListRowKey::Agent(agent_one));
}


/// Verifies the plain `k` agent-list navigation shortcut emits focus+isolate for the previous
/// terminal.
#[test]
fn plain_k_navigates_to_previous_agent_and_isolates_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);
    insert_terminal_manager_resources(&mut world, manager);
    let previous_agent = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_one)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(previous_agent)),
            AppCommand::Agent(AppAgentCommand::Inspect(previous_agent)),
        ]
    );
}


/// Verifies that the up-arrow shortcut uses the same previous-agent focus+isolate behavior as `k`.
#[test]
fn up_arrow_navigates_to_previous_agent_and_isolates_it() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);
    insert_terminal_manager_resources(&mut world, manager);
    let previous_agent = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id_one)
        .expect("agent should be linked");
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::ArrowUp, None));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(previous_agent)),
            AppCommand::Agent(AppAgentCommand::Inspect(previous_agent)),
        ]
    );
}

