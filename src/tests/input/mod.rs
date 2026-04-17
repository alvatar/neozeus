use super::{
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

const DIRECT_INPUT_TYPING_BURST_KEYS: usize = 512;
const MESSAGE_BOX_TYPING_BURST_KEYS: usize = 512;
const DIRECT_INPUT_TYPING_OVERHEAD_RATIO_MAX: f64 = 4.0;
const MESSAGE_BOX_TYPING_OVERHEAD_RATIO_MAX: f64 = 4.25;

mod clone_cases;

fn wheel_lines(y: f32) -> MouseWheel {
    MouseWheel {
        x: 0.0,
        y,
        unit: MouseScrollUnit::Line,
        window: Entity::PLACEHOLDER,
    }
}

fn wheel_pixels(y: f32) -> MouseWheel {
    MouseWheel {
        x: 0.0,
        y,
        unit: MouseScrollUnit::Pixel,
        window: Entity::PLACEHOLDER,
    }
}

/// Builds a pressed `KeyboardInput` event without text payload for shortcut-oriented tests.
fn pressed_key(key_code: KeyCode, logical_key: Key) -> KeyboardInput {
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
fn init_hud_commands(world: &mut World) {
    world.init_resource::<Messages<AppCommand>>();
}

/// Drains and collects queued app commands.
fn drain_hud_commands(world: &mut World) -> Vec<AppCommand> {
    world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap()
}

/// Ensures app command world resources exists and returns its identifier.
fn ensure_app_command_world_resources(world: &mut World) {
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
fn run_app_command_cycle(world: &mut World) {
    ensure_app_command_world_resources(world);
    crate::app::run_apply_app_commands(world);
}

/// Injects one keyboard event into the modal-editor keyboard handler under test.
fn dispatch_message_box_key(world: &mut World, event: KeyboardInput) {
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
fn dispatch_terminal_ui_key(world: &mut World, event: KeyboardInput) {
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
fn dispatch_key_through_real_keyboard_pipeline(world: &mut World, event: KeyboardInput) {
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

fn dispatch_terminal_wheel(world: &mut World, event: MouseWheel) {
    ensure_app_command_world_resources(world);
    world.insert_resource(Messages::<MouseWheel>::default());
    world.resource_mut::<Messages<MouseWheel>>().write(event);
    world
        .run_system_once(scroll_terminal_with_mouse_wheel)
        .unwrap();
    world.run_system_once(zoom_terminal_view).unwrap();
}

fn set_terminal_surface_rows(
    world: &mut World,
    terminal_id: crate::terminals::TerminalId,
    rows: usize,
) {
    let mut manager = world.resource_mut::<TerminalManager>();
    let terminal = manager.get_mut(terminal_id).expect("terminal should exist");
    terminal.snapshot.surface = Some(crate::terminals::TerminalSurface::new(80, rows));
}

/// Builds a unique temporary directory path for one filesystem-backed input test.
fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("neozeus-{prefix}-{nanos}-{}", std::process::id()))
}

/// Builds a test world containing one focused terminal panel plus the receiver for commands sent to
/// its bridge.
fn world_with_active_terminal_and_receiver_and_mailbox(
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

fn world_with_active_terminal_and_receiver(
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
fn world_with_active_terminal(
    cursor: Vec2,
    panel_visible: bool,
    panel_position: Vec2,
) -> (World, crate::terminals::TerminalId) {
    let (world, terminal_id, _input_rx) =
        world_with_active_terminal_and_receiver(cursor, panel_visible, panel_position);
    (world, terminal_id)
}

/// Verifies a few representative control-sequence mappings used by terminal keyboard translation.
#[test]
fn ctrl_sequence_maps_common_shortcuts() {
    assert_eq!(ctrl_sequence(KeyCode::KeyC), Some("\u{3}"));
    assert_eq!(ctrl_sequence(KeyCode::KeyL), Some("\u{c}"));
    assert_eq!(ctrl_sequence(KeyCode::Enter), None);
}

/// Verifies that ordinary printable key events become `InputText` terminal commands.
#[test]
fn plain_text_uses_text_payload() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyA, Some("a"));
    let command = keyboard_input_to_terminal_command(&event, &keys);
    match command {
        Some(TerminalCommand::InputText(text)) => assert_eq!(text, "a"),
        _ => panic!("expected text input command"),
    }
}

#[test]
fn widget_toggle_and_reset_work_in_full_keyboard_path() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut hud_state = crate::hud::HudState::default();
    hud_state.insert_default_module(crate::hud::HudWidgetKey::AgentList);
    insert_test_hud_state(&mut world, hud_state);

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_text(KeyCode::Digit1, Some("1")),
    );
    assert!(!world
        .resource::<crate::hud::HudLayoutState>()
        .module_enabled(crate::hud::HudWidgetKey::AgentList));

    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::Digit1, Key::Character("!".into())),
    );
    assert!(world
        .resource::<crate::hud::HudLayoutState>()
        .module_enabled(crate::hud::HudWidgetKey::AgentList));
}

