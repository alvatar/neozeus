//! Test submodule: `widgets` — extracted from the centralized test bucket.

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

/// Verifies that widget toggles snap visibility immediately instead of fading over later animation
/// ticks.
#[test]
fn toggling_widgets_snaps_alpha_immediately() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    hud_state.insert_default_module(HudWidgetKey::ConversationList);
    let conversation_rect = HudRect {
        x: 332.0,
        y: 112.0,
        w: 320.0,
        h: 320.0,
    };
    hud_state.set_module_shell_state(
        HudWidgetKey::ConversationList,
        true,
        conversation_rect,
        conversation_rect,
        1.0,
        1.0,
    );
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::AgentList,
        )));
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::ConversationList,
        )));
    run_app_commands(&mut world);

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(
        hud_state.module_enabled(HudWidgetKey::AgentList),
        Some(false)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::AgentList),
        Some(0.0)
    );
    assert_eq!(
        hud_state.module_enabled(HudWidgetKey::ConversationList),
        Some(false)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::ConversationList),
        Some(0.0)
    );

    world.insert_resource(Messages::<AppCommand>::default());
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::AgentList,
        )));
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::ConversationList,
        )));
    run_app_commands(&mut world);

    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(
        hud_state.module_enabled(HudWidgetKey::AgentList),
        Some(true)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::AgentList),
        Some(1.0)
    );
    assert_eq!(
        hud_state.module_enabled(HudWidgetKey::ConversationList),
        Some(true)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::ConversationList),
        Some(1.0)
    );
}


/// Verifies that resetting a HUD module restores the baked-in default shell state instead of merely
/// toggling enablement.
#[test]
fn reset_module_restores_default_toolbar_state() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut hud_state = HudState::default();
    let rect = HudRect {
        x: 1800.0,
        y: 1200.0,
        w: 10.0,
        h: 10.0,
    };
    hud_state.insert_default_module(HudWidgetKey::InfoBar);
    hud_state.set_module_shell_state(HudWidgetKey::InfoBar, false, rect, rect, 0.0, 0.0);

    hud_state.reset_module(HudWidgetKey::InfoBar);

    assert_eq!(hud_state.module_enabled(HudWidgetKey::InfoBar), Some(true));
    assert_eq!(
        hud_state.module_target_rect(HudWidgetKey::InfoBar),
        Some(crate::hud::HUD_MODULE_DEFINITIONS[0].default_rect)
    );
    assert_eq!(
        hud_state.module_current_alpha(HudWidgetKey::InfoBar),
        Some(1.0)
    );
    assert!(hud_state.dirty_layout);
}


/// Verifies that a plain digit key emits the expected module-toggle intent.
#[test]
fn plain_digit_module_shortcut_toggles_module() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(TerminalManager::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit1, Some("1")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Widget(WidgetCommand::Toggle(
            HudWidgetKey::AgentList
        ))]
    );
}


/// Verifies that the authoritative app-command path updates focus/visibility and requests redraws.
#[test]
fn focus_and_visibility_requests_request_redraw_immediately() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id)
        .expect("agent should be linked");
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Inspect(agent_id)));
    run_app_commands(&mut world);

    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(id)
    );
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    let redraws_after_focus = world.resource::<Messages<RequestRedraw>>().len();
    assert!(redraws_after_focus >= 1);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::ClearFocus));
    run_app_commands(&mut world);

    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::ShowAll
    );
    assert!(world.resource::<Messages<RequestRedraw>>().len() > redraws_after_focus);
}


/// Verifies that `Alt+Shift+digit` still emits reset intents rather than toggle intents.
#[test]
fn alt_shift_module_shortcut_still_resets_module() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::AltLeft);
    keys.press(KeyCode::ShiftLeft);
    world.insert_resource(keys);
    world.insert_resource(TerminalManager::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit0, Some("0")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Widget(WidgetCommand::Reset(
            HudWidgetKey::InfoBar
        ))]
    );
}


/// Verifies that HUD module shortcuts are ignored while direct terminal input has keyboard capture.
#[test]
fn module_shortcuts_are_suppressed_while_direct_input_is_open() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.open_direct_terminal_input(crate::terminals::TerminalId(1));
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit1, Some("1")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
}


/// Verifies that HUD hit-testing returns the frontmost enabled module when rects overlap.
#[test]
fn hud_state_topmost_enabled_at_prefers_frontmost_module() {
    let mut state = HudState::default();
    state.insert_default_module(HudWidgetKey::InfoBar);
    state.insert_default_module(HudWidgetKey::AgentList);
    state.raise_to_front(HudWidgetKey::AgentList);

    assert_eq!(
        state.topmost_enabled_at(Vec2::new(40.0, 110.0)),
        Some(HudWidgetKey::AgentList)
    );
}

