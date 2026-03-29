use super::{
    capturing_bridge, fake_runtime_spawner, insert_default_hud_resources,
    insert_terminal_manager_resources, insert_test_hud_state, pressed_text,
    snapshot_test_hud_state, test_bridge, FakeDaemonClient,
};
use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::{
        AgentCommand as AppAgentCommand, AppCommand, AppSessionState, AppStatePersistenceState,
        CreateAgentDialogField, CreateAgentKind, TaskCommand as AppTaskCommand,
    },
    conversations::{AgentTaskStore, ConversationStore, MessageTransportAdapter},
    hud::TerminalVisibilityState,
    input::{
        ctrl_sequence, focus_terminal_on_panel_click, handle_global_terminal_spawn_shortcut,
        handle_terminal_direct_input_keyboard, handle_terminal_lifecycle_shortcuts,
        handle_terminal_message_box_keyboard, hide_terminal_on_background_click,
        keyboard_input_to_terminal_command, should_exit_application, should_kill_active_terminal,
        should_spawn_shell_terminal_globally, should_spawn_terminal_globally,
    },
    terminals::{
        TerminalCommand, TerminalManager, TerminalNotesState, TerminalPanel, TerminalPresentation,
    },
};
use bevy::{
    app::AppExit,
    asset::Assets,
    ecs::system::RunSystemOnce,
    input::{
        keyboard::{Key, KeyboardInput},
        ButtonInput, ButtonState,
    },
    prelude::{
        Entity, Image, KeyCode, Messages, MouseButton, Time, Vec2, Visibility, Window, World,
    },
    window::{PrimaryWindow, RequestRedraw},
};

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
    if !world.contains_resource::<Time<()>>() {
        world.insert_resource(Time::<()>::default());
    }
    if !world.contains_resource::<Assets<Image>>() {
        world.insert_resource(Assets::<Image>::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalPresentationStore>() {
        world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    }
    if !world.contains_resource::<TerminalManager>() {
        world.insert_resource(TerminalManager::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalFocusState>() {
        world.insert_resource(crate::terminals::TerminalFocusState::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalRuntimeSpawner>() {
        world.insert_resource(fake_runtime_spawner(std::sync::Arc::new(
            FakeDaemonClient::default(),
        )));
    }
    if !world.contains_resource::<AgentCatalog>() {
        world.insert_resource(AgentCatalog::default());
    }
    if !world.contains_resource::<AgentRuntimeIndex>() {
        world.insert_resource(AgentRuntimeIndex::default());
    }
    if !world.contains_resource::<AppSessionState>() {
        world.insert_resource(AppSessionState::default());
    }
    if !world.contains_resource::<ConversationStore>() {
        world.insert_resource(ConversationStore::default());
    }
    if !world.contains_resource::<crate::conversations::ConversationPersistenceState>() {
        world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    }
    if !world.contains_resource::<AgentTaskStore>() {
        world.insert_resource(AgentTaskStore::default());
    }
    if !world.contains_resource::<MessageTransportAdapter>() {
        world.insert_resource(MessageTransportAdapter);
    }
    if !world.contains_resource::<TerminalNotesState>() {
        world.insert_resource(TerminalNotesState::default());
    }
    if !world.contains_resource::<AppStatePersistenceState>() {
        world.insert_resource(AppStatePersistenceState::default());
    }
    if !world.contains_resource::<TerminalVisibilityState>() {
        world.insert_resource(TerminalVisibilityState::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalViewState>() {
        world.insert_resource(crate::terminals::TerminalViewState::default());
    }
    if !world.contains_resource::<crate::hud::HudInputCaptureState>() {
        world.insert_resource(crate::hud::HudInputCaptureState::default());
    }
    if !world.contains_resource::<Messages<AppCommand>>() {
        world.init_resource::<Messages<AppCommand>>();
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
fn world_with_active_terminal_and_receiver(
    cursor: Vec2,
    panel_visible: bool,
    panel_position: Vec2,
) -> (
    World,
    crate::terminals::TerminalId,
    std::sync::mpsc::Receiver<TerminalCommand>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let (bridge, input_rx, _) = capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    let session_name = manager
        .get(terminal_id)
        .expect("terminal should exist")
        .session_name
        .clone();

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog
        .create_agent(
            Some("agent-1".into()),
            crate::agents::AgentKind::Terminal,
            crate::agents::AgentCapabilities::terminal_defaults(),
        )
        .unwrap();
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
    world.insert_resource(ConversationStore::default());
    world.insert_resource(AgentTaskStore::default());
    world.insert_resource(MessageTransportAdapter);
    world.insert_resource(TerminalNotesState::default());
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    init_hud_commands(&mut world);
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

/// Verifies that the global spawn shortcut is accepted only for an unmodified `z` key press.
#[test]
fn global_spawn_shortcut_only_uses_plain_z() {
    let keys = ButtonInput::<KeyCode>::default();
    let event = pressed_text(KeyCode::KeyZ, Some("z"));
    assert!(should_spawn_terminal_globally(&event, &keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_spawn_terminal_globally(&event, &ctrl_keys));
}

/// Verifies that the explicit shell-spawn shortcut is accepted only for `Ctrl+Alt+z`.
#[test]
fn global_shell_spawn_shortcut_only_uses_ctrl_alt_z() {
    let event = pressed_text(KeyCode::KeyZ, Some("z"));

    let mut ctrl_alt_keys = ButtonInput::<KeyCode>::default();
    ctrl_alt_keys.press(KeyCode::ControlLeft);
    ctrl_alt_keys.press(KeyCode::AltLeft);
    assert!(should_spawn_shell_terminal_globally(&event, &ctrl_alt_keys));

    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(!should_spawn_shell_terminal_globally(&event, &plain_keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_spawn_shell_terminal_globally(&event, &ctrl_keys));
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
    assert_eq!(session.create_agent_dialog.kind, CreateAgentKind::Agent);
    assert_eq!(session.create_agent_dialog.name_field.text, "");
    assert_eq!(session.create_agent_dialog.cwd_field.field.text, "~/code");
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Name
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that the explicit shell-spawn shortcut opens the create-agent dialog with shell kind
/// preselected.
#[test]
fn global_shell_spawn_shortcut_opens_shell_create_agent_dialog() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);

    let mut world = World::default();
    let window = Window {
        focused: true,
        ..Default::default()
    };
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
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
    assert_eq!(session.create_agent_dialog.kind, CreateAgentKind::Shell);
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Name
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that `Tab` advances focus through the create-agent dialog fields.
#[test]
fn create_agent_dialog_tab_advances_focus() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Agent);
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
}

/// Verifies that pressing `Space` toggles the selected type while the type row is focused.
#[test]
fn create_agent_dialog_space_toggles_type() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Agent);
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
    assert_eq!(session.create_agent_dialog.kind, CreateAgentKind::Shell);
    assert_eq!(
        session.create_agent_dialog.focus,
        CreateAgentDialogField::Kind
    );
}

/// Verifies that `Tab` in the cwd field starts completion and cycles matching directories.
#[test]
fn create_agent_dialog_tab_cycles_cwd_completions() {
    let root = unique_temp_dir("cwd-cycle");
    std::fs::create_dir_all(root.join("code")).unwrap();
    std::fs::create_dir_all(root.join("configs")).unwrap();

    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Agent);
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

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
    {
        let session = world.resource::<AppSessionState>();
        assert_eq!(
            session.create_agent_dialog.cwd_field.field.text,
            format!("{}/code/", root.display())
        );
        assert!(session.create_agent_dialog.cwd_field.completion.is_some());
    }

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
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
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Agent);
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

    dispatch_message_box_key(&mut world, pressed_key(KeyCode::Tab, Key::Tab));
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

/// Verifies that `Escape` cancels the create-agent dialog without spawning anything.
#[test]
fn create_agent_dialog_escape_closes_without_spawning() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(AppSessionState::default());
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(CreateAgentKind::Agent);
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
    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.create_agent_dialog.open(CreateAgentKind::Shell);
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
            label: Some("oracle".into()),
            spawn_shell_only: true,
            working_directory: "~/code".into(),
        })]
    );
}

/// Verifies that the kill-active-terminal shortcut is accepted only for plain `Ctrl+k`.
#[test]
fn kill_active_terminal_shortcut_only_uses_plain_ctrl_k() {
    let event = pressed_text(KeyCode::KeyK, Some("k"));
    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(should_kill_active_terminal(&event, &ctrl_keys));

    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(!should_kill_active_terminal(&event, &plain_keys));

    let mut alt_ctrl_keys = ButtonInput::<KeyCode>::default();
    alt_ctrl_keys.press(KeyCode::ControlLeft);
    alt_ctrl_keys.press(KeyCode::AltLeft);
    assert!(!should_kill_active_terminal(&event, &alt_ctrl_keys));
}

/// Verifies that the application-exit shortcut is accepted only for unmodified `F10`.
#[test]
fn exit_application_shortcut_only_uses_plain_f10() {
    let event = pressed_text(KeyCode::F10, None);
    let plain_keys = ButtonInput::<KeyCode>::default();
    assert!(should_exit_application(&event, &plain_keys));

    let mut ctrl_keys = ButtonInput::<KeyCode>::default();
    ctrl_keys.press(KeyCode::ControlLeft);
    assert!(!should_exit_application(&event, &ctrl_keys));

    let mut alt_keys = ButtonInput::<KeyCode>::default();
    alt_keys.press(KeyCode::AltLeft);
    assert!(!should_exit_application(&event, &alt_keys));
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
/// marks session persistence dirty.
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
        .is_some());
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
    let first_agent = catalog
        .create_agent(
            Some("agent-1".into()),
            crate::agents::AgentKind::Terminal,
            crate::agents::AgentCapabilities::terminal_defaults(),
        )
        .unwrap();
    let second_agent = catalog
        .create_agent(
            Some("agent-2".into()),
            crate::agents::AgentKind::Terminal,
            crate::agents::AgentCapabilities::terminal_defaults(),
        )
        .unwrap();
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
    let (mut world, terminal_id, input_rx) =
        world_with_active_terminal_and_receiver(Vec2::new(10.0, 10.0), false, Vec2::ZERO);
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

    assert_eq!(
        input_rx.try_recv().unwrap(),
        TerminalCommand::SendCommand("a\nb".into())
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
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world
        .run_system_once(handle_terminal_lifecycle_shortcuts)
        .unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
    assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
}

/// Verifies that global lifecycle shortcuts are ignored while direct terminal input owns keyboard
/// capture.
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
    hud_state.insert_default_module(crate::hud::HudWidgetKey::DebugToolbar);
    hud_state.set_module_shell_state(
        crate::hud::HudWidgetKey::DebugToolbar,
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