/// Verifies that the global spawn shortcut is accepted only for an unmodified physical `z` key press.
#[test]
fn global_spawn_shortcut_only_uses_plain_z() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyZ, Some("z"));
    assert!(should_spawn_terminal_globally(&event, &keys));

    let capslock_like_event = pressed_key(KeyCode::KeyZ, Key::Character("Z".into()));
    assert!(should_spawn_terminal_globally(&capslock_like_event, &keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_spawn_terminal_globally(&event, &ctrl_keys));

    let mut shift_keys = ButtonInput::<KeyCode>::default();
    shift_keys.press(KeyCode::ShiftLeft);
    assert!(!should_spawn_terminal_globally(&event, &shift_keys));
}

/// Verifies that the global spawn shortcut opens the centered create-agent dialog even when another
/// terminal is already active.
#[test]
fn global_spawn_shortcut_opens_create_agent_dialog_even_with_active_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);

    let mut world = World::default();
    let window = Window {
        focused: true,
        ..Default::default()
    };
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyZ, Some("z")));

    world
        .run_system_once(handle_global_terminal_spawn_shortcut)
        .unwrap();

    let session = world.resource::<AppSessionState>();
    assert!(session.create_agent_dialog.visible);
    assert_eq!(session.create_agent_dialog.kind, CreateAgentKind::Pi);
    assert_eq!(session.create_agent_dialog.name_field.text, "");
    assert_eq!(session.create_agent_dialog.cwd_field.field.text, "~/code");
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Name
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that `Tab` advances through every create-agent control, including the create button.
#[test]
fn create_agent_dialog_tab_advances_focus() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .focus,
        CreateAgentDialogField::Kind
    );

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .focus,
        CreateAgentDialogField::StartingFolder
    );

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .focus,
        CreateAgentDialogField::CreateButton
    );
}

/// Verifies that pressing `Space` toggles the selected type while the type row is focused.
#[test]
fn create_agent_dialog_space_toggles_type() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session.create_agent_dialog.focus = CreateAgentDialogField::Kind;
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));

    let session = world.resource::<AppSessionState>();
    assert_eq!(session.create_agent_dialog.kind, CreateAgentKind::Claude);
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Kind
    );
}

/// Verifies that `Ctrl+Space` in the cwd field starts completion and cycles matching directories.
#[test]
fn create_agent_dialog_ctrl_space_cycles_cwd_completions() {
    let root = unique_temp_dir("cwd-cycle");
    std::fs::create_dir_all(root.join("code")).unwrap();
    std::fs::create_dir_all(root.join("configs")).unwrap();

    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session.create_agent_dialog.focus = CreateAgentDialogField::StartingFolder;
        session
            .create_agent_dialog
            .cwd_field
            .load_text(&format!("{}/co", root.display()));
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    {
        let session = world.resource::<AppSessionState>();
        assert_eq!(
            session.create_agent_dialog.cwd_field.field.text,
            format!("{}/code/", root.display())
        );
        assert!(session.create_agent_dialog.cwd_field.completion.is_some());
    }

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    let session = world.resource::<AppSessionState>();
    assert_eq!(
        session.create_agent_dialog.cwd_field.field.text,
        format!("{}/configs/", root.display())
    );

    let _ = std::fs::remove_dir_all(root);
}

/// Verifies that `Enter` in the cwd field accepts the current completion and opens the next level.
#[test]
fn create_agent_dialog_enter_descends_into_selected_cwd_completion() {
    let root = unique_temp_dir("cwd-enter");
    std::fs::create_dir_all(root.join("code").join("alpha")).unwrap();
    std::fs::create_dir_all(root.join("configs")).unwrap();

    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session.create_agent_dialog.focus = CreateAgentDialogField::StartingFolder;
        session
            .create_agent_dialog
            .cwd_field
            .load_text(&format!("{}/co", root.display()));
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::Space, Some(" ")));
    world.insert_resource(ButtonInput::<KeyCode>::default());
    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let session = world.resource::<AppSessionState>();
    assert_eq!(
        session.create_agent_dialog.cwd_field.field.text,
        format!("{}/code/", root.display())
    );
    let completion = session
        .create_agent_dialog
        .cwd_field
        .completion
        .as_ref()
        .expect("next-level completion should stay open");
    assert!(!completion.preview_active);
    assert_eq!(
        completion.items[0].completion_text,
        format!("{}/code/alpha/", root.display())
    );

    let _ = std::fs::remove_dir_all(root);
}

/// Verifies that `Ctrl+U` clears the create-agent name field.
#[test]
fn create_agent_dialog_ctrl_u_clears_name_field() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session.create_agent_dialog.name_field.load_text("ORACLE");
        session.create_agent_dialog.focus = CreateAgentDialogField::Name;
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyU, Key::Character("u".into())),
    );

    let session = world.resource::<AppSessionState>();
    assert_eq!(session.create_agent_dialog.name_field.text, "");
    assert_eq!(session.create_agent_dialog.name_field.cursor, 0);
}

