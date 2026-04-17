//! Shared test-only helpers for this area.
//!
//! Holds the imports, constants, and builders used by per-topic test submodules.
//! Private items are promoted to `pub(super)` so sibling submodules can reach them.

#![allow(unused_imports, dead_code)]

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

pub(super) const DIRECT_INPUT_TYPING_BURST_KEYS: usize = 512;
pub(super) const MESSAGE_BOX_TYPING_BURST_KEYS: usize = 512;
pub(super) const DIRECT_INPUT_TYPING_OVERHEAD_RATIO_MAX: f64 = 4.0;
pub(super) const MESSAGE_BOX_TYPING_OVERHEAD_RATIO_MAX: f64 = 4.25;


pub(super) fn wheel_lines(y: f32) -> MouseWheel {
    MouseWheel {
        x: 0.0,
        y,
        unit: MouseScrollUnit::Line,
        window: Entity::PLACEHOLDER,
    }
}

pub(super) fn wheel_pixels(y: f32) -> MouseWheel {
    MouseWheel {
        x: 0.0,
        y,
        unit: MouseScrollUnit::Pixel,
        window: Entity::PLACEHOLDER,
    }
}

/// Builds a pressed `KeyboardInput` event without text payload for shortcut-oriented tests.
pub(super) fn pressed_key(key_code: KeyCode, logical_key: Key) -> KeyboardInput {
    KeyboardInput {
        key_code,
        logical_key,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    }
}

/// Initializes the app-command message resource in an input-test world.
pub(super) fn init_hud_commands(world: &mut World) {
    world.init_resource::<Messages<AppCommand>>();
}

/// Drains and collects queued app commands.
pub(super) fn drain_hud_commands(world: &mut World) -> Vec<AppCommand> {
    world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap()
}

/// Ensures app command world resources exists and returns its identifier.
pub(super) fn ensure_app_command_world_resources(world: &mut World) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    ensure_shared_app_command_test_resources(world);
    if !world.contains_resource::<TerminalManager>() {
        world.insert_resource(TerminalManager::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalFocusState>() {
        world.insert_resource(crate::terminals::TerminalFocusState::default());
    }
    if !world.contains_resource::<AgentCatalog>() {
        world.insert_resource(AgentCatalog::default());
    }
    if !world.contains_resource::<AgentRuntimeIndex>() {
        world.insert_resource(AgentRuntimeIndex::default());
    }
    if !world.contains_resource::<AppSessionState>() {
        world.insert_resource(AppSessionState::default());
        world.insert_resource(crate::aegis::AegisPolicyStore::default());
        world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    }
    if !world.contains_resource::<crate::hud::HudLayoutState>() {
        world.insert_resource(crate::hud::HudLayoutState::default());
    }
    if !world.contains_resource::<crate::hud::AgentListUiState>() {
        world.insert_resource(crate::hud::AgentListUiState::default());
    }
    if !world.contains_resource::<crate::hud::ConversationListUiState>() {
        world.insert_resource(crate::hud::ConversationListUiState::default());
    }
    if !world.contains_resource::<crate::hud::InfoBarUiState>() {
        world.insert_resource(crate::hud::InfoBarUiState);
    }
    if !world.contains_resource::<crate::hud::ThreadPaneUiState>() {
        world.insert_resource(crate::hud::ThreadPaneUiState);
    }
    if !world.contains_resource::<crate::hud::HudInputCaptureState>() {
        world.insert_resource(crate::hud::HudInputCaptureState::default());
    }
    if !world.contains_resource::<crate::hud::AgentListSelection>() {
        world.insert_resource(crate::hud::AgentListSelection::default());
    }
    if !world.contains_resource::<crate::hud::AgentListView>() {
        world.insert_resource(crate::hud::AgentListView::default());
    }
    if !world.contains_resource::<crate::hud::ConversationListView>() {
        world.insert_resource(crate::hud::ConversationListView::default());
    }
    if !world.contains_resource::<crate::hud::ThreadView>() {
        world.insert_resource(crate::hud::ThreadView::default());
    }
    if !world.contains_resource::<crate::hud::ComposerView>() {
        world.insert_resource(crate::hud::ComposerView::default());
    }
    if !world.contains_resource::<crate::hud::InfoBarView>() {
        world.insert_resource(crate::hud::InfoBarView::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalPointerState>() {
        world.insert_resource(crate::terminals::TerminalPointerState::default());
    }
    if !world.contains_resource::<Messages<AppCommand>>() {
        world.init_resource::<Messages<AppCommand>>();
    }
    if !world.contains_resource::<Messages<AppExit>>() {
        world.init_resource::<Messages<AppExit>>();
    }
    if !world.contains_resource::<Messages<RequestRedraw>>() {
        world.init_resource::<Messages<RequestRedraw>>();
    }
}

/// Handles run app command cycle.
pub(super) fn run_app_command_cycle(world: &mut World) {
    ensure_app_command_world_resources(world);
    crate::app::run_apply_app_commands(world);
}

/// Injects one keyboard event into the modal-editor keyboard handler under test.
pub(super) fn dispatch_message_box_key(world: &mut World, event: KeyboardInput) {
    ensure_app_command_world_resources(world);
    world.insert_resource(Messages::<KeyboardInput>::default());
    world.resource_mut::<Messages<KeyboardInput>>().write(event);
    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    if !world.resource::<Messages<AppCommand>>().is_empty() {
        run_app_command_cycle(world);
    }
}

/// Injects one keyboard event through the direct-input handler and then the modal keyboard handler,
/// matching the real schedule order.
pub(super) fn dispatch_terminal_ui_key(world: &mut World, event: KeyboardInput) {
    ensure_app_command_world_resources(world);
    world.insert_resource(Messages::<KeyboardInput>::default());
    world.resource_mut::<Messages<KeyboardInput>>().write(event);
    world
        .run_system_once(handle_terminal_direct_input_keyboard)
        .unwrap();
    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    if !world.resource::<Messages<AppCommand>>().is_empty() {
        run_app_command_cycle(world);
    }
}

/// Injects one keyboard event through the real scheduled keyboard path: global shortcuts,
/// lifecycle shortcuts, direct-input handling, modal/terminal shortcuts, HUD shortcuts, then app
/// command dispatch.
pub(super) fn dispatch_key_through_real_keyboard_pipeline(world: &mut World, event: KeyboardInput) {
    ensure_app_command_world_resources(world);
    world.insert_resource(Messages::<KeyboardInput>::default());
    let mut primary_window_query = world.query_filtered::<Entity, With<PrimaryWindow>>();
    if primary_window_query.iter(world).next().is_none() {
        world.spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            PrimaryWindow,
        ));
    }

    let mut schedule = bevy::ecs::schedule::Schedule::default();
    schedule.add_systems(handle_keyboard_input);
    let _ = schedule.initialize(world);
    world.resource_mut::<Messages<KeyboardInput>>().write(event);
    schedule.run(world);

    if !world.resource::<Messages<AppCommand>>().is_empty() {
        run_app_command_cycle(world);
    }
}

pub(super) fn dispatch_terminal_wheel(world: &mut World, event: MouseWheel) {
    ensure_app_command_world_resources(world);
    world.insert_resource(Messages::<MouseWheel>::default());
    world.resource_mut::<Messages<MouseWheel>>().write(event);
    world
        .run_system_once(scroll_terminal_with_mouse_wheel)
        .unwrap();
    world.run_system_once(zoom_terminal_view).unwrap();
}

pub(super) fn set_terminal_surface_rows(
    world: &mut World,
    terminal_id: crate::terminals::TerminalId,
    rows: usize,
) {
    let mut manager = world.resource_mut::<TerminalManager>();
    let terminal = manager.get_mut(terminal_id).expect("terminal should exist");
    terminal.snapshot.surface = Some(crate::terminals::TerminalSurface::new(80, rows));
}

/// Builds a unique temporary directory path for one filesystem-backed input test.
pub(super) fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("neozeus-{prefix}-{nanos}-{}", std::process::id()))
}