/// Verifies that typed create-agent names are uppercased immediately in the field.
#[test]
fn create_agent_dialog_typing_uppercases_name_field() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyB, Some("b")));

    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .name_field
            .text,
        "AB"
    );
}

#[test]
fn middle_click_paste_in_create_agent_dialog_inserts_into_text_fields() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(&world)
        .expect("primary window should exist")
        .clone();
    let name_rect = create_agent_name_field_rect(&window);
    let cwd_rect = create_agent_starting_folder_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_create_agent_dialog(
        &mut app_session,
        &window,
        Vec2::new(name_rect.x + 4.0, name_rect.y + 4.0),
        "mixedCase",
    ));
    assert!(paste_into_create_agent_dialog(
        &mut app_session,
        &window,
        Vec2::new(cwd_rect.x + 4.0, cwd_rect.y + 4.0),
        "/tmp/work",
    ));

    assert_eq!(app_session.create_agent_dialog.name_field.text, "MIXEDCASE");
    assert_eq!(
        app_session.create_agent_dialog.cwd_field.field.text,
        "~/code/tmp/work"
    );
}

/// Verifies that `Escape` cancels the create-agent dialog without spawning anything.
#[test]
fn create_agent_dialog_escape_closes_without_spawning() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Escape, Key::Escape));

    assert!(
        !world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .visible
    );
}

/// Verifies that submitting the create-agent dialog emits the configured agent-create command.
#[test]
fn create_agent_dialog_submit_emits_create_command() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Terminal);
        session.create_agent_dialog.name_field.load_text("oracle");
        session.create_agent_dialog.cwd_field.load_text("~/code");
        session.create_agent_dialog.focus = CreateAgentDialogField::CreateButton;
    }
    ensure_app_command_world_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::Enter, Key::Enter));
    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Agent(AppAgentCommand::Create {
            label: Some("ORACLE".into()),
            kind: crate::agents::AgentKind::Terminal,
            working_directory: "~/code".into(),
        })]
    );
}

/// Verifies that the kill-active-terminal shortcut is accepted for `Ctrl+k`, regardless of Shift,
/// and still rejects unrelated modifier mixes.
#[test]
fn kill_active_terminal_shortcut_accepts_ctrl_k_even_with_shift() {
    let event = pressed_text(KeyCode::KeyK, Some("k"));
    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(should_kill_active_terminal(&event, &ctrl_keys));

    let mut shift_ctrl_keys = ButtonInput::<KeyCode>::default();
    shift_ctrl_keys.press(KeyCode::ControlLeft);
    shift_ctrl_keys.press(KeyCode::ShiftLeft);
    assert!(should_kill_active_terminal(&event, &shift_ctrl_keys));

    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(!should_kill_active_terminal(&event, &plain_keys));

    let mut alt_ctrl_keys = ButtonInput::<KeyCode>::default();
    alt_ctrl_keys.press(KeyCode::ControlLeft);
    alt_ctrl_keys.press(KeyCode::AltLeft);
    assert!(!should_kill_active_terminal(&event, &alt_ctrl_keys));
}

#[test]
fn ctrl_alt_r_opens_reset_dialog_without_emitting_command() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyR, Some("r")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(world.resource::<AppSessionState>().reset_dialog.visible);
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset requested: confirmation required")
    );
    assert!(world.resource::<Messages<AppCommand>>().is_empty());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn ctrl_alt_shift_r_still_opens_reset_dialog_without_emitting_command() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    keys.press(KeyCode::ShiftLeft);
    world.insert_resource(keys);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyR, Key::Character("R".into())));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(world.resource::<AppSessionState>().reset_dialog.visible);
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset requested: confirmation required")
    );
    assert!(world.resource::<Messages<AppCommand>>().is_empty());
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn ctrl_alt_r_opens_reset_dialog_in_full_keyboard_path() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyR, Some("r")));

    assert!(world.resource::<AppSessionState>().reset_dialog.visible);
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset requested: confirmation required")
    );
    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}

#[test]
fn ctrl_alt_r_is_suppressed_while_other_modal_has_keyboard_capture() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Pi);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyR, Some("r")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(!world.resource::<AppSessionState>().reset_dialog.visible);
    assert!(world
        .resource::<AppSessionState>()
        .recovery_status
        .title
        .is_none());
    assert!(world.resource::<Messages<AppCommand>>().is_empty());
}

/// Verifies that the application-exit shortcut ignores Shift and only rejects Ctrl/Alt/Super.
#[test]
fn exit_application_shortcut_ignores_shift() {
    let event = pressed_text(KeyCode::F10, None);
    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(should_exit_application(&event, &plain_keys));

    let mut shift_keys = ButtonInput::<KeyCode>::default();
    shift_keys.press(KeyCode::ShiftLeft);
    assert!(should_exit_application(&event, &shift_keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_exit_application(&event, &ctrl_keys));

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    assert!(!should_exit_application(&event, &alt_keys));
}

/// Verifies that one plain `Ctrl+k` removes a disconnected active terminal in one shot.
#[test]
fn ctrl_k_removes_disconnected_active_terminal_in_one_press() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client));
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        None
    );
}

#[test]
fn ctrl_k_removes_disconnected_active_terminal_in_full_keyboard_path() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client));
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyK, Some("k")));

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        None
    );
}

/// Verifies that one plain `Ctrl+k` still removes the active terminal when the local runtime
/// snapshot is stale but the daemon already reports that session as disconnected.
#[test]
fn ctrl_k_removes_terminal_when_daemon_runtime_is_disconnected_but_local_snapshot_is_stale() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let session_name = world
        .resource::<TerminalManager>()
        .get(terminal_id)
        .expect("terminal should exist")
        .session_name
        .clone();
    client.set_session_runtime(
        &session_name,
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );
    world.insert_resource(fake_runtime_spawner(client));
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));

    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        None
    );
}

/// Verifies that the lifecycle shortcut handler turns `F10` into an app-exit message.
#[test]
fn f10_enqueues_app_exit() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::F10, None));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert_eq!(world.resource::<Messages<AppExit>>().len(), 1);
    assert!(drain_hud_commands(&mut world).is_empty());
}

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

/// Verifies that plain `r` opens the rename-agent dialog prefilled from the active agent label.
#[test]
fn plain_r_opens_rename_dialog_for_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyR, Some("r")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();

    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.rename_agent_dialog.visible);
    assert_eq!(app_session.rename_agent_dialog.target_agent, Some(agent_id));
    assert_eq!(app_session.rename_agent_dialog.name_field.text, "AGENT-1");
    assert_eq!(
        app_session.rename_agent_dialog.focus,
        RenameAgentDialogField::Name
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that confirming the rename dialog renames the active agent and closes the modal.
#[test]
fn rename_dialog_enter_submits_agent_rename() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session
            .rename_agent_dialog
            .name_field
            .load_text("renamed");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::RenameButton;
    }
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        world
            .resource::<crate::agents::AgentCatalog>()
            .label(agent_id),
        Some("RENAMED")
    );
    assert!(
        !world
            .resource::<AppSessionState>()
            .rename_agent_dialog
            .visible
    );
}

#[test]
fn rename_dialog_updates_live_daemon_metadata() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let session_name = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .session_name(agent_id)
        .unwrap()
        .to_owned();
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session
            .rename_agent_dialog
            .name_field
            .load_text("renamed");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::RenameButton;
    }
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        client
            .session_metadata
            .lock()
            .unwrap()
            .get(&session_name)
            .and_then(|metadata| metadata.agent_label.as_deref()),
        Some("RENAMED")
    );
}

/// Verifies that `Ctrl+U` clears the create-agent cwd field.
#[test]
fn create_agent_dialog_ctrl_u_clears_cwd_field() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        session
            .create_agent_dialog
            .cwd_field
            .load_text("~/code/project");
        session.create_agent_dialog.focus = CreateAgentDialogField::StartingFolder;
    }
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyU, Key::Character("u".into())),
    );

    let session = world.resource::<AppSessionState>();
    assert_eq!(session.create_agent_dialog.cwd_field.field.text, "");
    assert_eq!(session.create_agent_dialog.cwd_field.field.cursor, 0);
}

/// Verifies that typed rename values are uppercased immediately in the field.
#[test]
fn rename_dialog_typing_uppercases_name_field() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session.rename_agent_dialog.name_field.clear();
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
    }
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyB, Some("b")));

    assert_eq!(
        world
            .resource::<AppSessionState>()
            .rename_agent_dialog
            .name_field
            .text,
        "AB"
    );
}

#[test]
fn reset_dialog_escape_preempts_rename_dialog() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.reset_dialog.visible = true;
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
    }
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Escape, Key::Escape));

    let app_session = world.resource::<AppSessionState>();
    assert!(!app_session.reset_dialog.visible);
    assert!(app_session.rename_agent_dialog.visible);
}

#[test]
fn rename_dialog_typing_preempts_message_editor_typing() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(terminal_id);
    insert_test_hud_state(&mut world, hud_state);
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session.rename_agent_dialog.name_field.clear();
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
        app_session.composer.message_editor.load_text("draft");
    }
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyZ, Some("z")));

    let app_session = world.resource::<AppSessionState>();
    assert_eq!(app_session.rename_agent_dialog.name_field.text, "Z");
    assert_eq!(app_session.composer.message_editor.text, "draft");
}

#[test]
fn rename_dialog_ctrl_u_clears_name_field() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session
            .rename_agent_dialog
            .name_field
            .load_text("RENAMED");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
    }
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<KeyboardInput>>();

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    world.insert_resource(ctrl_keys);
    dispatch_message_box_key(
        &mut world,
        pressed_key(KeyCode::KeyU, Key::Character("u".into())),
    );

    let app_session = world.resource::<AppSessionState>();
    assert_eq!(app_session.rename_agent_dialog.name_field.text, "");
    assert_eq!(app_session.rename_agent_dialog.name_field.cursor, 0);
}