/// Builds a test world containing one focused terminal panel plus the receiver for commands sent to
/// its bridge.
pub(super) fn world_with_active_terminal_and_receiver_and_mailbox(
    cursor: Vec2,
    panel_visible: bool,
    panel_position: Vec2,
) -> (
    World,
    crate::terminals::TerminalId,
    std::sync::mpsc::Receiver<TerminalCommand>,
    std::sync::Arc<crate::terminals::TerminalUpdateMailbox>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let (bridge, input_rx, mailbox) = capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    let session_name = manager
        .get(terminal_id)
        .expect("terminal should exist")
        .session_name
        .clone();

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("agent-1".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentCapabilities::terminal_defaults(),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, session_name, None);

    let mut world = World::default();
    let mut window = Window::default();
    window.set_cursor_position(Some(cursor));
    window.focused = true;

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(MessageTransportAdapter);
    world.insert_resource(TerminalNotesState::default());
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id: terminal_id },
        TerminalPresentation {
            home_position: panel_position,
            current_position: panel_position,
            target_position: panel_position,
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        if panel_visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        },
    ));

    (world, terminal_id, input_rx, mailbox)
}

pub(super) fn world_with_active_terminal_and_receiver(
    cursor: Vec2,
    panel_visible: bool,
    panel_position: Vec2,
) -> (
    World,
    crate::terminals::TerminalId,
    std::sync::mpsc::Receiver<TerminalCommand>,
) {
    let (world, terminal_id, input_rx, _mailbox) =
        world_with_active_terminal_and_receiver_and_mailbox(cursor, panel_visible, panel_position);
    (world, terminal_id, input_rx)
}

/// Convenience wrapper around `world_with_active_terminal_and_receiver` when the bridge receiver is
/// not needed.
pub(super) fn world_with_active_terminal(
    cursor: Vec2,
    panel_visible: bool,
    panel_position: Vec2,
) -> (World, crate::terminals::TerminalId) {
    let (world, terminal_id, _input_rx) =
        world_with_active_terminal_and_receiver(cursor, panel_visible, panel_position);
    (world, terminal_id)
}