#[test]
fn middle_click_paste_in_rename_dialog_uppercases_text() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session.rename_agent_dialog.name_field.clear();
    }

    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(&world)
        .expect("primary window should exist")
        .clone();
    let rect = rename_agent_name_field_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_rename_agent_dialog(
        &mut app_session,
        &window,
        Vec2::new(rect.x + 4.0, rect.y + 4.0),
        "renamed",
    ));
    assert_eq!(app_session.rename_agent_dialog.name_field.text, "RENAMED");
}

#[test]
fn rename_dialog_keeps_local_label_unchanged_when_metadata_sync_fails() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_update_session_metadata.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.insert_resource(fake_runtime_spawner(client));
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session
            .rename_agent_dialog
            .name_field
            .load_text("renamed");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::RenameButton;
    }
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        world
            .resource::<crate::agents::AgentCatalog>()
            .label(agent_id),
        Some("AGENT-1")
    );
    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.rename_agent_dialog.visible);
    assert_eq!(
        app_session.rename_agent_dialog.error.as_deref(),
        Some("update metadata failed")
    );
}

/// Verifies that duplicate rename targets are rejected and keep the rename dialog open.
#[test]
fn rename_dialog_rejects_duplicate_agent_name() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world
        .resource_mut::<crate::agents::AgentCatalog>()
        .create_agent(
            Some("beta".into()),
            crate::agents::AgentKind::Terminal,
            crate::agents::AgentCapabilities::terminal_defaults(),
        );
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.rename_agent_dialog.open(agent_id, "AGENT-1");
        app_session.rename_agent_dialog.name_field.load_text("beta");
        app_session.rename_agent_dialog.focus = RenameAgentDialogField::RenameButton;
    }
    init_hud_commands(&mut world);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert_eq!(
        world
            .resource::<crate::agents::AgentCatalog>()
            .label(agent_id),
        Some("AGENT-1")
    );
    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.rename_agent_dialog.visible);
    assert_eq!(
        app_session.rename_agent_dialog.error.as_deref(),
        Some("agent `BETA` already exists")
    );
}

#[test]
fn plain_a_opens_aegis_dialog_for_active_terminal_with_default_prompt() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.aegis_dialog.visible);
    assert_eq!(app_session.aegis_dialog.target_agent, Some(agent_id));
    assert_eq!(
        app_session.aegis_dialog.prompt_editor.text,
        DEFAULT_AEGIS_PROMPT
    );
    assert_eq!(app_session.aegis_dialog.focus, AegisDialogField::Prompt);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn disabled_agent_reopens_aegis_dialog_with_saved_prompt() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    ensure_app_command_world_resources(&mut world);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let agent_uid = world
        .resource::<crate::agents::AgentCatalog>()
        .uid(agent_id)
        .expect("agent uid should exist")
        .to_owned();
    world
        .resource_mut::<crate::aegis::AegisPolicyStore>()
        .upsert_disabled_prompt(&agent_uid, "saved custom prompt".into());

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    assert_eq!(
        world
            .resource::<AppSessionState>()
            .aegis_dialog
            .prompt_editor
            .text,
        "saved custom prompt"
    );
}

#[test]
fn aegis_dialog_enable_button_persists_custom_text_and_closes() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    ensure_app_command_world_resources(&mut world);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let agent_uid = world
        .resource::<crate::agents::AgentCatalog>()
        .uid(agent_id)
        .expect("agent uid should exist")
        .to_owned();
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session
            .aegis_dialog
            .open(agent_id, DEFAULT_AEGIS_PROMPT);
        app_session
            .aegis_dialog
            .prompt_editor
            .load_text("custom aegis prompt");
        app_session.aegis_dialog.focus = AegisDialogField::EnableButton;
    }
    world.init_resource::<Messages<RequestRedraw>>();

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    assert!(!world.resource::<AppSessionState>().aegis_dialog.visible);
    assert!(world
        .resource::<crate::aegis::AegisPolicyStore>()
        .is_enabled(&agent_uid));
    assert_eq!(
        world
            .resource::<crate::aegis::AegisPolicyStore>()
            .prompt_text(&agent_uid),
        Some("custom aegis prompt")
    );
    assert_eq!(
        world
            .resource::<crate::aegis::AegisRuntimeStore>()
            .state(agent_id),
        Some(crate::aegis::AegisRuntimeState::Armed)
    );
}

#[test]
fn aegis_dialog_rejects_empty_prompt() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session
            .aegis_dialog
            .open(agent_id, DEFAULT_AEGIS_PROMPT);
        app_session.aegis_dialog.prompt_editor.load_text("");
        app_session.aegis_dialog.focus = AegisDialogField::EnableButton;
    }

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));

    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.aegis_dialog.visible);
    assert_eq!(
        app_session.aegis_dialog.error.as_deref(),
        Some("Aegis prompt is required")
    );
}

#[test]
fn aegis_dialog_prompt_accepts_multiline_text_without_submitting() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.aegis_dialog.open(agent_id, "line one");
        app_session.aegis_dialog.focus = AegisDialogField::Prompt;
    }

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Enter, Key::Enter));
    dispatch_message_box_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    let app_session = world.resource::<AppSessionState>();
    assert!(app_session.aegis_dialog.visible);
    assert_eq!(app_session.aegis_dialog.prompt_editor.text, "line one\na");
}

#[test]
fn plain_a_disables_enabled_aegis_for_active_terminal() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    ensure_app_command_world_resources(&mut world);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let agent_uid = world
        .resource::<crate::agents::AgentCatalog>()
        .uid(agent_id)
        .expect("agent uid should exist")
        .to_owned();
    world
        .resource_mut::<crate::aegis::AegisPolicyStore>()
        .enable(&agent_uid, "custom aegis prompt".into());
    world
        .resource_mut::<crate::aegis::AegisRuntimeStore>()
        .set_state(agent_id, crate::aegis::AegisRuntimeState::Armed);

    dispatch_terminal_ui_key(&mut world, pressed_text(KeyCode::KeyA, Some("a")));

    assert!(!world
        .resource::<crate::aegis::AegisPolicyStore>()
        .is_enabled(&agent_uid));
    assert!(world
        .resource::<crate::aegis::AegisRuntimeStore>()
        .state(agent_id)
        .is_none());
}

#[test]
fn middle_click_paste_in_aegis_dialog_inserts_prompt_text() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.aegis_dialog.open(agent_id, "");
        app_session.aegis_dialog.prompt_editor.load_text("");
    }

    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(&world)
        .expect("primary window should exist")
        .clone();
    let rect = aegis_prompt_field_rect(&window);
    let mut app_session = world.resource_mut::<AppSessionState>();

    assert!(paste_into_aegis_dialog(
        &mut app_session,
        &window,
        Vec2::new(rect.x + 4.0, rect.y + 4.0),
        "continue cleanly"
    ));
    assert_eq!(
        app_session.aegis_dialog.prompt_editor.text,
        "continue cleanly"
    );
}

/// Verifies that plain `n` enqueues the consume-next-task intent for the active terminal.
#[test]
fn plain_n_enqueues_consume_next_task_for_active_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyN, Some("n")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Task(AppTaskCommand::ConsumeNext { agent_id })]
    );
}

#[test]
fn plain_p_toggles_paused_state_for_active_terminal_agent() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyP, Some("p")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);
    assert!(world.resource::<AgentCatalog>().is_paused(agent_id));

    world.insert_resource(Messages::<AppCommand>::default());
    world.insert_resource(Messages::<KeyboardInput>::default());
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyP, Key::Character("P".into())));
    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);
    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
}

#[test]
fn plain_p_toggles_only_once_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<AppSessionState>()
        .focus_intent
        .focus_agent(agent_id, crate::app::VisibilityMode::ShowAll);

    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyP, Some("p")));

    assert!(
        world.resource::<AgentCatalog>().is_paused(agent_id),
        "plain p should toggle once even when terminal and HUD shortcut systems both run"
    );
}

#[test]
fn plain_p_toggles_selected_focus_agent_without_active_terminal_target() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let _ = world
        .resource_mut::<crate::terminals::TerminalFocusState>()
        .clear_active_terminal();
    world
        .resource_mut::<AppSessionState>()
        .focus_intent
        .focus_agent(agent_id, crate::app::VisibilityMode::ShowAll);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyP, Some("p")));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert!(
        world.resource::<AgentCatalog>().is_paused(agent_id),
        "plain p should still toggle the selected focused agent when no interactive terminal owns the shortcut"
    );
}

#[test]
fn shift_p_does_not_toggle_paused_state_for_active_terminal_agent() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyP, Key::Character("P".into())));

    world
        .run_system_once(handle_terminal_message_box_keyboard)
        .unwrap();
    run_app_command_cycle(&mut world);
    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
}

#[test]
fn shift_p_does_not_toggle_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::KeyP, Key::Character("P".into())),
    );

    assert!(!world.resource::<AgentCatalog>().is_paused(agent_id));
}

#[test]
fn plain_i_toggles_selected_agent_context_box_in_full_keyboard_path() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));

    assert!(
        !world
            .resource::<crate::hud::AgentListUiState>()
            .show_selected_context,
        "precondition: selected-agent context box should start disabled"
    );
    dispatch_key_through_real_keyboard_pipeline(&mut world, pressed_text(KeyCode::KeyI, Some("i")));
    assert!(
        world
            .resource::<crate::hud::AgentListUiState>()
            .show_selected_context
    );

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::KeyI, Key::Character("I".into())),
    );
    assert!(
        !world
            .resource::<crate::hud::AgentListUiState>()
            .show_selected_context
    );
}

#[test]
fn shift_i_does_not_toggle_selected_agent_context_box() {
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::KeyI, Key::Character("I".into())),
    );

    assert!(
        !world
            .resource::<crate::hud::AgentListUiState>()
            .show_selected_context
    );
}

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

#[test]
fn wheel_scroll_sends_scrollback_to_focused_terminal_in_visual_mode() {
    let (mut world, _terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);

    dispatch_terminal_wheel(&mut world, wheel_lines(2.0));
    dispatch_terminal_wheel(&mut world, wheel_lines(-3.0));

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(2)
    );
    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(-3)
    );
}

#[test]
fn wheel_scroll_sends_scrollback_to_direct_input_terminal() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);
    insert_test_hud_state(&mut world, hud_state);

    dispatch_terminal_wheel(&mut world, wheel_lines(1.0));

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(1)
    );
}

#[test]
fn wheel_scroll_accumulates_fractional_pixel_deltas() {
    let (mut world, _terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);

    dispatch_terminal_wheel(&mut world, wheel_pixels(10.0));
    dispatch_terminal_wheel(&mut world, wheel_pixels(10.0));
    assert!(input_rx.try_recv().is_err());

    dispatch_terminal_wheel(&mut world, wheel_pixels(10.0));
    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(1)
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
fn control_v_scrolls_many_rows_down_when_terminal_is_not_captured() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);

    dispatch_terminal_ui_key(
        &mut world,
        pressed_key(KeyCode::KeyV, Key::Character("v".into())),
    );

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(-39)
    );
}

#[test]
fn control_shift_v_still_scrolls_many_rows_down_when_terminal_is_not_captured() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);

    dispatch_terminal_ui_key(
        &mut world,
        pressed_key(KeyCode::KeyV, Key::Character("V".into())),
    );

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(-39)
    );
}

#[test]
fn alt_v_scrolls_many_rows_up_when_terminal_is_not_captured() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);

    dispatch_terminal_ui_key(
        &mut world,
        pressed_key(KeyCode::KeyV, Key::Character("v".into())),
    );

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(39)
    );
}

#[test]
fn alt_shift_v_still_scrolls_many_rows_up_when_terminal_is_not_captured() {
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    set_terminal_surface_rows(&mut world, terminal_id, 40);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);

    dispatch_terminal_ui_key(
        &mut world,
        pressed_key(KeyCode::KeyV, Key::Character("V".into())),
    );

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::ScrollDisplay(39)
    );
}

#[test]
fn shift_wheel_keeps_zoom_and_does_not_send_scrollback() {
    let (mut world, _terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    let starting_distance = world
        .resource::<crate::terminals::TerminalViewState>()
        .distance;

    dispatch_terminal_wheel(&mut world, wheel_lines(2.0));

    assert!(input_rx.try_recv().is_err());
    assert!(
        world
            .resource::<crate::terminals::TerminalViewState>()
            .distance
            < starting_distance
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

/// Verifies that agent-list keyboard navigation lands on owned tmux child rows.
#[test]
fn ctrl_k_kills_selected_agent_without_hidden_session_state() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    let (mut world, terminal_id) =
        world_with_active_terminal(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
    let agent_id = world
        .resource::<AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<TerminalManager>()
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();
    run_app_command_cycle(&mut world);

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::None
    );
}

#[test]
fn hud_navigation_selects_owned_tmux_child_row() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(crate::hud::AgentListSelection::Agent(
        crate::agents::AgentId(1),
    ));
    world.insert_resource(crate::hud::AgentListView {
        rows: vec![
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(crate::agents::AgentId(1)),
                label: "ALPHA".into(),
                focused: true,
                kind: crate::hud::AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: None,
                    has_tasks: false,
                    interactive: true,
                    activity: crate::hud::AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            },
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::OwnedTmux("tmux-1".into()),
                label: "BUILD".into(),
                focused: false,
                kind: crate::hud::AgentListRowKind::OwnedTmux {
                    session_uid: "tmux-1".into(),
                    owner: crate::hud::OwnedTmuxOwnerBinding::Bound(crate::agents::AgentId(1)),
                    tmux_name: "neozeus-tmux-1".into(),
                    cwd: "/tmp/work".into(),
                    attached: false,
                },
            },
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(crate::agents::AgentId(2)),
                label: "BETA".into(),
                focused: false,
                kind: crate::hud::AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(2),
                    terminal_id: None,
                    has_tasks: false,
                    interactive: true,
                    activity: crate::hud::AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            },
        ],
    });
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyJ, Key::Character("j".into())));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::Select {
                session_uid: "tmux-1".into(),
            }
        )]
    );
}

#[test]
fn hud_navigation_works_in_full_keyboard_path() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    let (bridge, _) = test_bridge();
    let mut terminal_manager = TerminalManager::default();
    let terminal_id = terminal_manager.create_terminal(bridge);
    world.insert_resource(terminal_manager);
    world.insert_resource(crate::terminals::TerminalFocusState::default());

    let mut agent_catalog = crate::agents::AgentCatalog::default();
    let agent_a = agent_catalog.create_agent(
        Some("ALPHA".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_b = agent_catalog.create_agent(
        Some("BETA".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let owner_agent_uid = agent_catalog.uid(agent_a).unwrap().to_owned();
    let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_a, terminal_id, "agent-a-session".into(), None);
    world.insert_resource(agent_catalog);
    world.insert_resource(runtime_index);

    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_a));
    world.insert_resource(crate::hud::AgentListView {
        rows: vec![
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(agent_a),
                label: "ALPHA".into(),
                focused: true,
                kind: crate::hud::AgentListRowKind::Agent {
                    agent_id: agent_a,
                    terminal_id: Some(terminal_id),
                    has_tasks: false,
                    interactive: true,
                    activity: crate::hud::AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            },
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::OwnedTmux("tmux-session-1".into()),
                label: "BUILD".into(),
                focused: false,
                kind: crate::hud::AgentListRowKind::OwnedTmux {
                    session_uid: "tmux-session-1".into(),
                    owner: crate::hud::OwnedTmuxOwnerBinding::Bound(agent_a),
                    tmux_name: "neozeus-tmux-1".into(),
                    cwd: "/tmp/work".into(),
                    attached: false,
                },
            },
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(agent_b),
                label: "BETA".into(),
                focused: false,
                kind: crate::hud::AgentListRowKind::Agent {
                    agent_id: agent_b,
                    terminal_id: None,
                    has_tasks: false,
                    interactive: true,
                    activity: crate::hud::AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            },
        ],
    });
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world
        .resource_mut::<crate::terminals::OwnedTmuxSessionStore>()
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid,
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });

    dispatch_key_through_real_keyboard_pipeline(
        &mut world,
        pressed_key(KeyCode::KeyJ, Key::Character("j".into())),
    );

    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
}

#[test]
fn hud_navigation_jk_uses_agent_list_selection_as_single_source_of_truth() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(crate::hud::AgentListSelection::OwnedTmux("tmux-1".into()));
    world.insert_resource(crate::hud::AgentListView {
        rows: vec![
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(crate::agents::AgentId(1)),
                label: "ALPHA".into(),
                focused: false,
                kind: crate::hud::AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: None,
                    has_tasks: false,
                    interactive: true,
                    activity: crate::hud::AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            },
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::OwnedTmux("tmux-1".into()),
                label: "BUILD".into(),
                focused: true,
                kind: crate::hud::AgentListRowKind::OwnedTmux {
                    session_uid: "tmux-1".into(),
                    owner: crate::hud::OwnedTmuxOwnerBinding::Bound(crate::agents::AgentId(1)),
                    tmux_name: "neozeus-tmux-1".into(),
                    cwd: "/tmp/work".into(),
                    attached: false,
                },
            },
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(crate::agents::AgentId(2)),
                label: "BETA".into(),
                focused: false,
                kind: crate::hud::AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(2),
                    terminal_id: None,
                    has_tasks: false,
                    interactive: true,
                    activity: crate::hud::AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            },
        ],
    });
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyJ, Key::Character("j".into())));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(crate::agents::AgentId(2))),
            AppCommand::Agent(AppAgentCommand::Inspect(crate::agents::AgentId(2))),
        ]
    );
}

#[test]
fn hud_navigation_arrow_keys_uses_agent_list_selection_as_single_source_of_truth() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    let mut active_terminal_content = crate::terminals::ActiveTerminalContentState::default();
    active_terminal_content.select_owned_tmux("tmux-1".into(), None);
    world.insert_resource(active_terminal_content);
    world.insert_resource(crate::hud::AgentListSelection::Agent(
        crate::agents::AgentId(1),
    ));
    world.insert_resource(crate::hud::AgentListView {
        rows: vec![
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(crate::agents::AgentId(1)),
                label: "ALPHA".into(),
                focused: true,
                kind: crate::hud::AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: None,
                    has_tasks: false,
                    interactive: true,
                    activity: crate::hud::AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            },
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::OwnedTmux("tmux-1".into()),
                label: "BUILD".into(),
                focused: false,
                kind: crate::hud::AgentListRowKind::OwnedTmux {
                    session_uid: "tmux-1".into(),
                    owner: crate::hud::OwnedTmuxOwnerBinding::Bound(crate::agents::AgentId(1)),
                    tmux_name: "neozeus-tmux-1".into(),
                    cwd: "/tmp/work".into(),
                    attached: false,
                },
            },
        ],
    });
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::ArrowDown, Key::ArrowDown));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::Select {
                session_uid: "tmux-1".into(),
            }
        )]
    );
}

/// Verifies that global lifecycle shortcuts are ignored while the message box owns keyboard capture.
#[test]
fn lifecycle_shortcuts_are_suppressed_while_message_box_is_open() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(crate::terminals::TerminalId(1));
    insert_test_hud_state(&mut world, hud_state);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}

/// Verifies that `Ctrl+k` kills the selected owned tmux row before touching the active agent.
#[test]
fn ctrl_k_kills_selected_owned_tmux_session_before_selected_agent_row() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), None);
    world.insert_resource(crate::hud::AgentListSelection::OwnedTmux(
        "tmux-session-1".into(),
    ));
    init_hud_commands(&mut world);
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::KillSelected
        )]
    );
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}

#[test]
fn lifecycle_shortcuts_are_suppressed_while_direct_input_is_open() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(crate::terminals::TerminalId(1));
    insert_test_hud_state(&mut world, hud_state);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    world.insert_resource(keys);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppExit>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
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
