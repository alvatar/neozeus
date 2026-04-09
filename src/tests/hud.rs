use super::{
    fake_runtime_spawner, insert_default_hud_resources, insert_terminal_manager_resources,
    insert_test_hud_state, pressed_text, snapshot_test_hud_state, temp_dir, test_bridge,
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
        task_dialog_action_buttons,
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

fn home_env_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn sqlite3_available() -> bool {
    Command::new("sqlite3")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn write_codex_state_db(path: &std::path::Path, rows: &[(&str, &str, i64, &str)]) {
    let parent = path.parent().expect("db path should have parent");
    fs::create_dir_all(parent).unwrap();
    let mut script = String::from(
        "create table threads (id text primary key, rollout_path text not null, created_at integer not null, updated_at integer not null, source text not null, model_provider text not null, cwd text not null, title text not null, sandbox_policy text not null, approval_mode text not null, tokens_used integer not null default 0, has_user_event integer not null default 0, archived integer not null default 0, archived_at integer, git_sha text, git_branch text, git_origin_url text, cli_version text not null default '', first_user_message text not null default '');\n",
    );
    for (id, cwd, created_at, title) in rows {
        script.push_str(&format!(
            "insert into threads (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode) values ('{}', '/tmp/out', {}, {}, 'chat', 'openai', '{}', '{}', '{{\"type\":\"workspace-write\"}}', 'on-request');\n",
            id, created_at, created_at, cwd, title.replace('\'', "''")
        ));
    }
    let output = Command::new("sqlite3")
        .arg(path)
        .arg(script)
        .output()
        .expect("sqlite3 should run");
    assert!(
        output.status.success(),
        "sqlite3 init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

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

/// Initializes the app-command message resource in a test world.
fn init_hud_commands(world: &mut World) {
    world.init_resource::<Messages<AppCommand>>();
}

/// Drains queued app commands from a test world.
fn drain_hud_commands(world: &mut World) -> Vec<AppCommand> {
    world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap()
}

/// Handles run app commands.
fn run_app_commands(world: &mut World) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if !world.contains_resource::<Time<()>>() {
        world.insert_resource(Time::<()>::default());
    }
    if !world.contains_resource::<Assets<Image>>() {
        world.insert_resource(Assets::<Image>::default());
    }
    if !world.contains_resource::<TerminalPresentationStore>() {
        world.insert_resource(TerminalPresentationStore::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalRuntimeSpawner>() {
        world.insert_resource(fake_runtime_spawner(Arc::new(FakeDaemonClient::default())));
    }
    if !world.contains_resource::<crate::conversations::ConversationStore>() {
        world.insert_resource(crate::conversations::ConversationStore::default());
    }
    if !world.contains_resource::<crate::conversations::AgentTaskStore>() {
        world.insert_resource(crate::conversations::AgentTaskStore::default());
    }
    if !world.contains_resource::<crate::conversations::ConversationPersistenceState>() {
        world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    }
    if !world.contains_resource::<crate::conversations::MessageTransportAdapter>() {
        world.insert_resource(crate::conversations::MessageTransportAdapter);
    }
    if !world.contains_resource::<crate::aegis::AegisPolicyStore>() {
        world.insert_resource(crate::aegis::AegisPolicyStore::default());
    }
    if !world.contains_resource::<crate::aegis::AegisRuntimeStore>() {
        world.insert_resource(crate::aegis::AegisRuntimeStore::default());
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
    if !world.contains_resource::<TerminalViewState>() {
        world.insert_resource(TerminalViewState::default());
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    crate::app::run_apply_app_commands(world);
    world
        .run_system_once(crate::conversations::sync_task_notes_projection)
        .unwrap();
}

fn clone_test_world(client: Arc<FakeDaemonClient>) -> World {
    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
}

fn write_pi_session_file(path: &std::path::Path, cwd: &str) {
    let escaped_cwd = cwd.replace('\\', "\\\\").replace('"', "\\\"");
    let content = format!(
        "{{\"type\":\"session\",\"version\":3,\"id\":\"parent-id\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"{escaped_cwd}\"}}\n{{\"type\":\"message\",\"id\":\"m1\",\"message\":{{\"role\":\"user\",\"content\":\"hello\"}}}}\n"
    );
    fs::write(path, content).expect("Pi session should write");
}

fn run_git(repo_root: &PathBuf, args: &[&str]) {
    let output = Command::new(args[0])
        .current_dir(repo_root)
        .args(&args[1..])
        .output()
        .expect("command should run");
    assert!(
        output.status.success(),
        "command {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_repo() -> PathBuf {
    let repo = PathBuf::from("/tmp").join(format!(
        "neozeus-clone-worktree-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&repo).expect("repo dir should create");
    run_git(&repo, &["git", "init"]);
    run_git(
        &repo,
        &["git", "config", "user.email", "neozeus@example.test"],
    );
    run_git(&repo, &["git", "config", "user.name", "NeoZeus Test"]);
    fs::write(repo.join("README.md"), "seed\n").unwrap();
    run_git(&repo, &["git", "add", "README.md"]);
    run_git(&repo, &["git", "commit", "-m", "initial"]);
    run_git(&repo, &["git", "branch", "-M", "main"]);
    repo
}

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

fn build_agent_list_navigation_world() -> (World, crate::agents::AgentId, crate::agents::AgentId) {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_one = manager.create_terminal(bridge_one);
    let terminal_two = manager.create_terminal(bridge_two);

    let mut catalog = AgentCatalog::default();
    let agent_one = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_two = catalog.create_agent(
        Some("beta".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_one_uid = catalog.uid(agent_one).unwrap().to_owned();
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_one, terminal_one, "alpha-session".into(), None);
    runtime_index.link_terminal(agent_two, terminal_two, "beta-session".into(), None);

    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_one));
    world
        .resource_mut::<crate::terminals::OwnedTmuxSessionStore>()
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: agent_one_uid,
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();

    (world, agent_one, agent_two)
}

fn dispatch_agent_list_nav_step(world: &mut World, event: KeyboardInput) {
    world.insert_resource(Messages::<KeyboardInput>::default());
    world.insert_resource(Messages::<AppCommand>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    world.resource_mut::<Messages<KeyboardInput>>().write(event);
    world.run_system_once(handle_hud_module_shortcuts).unwrap();
    run_app_commands(world);
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();
}

fn assert_exactly_one_selected_row(world: &World, expected_key: crate::hud::AgentListRowKey) {
    let rows = &world.resource::<crate::hud::AgentListView>().rows;
    assert_eq!(rows.iter().filter(|row| row.focused).count(), 1);
    assert!(rows
        .iter()
        .any(|row| row.focused && row.key == expected_key));
}

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

/// Verifies the fixed proportional layout of the message-box modal within the window.
#[test]
fn message_box_rect_is_top_aligned_and_shorter() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };

    let rect = message_box_rect(&window);
    assert!((rect.w - 1176.0).abs() < 0.01);
    assert!((rect.h - 468.0).abs() < 0.01);
    assert!((rect.x - 112.0).abs() < 0.01);
    assert!((rect.y - 8.0).abs() < 0.01);
}

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
    let (bridge, _, _) = super::capturing_bridge();
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
    let (bridge, _, _) = super::capturing_bridge();
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
    let (bridge, input_rx, _) = super::capturing_bridge();
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

#[test]
fn clone_agent_dialog_pointer_click_updates_focus_toggles_workdir_and_emits_command() {
    let mut world = World::default();
    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let name_rect = clone_agent_name_field_rect(&window);
    let workdir_rect = clone_agent_workdir_rect(&window);
    let clone_rect = clone_agent_submit_button_rect(&window);

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    init_hud_commands(&mut world);
    insert_default_hud_resources(&mut world);
    world.spawn((window.clone(), PrimaryWindow));

    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.clone_agent_dialog.open(
            crate::agents::AgentId(7),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
        session.clone_agent_dialog.error = Some("stale error".into());
    }

    window.set_cursor_position(Some(Vec2::new(name_rect.x + 4.0, name_rect.y + 4.0)));
    *world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .single_mut(&mut world)
        .expect("primary window should exist") = window.clone();
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();
    assert_eq!(
        world.resource::<AppSessionState>().clone_agent_dialog.focus,
        crate::app::CloneAgentDialogField::Name
    );
    assert_eq!(
        world.resource::<AppSessionState>().clone_agent_dialog.error,
        None
    );

    window.set_cursor_position(Some(Vec2::new(workdir_rect.x + 4.0, workdir_rect.y + 4.0)));
    *world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .single_mut(&mut world)
        .expect("primary window should exist") = window.clone();
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();
    assert!(
        world
            .resource::<AppSessionState>()
            .clone_agent_dialog
            .workdir
    );

    world
        .resource_mut::<AppSessionState>()
        .clone_agent_dialog
        .name_field
        .load_text("child");
    window.set_cursor_position(Some(Vec2::new(clone_rect.x + 4.0, clone_rect.y + 4.0)));
    *world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .single_mut(&mut world)
        .expect("primary window should exist") = window;
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: crate::agents::AgentId(7),
            label: "CHILD".into(),
            workdir: true,
        })]
    );
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

/// Verifies that removing a middle active terminal promotes the previous surviving terminal in
/// creation order to active/isolate state.
#[test]
fn killing_active_terminal_selects_previous_terminal_in_creation_order() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client.sessions.lock().unwrap().extend([
        "neozeus-session-a".to_owned(),
        "neozeus-session-b".to_owned(),
        "neozeus-session-c".to_owned(),
    ]);

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let (bridge_three, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    let id_three = manager.create_terminal_with_session(bridge_three, "neozeus-session-c".into());
    manager.focus_terminal(id_two);

    let mut store = TerminalPresentationStore::default();
    for id in [id_one, id_two, id_three] {
        store.register(
            id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_two),
    });
    world.insert_resource(TerminalViewState::default());
    for id in [id_one, id_two, id_three] {
        let panel_entity = world.spawn((TerminalPanel { id },)).id();
        let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    let focus = manager.clone_focus_state();
    assert_eq!(manager.terminal_ids(), &[id_one, id_three]);
    assert_eq!(focus.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_two)
    );
}

/// Verifies that removing the first active terminal promotes the next surviving terminal to
/// active/isolate state.
#[test]
fn killing_first_active_terminal_selects_next_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client.sessions.lock().unwrap().extend([
        "neozeus-session-a".to_owned(),
        "neozeus-session-b".to_owned(),
    ]);

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_one);

    let mut store = TerminalPresentationStore::default();
    for id in [id_one, id_two] {
        store.register(
            id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_one),
    });
    world.insert_resource(TerminalViewState::default());
    for id in [id_one, id_two] {
        let panel_entity = world.spawn((TerminalPanel { id },)).id();
        let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    let focus = manager.clone_focus_state();
    assert_eq!(manager.terminal_ids(), &[id_two]);
    assert_eq!(focus.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_one)
    );
}

/// Verifies that a successful active-terminal kill removes terminal-manager state, presentation
/// state, labels, spawned panel entities, and resets visibility/persistence bookkeeping.
#[test]
fn killing_active_terminal_removes_runtime_presentation_and_labels() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_some());
    assert!(client.sessions.lock().unwrap().is_empty());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}

/// Verifies that duplicate agent names are rejected before any daemon session is created.
#[test]
fn create_agent_rejects_duplicate_name_without_creating_session() {
    let client = std::sync::Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.init_resource::<Messages<AppCommand>>();
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    let mut catalog = crate::agents::AgentCatalog::default();
    catalog.create_agent(
        Some("oracle".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentCapabilities::terminal_defaults(),
    );
    world.insert_resource(catalog);
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(crate::app::CreateAgentKind::Pi);
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Create {
            label: Some("oracle".into()),
            kind: crate::agents::AgentKind::Pi,
            working_directory: "~/code".into(),
        }));

    run_app_commands(&mut world);

    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 0);
    assert!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .visible
    );
    assert_eq!(
        world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .error
            .as_deref(),
        Some("agent `ORACLE` already exists")
    );
    assert!(client.created_sessions.lock().unwrap().is_empty());
}

/// Verifies that creating agent sessions bootstraps the selected CLI command.
#[test]
fn create_agent_request_bootstraps_selected_cli_command() {
    if !sqlite3_available() {
        return;
    }
    let _home_lock = home_env_test_lock().lock().unwrap();
    let previous_home = std::env::var_os("HOME");
    let codex_home = temp_dir("neozeus-codex-create-test-home");
    std::env::set_var("HOME", &codex_home);
    write_codex_state_db(
        &codex_home.join(".codex").join("state_5.sqlite"),
        &[("thread-old", "/tmp/other", 10, "old")],
    );
    std::thread::spawn({
        let codex_home = codex_home.clone();
        move || {
            std::thread::sleep(Duration::from_millis(150));
            write_codex_state_db(
                &codex_home.join(".codex").join("state_6.sqlite"),
                &[
                    ("thread-old", "/tmp/other", 10, "old"),
                    (
                        "thread-new",
                        codex_home.join("code").to_string_lossy().as_ref(),
                        20,
                        "new",
                    ),
                ],
            );
        }
    });

    let client = Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    for (label, kind) in [
        ("pi-agent", crate::agents::AgentKind::Pi),
        ("claude-agent", crate::agents::AgentKind::Claude),
        ("codex-agent", crate::agents::AgentKind::Codex),
    ] {
        world
            .resource_mut::<Messages<AppCommand>>()
            .write(AppCommand::Agent(AppAgentCommand::Create {
                label: Some(label.into()),
                kind,
                working_directory: "~/code".into(),
            }));
    }

    run_app_commands(&mut world);

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 3);
    for (_, _, env_overrides) in &created_sessions {
        assert!(env_overrides
            .iter()
            .any(|(key, _)| key == "NEOZEUS_AGENT_UID"));
        assert!(env_overrides
            .iter()
            .any(|(key, _)| key == "NEOZEUS_AGENT_LABEL"));
        assert!(env_overrides
            .iter()
            .any(|(key, _)| key == "NEOZEUS_AGENT_KIND"));
    }

    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 3);
    assert!(commands.iter().any(|(_, command)| {
        matches!(command, crate::terminals::TerminalCommand::SendCommand(value) if value.starts_with("pi --session "))
    }));
    assert!(commands.iter().any(|(_, command)| {
        matches!(command, crate::terminals::TerminalCommand::SendCommand(value) if value.starts_with("claude --session-id "))
    }));
    assert!(commands.iter().any(|(_, command)| {
        matches!(command, crate::terminals::TerminalCommand::SendCommand(value) if value == "codex")
    }));

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let pi_agent = catalog
        .iter()
        .find_map(|(agent_id, label)| (label == "PI-AGENT").then_some(agent_id))
        .expect("Pi agent should exist");
    let session_path = catalog
        .clone_source_session_path(pi_agent)
        .expect("Pi agent should persist clone provenance");
    assert!(session_path.ends_with(".jsonl"));
    assert!(!catalog.is_workdir(pi_agent));
    let claude_agent = catalog
        .iter()
        .find_map(|(agent_id, label)| (label == "CLAUDE-AGENT").then_some(agent_id))
        .expect("Claude agent should exist");
    assert!(matches!(
        catalog.recovery_spec(claude_agent),
        Some(crate::agents::AgentRecoverySpec::Claude { session_id, cwd, .. })
            if !session_id.trim().is_empty() && cwd.ends_with("/code")
    ));
    let codex_agent = catalog
        .iter()
        .find_map(|(agent_id, label)| (label == "CODEX-AGENT").then_some(agent_id))
        .expect("Codex agent should exist");
    assert!(matches!(
        catalog.recovery_spec(codex_agent),
        Some(crate::agents::AgentRecoverySpec::Codex { session_id, cwd, .. })
            if session_id == "thread-new"
                && cwd == codex_home.join("code").to_string_lossy().as_ref()
    ));
    if let Some(previous_home) = previous_home {
        std::env::set_var("HOME", previous_home);
    } else {
        std::env::remove_var("HOME");
    }
}

/// Verifies that creating a terminal session does not inject any agent bootstrap command payload.
#[test]
fn create_terminal_agent_request_does_not_send_bootstrap_command() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    world
        .resource_mut::<AppSessionState>()
        .create_agent_dialog
        .open(crate::app::CreateAgentKind::Terminal);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(AgentListView::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Create {
            label: Some("shell".into()),
            kind: crate::agents::AgentKind::Terminal,
            working_directory: "~/code".into(),
        }));

    run_app_commands(&mut world);

    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
    assert!(
        !world
            .resource::<AppSessionState>()
            .create_agent_dialog
            .visible
    );
    assert!(client.sent_commands.lock().unwrap().is_empty());
    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].0, "neozeus-session-0");
    assert_eq!(created_sessions[0].1.as_deref(), Some("~/code"));
    assert!(created_sessions[0]
        .2
        .iter()
        .any(|(key, value)| key == "NEOZEUS_AGENT_LABEL" && value == "SHELL"));
    assert!(created_sessions[0]
        .2
        .iter()
        .any(|(key, value)| key == "NEOZEUS_AGENT_KIND" && value == "terminal"));
    assert!(created_sessions[0]
        .2
        .iter()
        .any(|(key, _)| key == "NEOZEUS_AGENT_UID"));
}

#[test]
fn clone_claude_agent_request_forks_and_persists_child_recovery_spec() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Claude,
            crate::agents::AgentKind::Claude.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: None,
                recovery: Some(crate::agents::AgentRecoverySpec::Claude {
                    session_id: "claude-parent".into(),
                    cwd: "/tmp/claude-demo".into(),
                    model: Some("sonnet".into()),
                    profile: None,
                }),
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/claude-demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value.starts_with("claude --resume claude-parent --fork-session --session-id ")
    ));
    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().expect("Claude child should exist");
    assert!(matches!(
        catalog.recovery_spec(clone_agent),
        Some(crate::agents::AgentRecoverySpec::Claude { session_id, cwd, model, .. })
            if session_id != "claude-parent"
                && cwd == "/tmp/claude-demo"
                && model.as_deref() == Some("sonnet")
    ));
}

#[test]
fn clone_codex_agent_request_forks_and_captures_child_recovery_spec() {
    if !sqlite3_available() {
        return;
    }
    let _home_lock = home_env_test_lock().lock().unwrap();
    let previous_home = std::env::var_os("HOME");
    let codex_home = temp_dir("neozeus-codex-clone-test-home");
    std::env::set_var("HOME", &codex_home);
    write_codex_state_db(
        &codex_home.join(".codex").join("state_5.sqlite"),
        &[("thread-old", "/tmp/other", 10, "old")],
    );
    std::thread::spawn({
        let codex_home = codex_home.clone();
        move || {
            std::thread::sleep(Duration::from_millis(150));
            write_codex_state_db(
                &codex_home.join(".codex").join("state_6.sqlite"),
                &[
                    ("thread-old", "/tmp/other", 10, "old"),
                    ("thread-child", "/tmp/codex-demo", 20, "child"),
                ],
            );
        }
    });

    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Codex,
            crate::agents::AgentKind::Codex.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: None,
                recovery: Some(crate::agents::AgentRecoverySpec::Codex {
                    session_id: "codex-parent".into(),
                    cwd: "/tmp/codex-demo".into(),
                    model: None,
                    profile: None,
                }),
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/codex-demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "codex fork codex-parent -C /tmp/codex-demo"
    ));
    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().expect("Codex child should exist");
    assert!(matches!(
        catalog.recovery_spec(clone_agent),
        Some(crate::agents::AgentRecoverySpec::Codex { session_id, cwd, .. })
            if session_id == "thread-child" && cwd == "/tmp/codex-demo"
    ));
    if let Some(previous_home) = previous_home {
        std::env::set_var("HOME", previous_home);
    } else {
        std::env::remove_var("HOME");
    }
}

#[test]
fn clone_agent_request_rejects_non_pi_source() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_agent = world.resource_mut::<AgentCatalog>().create_agent(
        Some("source".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert_eq!(world.resource::<AgentCatalog>().order.len(), 1);
    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 0);
}

#[test]
fn clone_agent_request_rejects_missing_clone_provenance() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_agent = world.resource_mut::<AgentCatalog>().create_agent(
        Some("source".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert_eq!(world.resource::<AgentCatalog>().order.len(), 1);
}

#[test]
fn clone_agent_request_rejects_duplicate_name() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_dir = temp_dir("clone-pi-duplicate-source");
    let source_session = source_dir.join("source.jsonl");
    write_pi_session_file(&source_session, "/tmp/clone-pi-duplicate-cwd");
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );
    world.resource_mut::<AgentCatalog>().create_agent(
        Some("child".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert_eq!(world.resource::<AgentCatalog>().order.len(), 2);
}

#[test]
fn clone_agent_request_creates_top_level_pi_clone_and_focuses_it() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_dir = temp_dir("clone-pi-source");
    let source_session = source_dir.join("source.jsonl");
    let source_cwd = "/tmp/clone-pi-cwd";
    write_pi_session_file(&source_session, source_cwd);
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    let catalog = world.resource::<AgentCatalog>();
    assert_eq!(catalog.order.len(), 2);
    let clone_agent = *catalog.order.last().expect("clone agent should exist");
    assert_eq!(catalog.label(clone_agent), Some("CHILD"));
    assert_eq!(
        catalog.kind(clone_agent),
        Some(crate::agents::AgentKind::Pi)
    );
    let clone_session_path = catalog
        .clone_source_session_path(clone_agent)
        .expect("clone should persist forked Pi session path")
        .to_owned();
    assert!(!catalog.is_workdir(clone_agent));

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some(source_cwd));
    let sent_commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(sent_commands.len(), 1);
    assert!(matches!(
        &sent_commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value.contains(&clone_session_path)
    ));
    let clone_header = crate::shared::pi_session_files::read_session_header(&clone_session_path)
        .expect("forked Pi session should read");
    assert_eq!(clone_header.cwd, source_cwd);
    assert_eq!(
        clone_header.parent_session.as_deref(),
        Some(source_session.to_string_lossy().as_ref())
    );
    let _ = catalog;

    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(clone_agent)
    );
    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
}

#[test]
fn clone_agent_request_creates_workdir_clone_and_persists_metadata() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let repo = init_git_repo();
    let source_session = repo.join("source.jsonl");
    write_pi_session_file(&source_session, repo.to_str().unwrap());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );
    let app_state_path = temp_dir("clone-pi-workdir-appstate").join("neozeus-state.v1");
    world.insert_resource(AppStatePersistenceState {
        path: Some(app_state_path.clone()),
        dirty_since_secs: None,
    });

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child-wt".into(),
            workdir: true,
        }));

    run_app_commands(&mut world);

    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().expect("workdir clone should exist");
    assert_eq!(catalog.label(clone_agent), Some("CHILD-WT"));
    assert!(catalog.is_workdir(clone_agent));
    assert_eq!(catalog.workdir_slug(clone_agent), Some("CHILD-WT"));
    let clone_session_path = catalog
        .clone_source_session_path(clone_agent)
        .expect("workdir clone should persist forked Pi session path")
        .to_owned();
    let clone_header = crate::shared::pi_session_files::read_session_header(&clone_session_path)
        .expect("workdir clone session should read");
    let expected_worktree = repo.join(".worktrees").join("CHILD-WT");
    assert_eq!(PathBuf::from(&clone_header.cwd), expected_worktree);
    assert!(expected_worktree.is_dir());
    let _ = catalog;

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), expected_worktree.to_str());

    world
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_secs(1));
    world
        .run_system_once(crate::app::save_app_state_if_dirty)
        .unwrap();
    let persisted = crate::shared::app_state_file::parse_persisted_app_state(
        &fs::read_to_string(app_state_path).expect("app state should persist"),
    );
    let persisted_clone = persisted
        .agents
        .iter()
        .find(|record| record.label.as_deref() == Some("CHILD-WT"))
        .expect("persisted workdir clone should exist");
    assert_eq!(persisted_clone.clone_source_session_path, None);
    assert!(matches!(
        &persisted_clone.recovery,
        Some(crate::shared::app_state_file::PersistedAgentRecoverySpec::Pi {
            session_path,
            is_workdir: true,
            workdir_slug: Some(slug),
            ..
        }) if session_path == &clone_session_path && slug == "CHILD-WT"
    ));
}

#[test]
fn clone_agent_request_sanitizes_workdir_slug_without_changing_display_label() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let repo = init_git_repo();
    let source_session = repo.join("source.jsonl");
    write_pi_session_file(&source_session, repo.to_str().unwrap());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child wt/1".into(),
            workdir: true,
        }));

    run_app_commands(&mut world);

    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().expect("workdir clone should exist");
    assert_eq!(catalog.label(clone_agent), Some("CHILD WT/1"));
    assert_eq!(catalog.workdir_slug(clone_agent), Some("CHILD-WT-1"));
    let clone_session_path = catalog
        .clone_source_session_path(clone_agent)
        .expect("workdir clone should persist session path")
        .to_owned();
    let clone_header = crate::shared::pi_session_files::read_session_header(&clone_session_path)
        .expect("workdir clone session should read");
    assert_eq!(
        PathBuf::from(&clone_header.cwd),
        repo.join(".worktrees").join("CHILD-WT-1")
    );
}

#[test]
fn clone_agent_request_rejects_non_git_workdir_source() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_dir = temp_dir("clone-pi-non-git-source");
    let source_session = source_dir.join("source.jsonl");
    let non_git_cwd = PathBuf::from("/tmp").join(format!(
        "neozeus-clone-non-git-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&non_git_cwd).expect("non-git cwd should create");
    write_pi_session_file(&source_session, non_git_cwd.to_str().unwrap());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child-wt".into(),
            workdir: true,
        }));

    run_app_commands(&mut world);

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert_eq!(world.resource::<AgentCatalog>().order.len(), 1);
}

/// Verifies the special-case cleanup path for disconnected terminals: local state is removed even if
/// daemon-side kill returns an error.
#[test]
fn killing_disconnected_active_terminal_removes_local_state_even_if_daemon_kill_fails() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager
        .get_mut(id)
        .expect("missing terminal")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_some());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}

/// Verifies the stale-snapshot cleanup path: if the local terminal still looks interactive but the
/// daemon already reports the session as disconnected, one kill still removes the local terminal.
#[test]
fn killing_active_terminal_removes_local_state_when_daemon_already_reports_disconnected() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager
        .get_mut(id)
        .expect("missing terminal")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::running("stale local snapshot");
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_some());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}

/// Verifies that a kill failure for an otherwise live terminal preserves all local state instead of
/// tearing presentation/labels down prematurely.
#[test]
fn killing_active_terminal_preserves_local_state_when_tmux_kill_fails() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut session_persistence: ResMut<AppStatePersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    assert_eq!(world.resource::<TerminalManager>().terminal_ids(), &[id]);
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_some());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<AppStatePersistenceState>()
        .dirty_since_secs
        .is_none());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 1);
    assert_eq!(frame_count, 1);
}

/// Verifies that killing an agent also cascade-kills any owned tmux child sessions.
#[test]
fn killing_agent_cascade_kills_owned_tmux_children() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(terminal_id);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "neozeus-session-a".into(), None);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));

    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: agent_uid.clone(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    assert!(world
        .resource_mut::<crate::aegis::AegisPolicyStore>()
        .enable(&agent_uid, "continue cleanly".into()));
    assert!(world
        .resource_mut::<crate::aegis::AegisRuntimeStore>()
        .set_state(
            agent_id,
            crate::aegis::AegisRuntimeState::PendingDelay { deadline_secs: 6.0 }
        ));
    assert!(world
        .resource_mut::<crate::conversations::AgentTaskStore>()
        .set_text(agent_id, "- [ ] task"));
    let conversation_id = world
        .resource_mut::<crate::conversations::ConversationStore>()
        .ensure_conversation(agent_id);
    let _ = world
        .resource_mut::<crate::conversations::ConversationStore>()
        .push_message(
            conversation_id,
            crate::conversations::MessageAuthor::User,
            "hello".into(),
            crate::conversations::MessageDeliveryState::Delivered,
        );
    assert!(world
        .resource_mut::<crate::terminals::TerminalNotesState>()
        .set_note_text_by_agent_uid(&agent_uid, "- [ ] task"));

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
    run_app_commands(&mut world);

    assert!(world.resource::<AgentCatalog>().order.is_empty());
    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(client.owned_tmux_sessions.lock().unwrap().is_empty());
    assert_eq!(
        world
            .resource::<crate::conversations::AgentTaskStore>()
            .text(agent_id),
        None
    );
    assert!(world
        .resource::<crate::conversations::ConversationStore>()
        .conversation_for_agent(agent_id)
        .is_none());
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalNotesState>()
            .note_text_by_agent_uid(&agent_uid),
        None
    );
    assert_eq!(
        world
            .resource::<crate::conversations::ConversationPersistenceState>()
            .dirty_since_secs,
        Some(1.0)
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalNotesState>()
            .dirty_since_secs,
        Some(1.0)
    );
    assert!(world
        .resource::<crate::aegis::AegisPolicyStore>()
        .policy(&agent_uid)
        .is_none());
    assert!(world
        .resource::<crate::aegis::AegisRuntimeStore>()
        .state(agent_id)
        .is_none());
}

/// Verifies that parent deletion cascades only the active agent's owned tmux children and leaves
/// other agents plus orphan rows untouched.
#[test]
fn killing_agent_cascade_kills_only_selected_agent_owned_tmux_children() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .extend(["neozeus-session-a".into(), "neozeus-session-b".into()]);

    let (bridge_a, _) = test_bridge();
    let (bridge_b, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_a = manager.create_terminal_with_session(bridge_a, "neozeus-session-a".into());
    let terminal_b = manager.create_terminal_with_session(bridge_b, "neozeus-session-b".into());
    manager.focus_terminal(terminal_a);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut catalog = AgentCatalog::default();
    let agent_a = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_b = catalog.create_agent(
        Some("beta".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_a_uid = catalog.uid(agent_a).unwrap().to_owned();
    let agent_b_uid = catalog.uid(agent_b).unwrap().to_owned();
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_a, terminal_a, "neozeus-session-a".into(), None);
    runtime_index.link_terminal(agent_b, terminal_b, "neozeus-session-b".into(), None);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_a));

    client.owned_tmux_sessions.lock().unwrap().extend([
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a1".into(),
            owner_agent_uid: agent_a_uid.clone(),
            tmux_name: "neozeus-tmux-a1".into(),
            display_name: "A-1".into(),
            cwd: "/tmp/a1".into(),
            attached: false,
            created_unix: 1,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-a2".into(),
            owner_agent_uid: agent_a_uid,
            tmux_name: "neozeus-tmux-a2".into(),
            display_name: "A-2".into(),
            cwd: "/tmp/a2".into(),
            attached: false,
            created_unix: 2,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-b1".into(),
            owner_agent_uid: agent_b_uid,
            tmux_name: "neozeus-tmux-b1".into(),
            display_name: "B-1".into(),
            cwd: "/tmp/b1".into(),
            attached: false,
            created_unix: 3,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-orphan".into(),
            owner_agent_uid: "missing-agent".into(),
            tmux_name: "neozeus-tmux-orphan".into(),
            display_name: "ORPHAN".into(),
            cwd: "/tmp/orphan".into(),
            attached: false,
            created_unix: 4,
        },
    ]);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
    run_app_commands(&mut world);

    let remaining = client.owned_tmux_sessions.lock().unwrap().clone();
    assert_eq!(remaining.len(), 2);
    assert!(remaining
        .iter()
        .any(|session| session.session_uid == "tmux-b1"));
    assert!(remaining
        .iter()
        .any(|session| session.session_uid == "tmux-orphan"));
    assert!(!remaining
        .iter()
        .any(|session| session.session_uid == "tmux-a1"));
    assert!(!remaining
        .iter()
        .any(|session| session.session_uid == "tmux-a2"));
    assert_eq!(world.resource::<AgentCatalog>().order, vec![agent_b]);
    assert_eq!(
        world.resource::<TerminalManager>().terminal_ids(),
        &[terminal_b]
    );
}

/// Verifies that parent deletion aborts when owned tmux child cleanup fails and the child still exists.
#[test]
fn killing_agent_aborts_when_owned_tmux_child_kill_fails() {
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_owned_tmux_kill.lock().unwrap() = true;
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(terminal_id);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "neozeus-session-a".into(), None);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));

    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: agent_uid,
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
    run_app_commands(&mut world);

    assert!(!world.resource::<AgentCatalog>().order.is_empty());
    assert_eq!(
        world.resource::<TerminalManager>().terminal_ids(),
        &[terminal_id]
    );
    assert_eq!(client.owned_tmux_sessions.lock().unwrap().len(), 1);
}

#[test]
fn killing_selected_agent_targets_selected_agent_even_when_focus_differs() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .extend(["neozeus-session-a".into(), "neozeus-session-b".into()]);

    let (bridge_a, _) = test_bridge();
    let (bridge_b, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_a = manager.create_terminal_with_session(bridge_a, "neozeus-session-a".into());
    let terminal_b = manager.create_terminal_with_session(bridge_b, "neozeus-session-b".into());
    manager.focus_terminal(terminal_a);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut catalog = AgentCatalog::default();
    let agent_a = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_b = catalog.create_agent(
        Some("beta".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_a, terminal_a, "neozeus-session-a".into(), None);
    runtime_index.link_terminal(agent_b, terminal_b, "neozeus-session-b".into(), None);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_b));

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
    run_app_commands(&mut world);

    assert_eq!(world.resource::<AgentCatalog>().order, vec![agent_a]);
    assert_eq!(
        world.resource::<TerminalManager>().terminal_ids(),
        &[terminal_a]
    );
    assert!(client
        .sessions
        .lock()
        .unwrap()
        .contains("neozeus-session-a"));
    assert!(!client
        .sessions
        .lock()
        .unwrap()
        .contains("neozeus-session-b"));
}

#[test]
fn composer_submit_marks_message_failed_when_daemon_send_fails() {
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_send.lock().unwrap() = true;
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "neozeus-session-a".into(), None);
    world.insert_resource(runtime_index);

    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.composer.open_message(agent_id);
        app_session.composer.message_editor.load_text("status");
    }

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Composer(AppComposerCommand::Submit));
    run_app_commands(&mut world);

    let conversations = world.resource::<crate::conversations::ConversationStore>();
    let conversation_id = conversations
        .conversation_for_agent(agent_id)
        .expect("conversation should exist");
    let messages = conversations.messages_for(conversation_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "status");
    assert_eq!(
        messages[0].1,
        crate::conversations::MessageDeliveryState::Failed("send failed".into())
    );
}

/// Verifies that explicit owned tmux kill clears selection after successful child deletion.
#[test]
fn killing_selected_owned_tmux_session_clears_selection_on_success() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), None);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::KillSelected,
        ));
    run_app_commands(&mut world);

    assert!(world
        .resource::<crate::terminals::ActiveTerminalContentState>()
        .selected_owned_tmux_session_uid()
        .is_none());
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::None
    );
    assert!(client.owned_tmux_sessions.lock().unwrap().is_empty());
}

/// Verifies that a successful owned tmux kill removes the child row from the derived agent list immediately.
#[test]
fn killing_selected_owned_tmux_session_removes_agent_list_row_immediately() {
    let client = Arc::new(FakeDaemonClient::default());

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();

    let session = crate::terminals::OwnedTmuxSessionInfo {
        session_uid: "tmux-session-1".into(),
        owner_agent_uid: agent_uid.clone(),
        tmux_name: "neozeus-tmux-1".into(),
        display_name: "BUILD".into(),
        cwd: "/tmp/work".into(),
        attached: false,
        created_unix: 0,
    };
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(session.clone());

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    let mut owned_tmux_sessions = crate::terminals::OwnedTmuxSessionStore::default();
    owned_tmux_sessions.sessions.push(session);
    world.insert_resource(owned_tmux_sessions);
    world.insert_resource(crate::hud::AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), None);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::KillSelected,
        ));
    run_app_commands(&mut world);
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();

    let rows = &world.resource::<crate::hud::AgentListView>().rows;
    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows[0].key,
        crate::hud::AgentListRowKey::Agent(found_agent_id) if found_agent_id == agent_id
    ));
    assert!(client.owned_tmux_sessions.lock().unwrap().is_empty());
}

/// Verifies that an already-gone owned tmux child clears selection after daemon recheck.
#[test]
fn killing_selected_owned_tmux_session_treats_missing_child_as_success() {
    let client = Arc::new(FakeDaemonClient::default());

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<crate::terminals::OwnedTmuxSessionStore>()
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), None);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::KillSelected,
        ));
    run_app_commands(&mut world);

    assert!(world
        .resource::<crate::terminals::ActiveTerminalContentState>()
        .selected_owned_tmux_session_uid()
        .is_none());
}

/// Verifies that owned tmux kill refreshes the cached session store from daemon truth when the local
/// store is stale.
#[test]
fn killing_selected_owned_tmux_session_refreshes_stale_store_from_daemon_truth() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-live".into(),
            owner_agent_uid: "agent-uid-2".into(),
            tmux_name: "neozeus-tmux-live".into(),
            display_name: "LIVE".into(),
            cwd: "/tmp/live".into(),
            attached: false,
            created_unix: 1,
        });

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    let mut stale_store = crate::terminals::OwnedTmuxSessionStore::default();
    stale_store.sessions.extend([
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-stale-selected".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-stale-selected".into(),
            display_name: "STALE".into(),
            cwd: "/tmp/stale".into(),
            attached: false,
            created_unix: 0,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-stale-other".into(),
            owner_agent_uid: "agent-uid-3".into(),
            tmux_name: "neozeus-tmux-stale-other".into(),
            display_name: "STALE-OTHER".into(),
            cwd: "/tmp/stale-other".into(),
            attached: false,
            created_unix: 2,
        },
    ]);
    world.insert_resource(stale_store);
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-stale-selected".into(), None);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::KillSelected,
        ));
    run_app_commands(&mut world);

    assert!(world
        .resource::<crate::terminals::ActiveTerminalContentState>()
        .selected_owned_tmux_session_uid()
        .is_none());
    let store = world.resource::<crate::terminals::OwnedTmuxSessionStore>();
    assert_eq!(store.sessions.len(), 1);
    assert_eq!(store.sessions[0].session_uid, "tmux-live");
}

/// Verifies that a real owned tmux kill failure preserves selection and surfaces the error.
#[test]
fn killing_selected_owned_tmux_session_preserves_selection_on_failure() {
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_owned_tmux_kill.lock().unwrap() = true;
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), None);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::KillSelected,
        ));
    run_app_commands(&mut world);

    let inspect = world.resource::<crate::terminals::ActiveTerminalContentState>();
    assert_eq!(
        inspect.selected_owned_tmux_session_uid(),
        Some("tmux-session-1")
    );
    assert_eq!(inspect.last_error(), Some("owned tmux kill failed"));
}

/// Verifies that navigating onto an owned tmux row renders the tmux capture in the main terminal panel.
#[test]
fn navigating_to_owned_tmux_should_render_capture_in_terminal_panel() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: true,
            created_unix: 0,
        });
    client
        .tmux_captures
        .lock()
        .unwrap()
        .insert("tmux-session-1".into(), "TMUX VERIFY\nline two\n".into());

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    client.owned_tmux_sessions.lock().unwrap()[0].owner_agent_uid = agent_uid;

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    manager.focus_terminal(terminal_id);
    let terminal = manager.get_mut(terminal_id).expect("terminal should exist");
    terminal.snapshot.surface = Some(crate::terminals::TerminalSurface::new(80, 24));
    terminal.surface_revision = 1;

    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "agent-session".into(), None);

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(TerminalFontState::default());
    world.insert_resource(TerminalTextRenderer::default());
    world.insert_resource(TerminalGlyphCache::default());
    world.insert_resource(Assets::<Image>::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world.insert_resource(crate::hud::AgentListView {
        rows: vec![
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(agent_id),
                label: "ALPHA".into(),
                focused: true,
                kind: crate::hud::AgentListRowKind::Agent {
                    agent_id,
                    terminal_id: Some(terminal_id),
                    has_tasks: false,
                    interactive: true,
                    activity: crate::hud::AgentListActivity::Idle,
                    context_pct_milli: None,
                },
            },
            crate::hud::AgentListRowView {
                key: crate::hud::AgentListRowKey::OwnedTmux("tmux-session-1".into()),
                label: "BUILD".into(),
                focused: false,
                kind: crate::hud::AgentListRowKind::OwnedTmux {
                    session_uid: "tmux-session-1".into(),
                    owner: crate::hud::OwnedTmuxOwnerBinding::Bound(agent_id),
                    tmux_name: "neozeus-tmux-1".into(),
                    cwd: "/tmp/work".into(),
                    attached: true,
                },
            },
        ],
    });
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.init_resource::<Messages<KeyboardInput>>();
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_key(KeyCode::KeyJ, Key::Character("j".into())));
    world.run_system_once(handle_hud_module_shortcuts).unwrap();
    run_app_commands(&mut world);
    world
        .run_system_once(crate::terminals::sync_owned_tmux_sessions)
        .unwrap();
    world
        .run_system_once(crate::terminals::sync_active_terminal_content)
        .unwrap();
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();
    world
        .run_system_once(crate::terminals::configure_terminal_fonts)
        .unwrap();
    world
        .run_system_once(crate::terminals::sync_terminal_texture)
        .unwrap();

    let store = world.resource::<TerminalPresentationStore>();
    let presented = store
        .get(terminal_id)
        .expect("presented terminal should exist");
    let images = world.resource::<Assets<Image>>();
    let image = images
        .get(&presented.image)
        .expect("terminal image should exist");
    let has_visible_tmux_pixels = image
        .data
        .as_ref()
        .expect("image data should exist")
        .chunks_exact(4)
        .any(|pixel| {
            pixel
                != [
                    DEFAULT_BG.r(),
                    DEFAULT_BG.g(),
                    DEFAULT_BG.b(),
                    DEFAULT_BG.a(),
                ]
        });

    assert!(
        has_visible_tmux_pixels,
        "navigating to selected tmux should render into the main terminal panel"
    );
}

/// Verifies that syncing owned tmux sessions wakes the renderer when startup or polling discovers new children.
#[test]
fn syncing_owned_tmux_sessions_requests_redraw_on_change_only() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });

    let mut world = World::default();
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .run_system_once(crate::terminals::sync_owned_tmux_sessions)
        .unwrap();
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    assert_eq!(
        world
            .resource::<crate::terminals::OwnedTmuxSessionStore>()
            .sessions
            .len(),
        1
    );

    world.clear_trackers();
    world
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_secs(1));
    world
        .run_system_once(crate::terminals::sync_owned_tmux_sessions)
        .unwrap();
    assert_eq!(
        world.resource::<Messages<RequestRedraw>>().len(),
        1,
        "unchanged tmux discovery should not spam redraws"
    );
}

/// Verifies that active terminal override state reports disappearance instead of rendering stale tmux content.
#[test]
fn active_terminal_content_reports_missing_selected_tmux_session() {
    let client = Arc::new(FakeDaemonClient::default());

    let mut world = World::default();
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);

    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-missing".into(), None);
    world
        .run_system_once(crate::terminals::sync_active_terminal_content)
        .unwrap();

    let active_terminal_content = world.resource::<crate::terminals::ActiveTerminalContentState>();
    assert_eq!(
        active_terminal_content.last_error(),
        Some("Owned tmux session is no longer available")
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that identical tmux recaptures do not mark the active terminal content dirty again.
#[test]
fn active_terminal_content_ignores_identical_recapture() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    client
        .tmux_captures
        .lock()
        .unwrap()
        .insert("tmux-session-1".into(), "same\ncontent\n".into());

    let mut world = World::default();
    world.insert_resource(fake_runtime_spawner(client));
    let mut owned_tmux_sessions = crate::terminals::OwnedTmuxSessionStore::default();
    owned_tmux_sessions
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    world.insert_resource(owned_tmux_sessions);
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);

    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), None);
    world
        .run_system_once(crate::terminals::sync_active_terminal_content)
        .unwrap();
    let revision_after_first_sync = world
        .resource::<crate::terminals::ActiveTerminalContentState>()
        .presentation_revision();
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);

    world.clear_trackers();
    world
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_secs(1));
    world
        .run_system_once(crate::terminals::sync_active_terminal_content)
        .unwrap();

    let revision_after_second_sync = world
        .resource::<crate::terminals::ActiveTerminalContentState>()
        .presentation_revision();
    assert_eq!(
        revision_after_second_sync, revision_after_first_sync,
        "identical tmux recapture should not bump the terminal presentation revision"
    );
    assert_eq!(
        world.resource::<Messages<RequestRedraw>>().len(),
        1,
        "identical tmux recapture should not spam redraws"
    );
}

#[test]
fn selecting_tmux_row_sets_tmux_terminal_override_without_changing_selected_row_kind() {
    let client = Arc::new(FakeDaemonClient::default());
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "alpha-session".into(), None);

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(manager);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world
        .resource_mut::<crate::terminals::OwnedTmuxSessionStore>()
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: agent_uid,
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::Select {
                session_uid: "tmux-session-1".into(),
            },
        ));
    run_app_commands(&mut world);

    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
    assert_eq!(
        world
            .resource::<crate::terminals::ActiveTerminalContentState>()
            .selected_owned_tmux_session_uid(),
        Some("tmux-session-1")
    );
}

#[test]
fn selecting_tmux_row_sets_parent_agent_thread_target_explicitly() {
    let client = Arc::new(FakeDaemonClient::default());
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "alpha-session".into(), None);

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(manager);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world
        .resource_mut::<crate::terminals::OwnedTmuxSessionStore>()
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: agent_uid,
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::OwnedTmux(
            crate::app::OwnedTmuxCommand::Select {
                session_uid: "tmux-session-1".into(),
            },
        ));
    run_app_commands(&mut world);

    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
}

#[test]
fn terminal_focus_sync_does_not_rewrite_agent_list_selection() {
    let (bridge_a, _) = test_bridge();
    let (bridge_b, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_a = manager.create_terminal(bridge_a);
    let terminal_b = manager.create_terminal(bridge_b);
    manager.focus_terminal(terminal_b);

    let mut catalog = AgentCatalog::default();
    let agent_a = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_b = catalog.create_agent(
        Some("beta".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_a, terminal_a, "alpha-session".into(), None);
    runtime_index.link_terminal(agent_b, terminal_b, "beta-session".into(), None);

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(crate::hud::AgentListSelection::OwnedTmux(
        "tmux-session-1".into(),
    ));
    world.insert_resource(manager.clone_focus_state());
    world.insert_resource(manager);

    world
        .run_system_once(crate::app::sync_agents_from_terminals)
        .unwrap();

    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::OwnedTmux("tmux-session-1".into())
    );
}

#[test]
fn sync_agents_from_terminals_cleans_tasks_conversations_notes_and_persistence_for_disappeared_agent(
) {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let agent_uid = catalog.uid(agent_id).unwrap().to_owned();
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "alpha-session".into(), None);
    let _ = manager.remove_terminal(terminal_id);

    let mut task_store = crate::conversations::AgentTaskStore::default();
    assert!(task_store.set_text(agent_id, "- [ ] task"));
    let mut conversations = crate::conversations::ConversationStore::default();
    let conversation_id = conversations.ensure_conversation(agent_id);
    let _ = conversations.push_message(
        conversation_id,
        crate::conversations::MessageAuthor::User,
        "hello".into(),
        crate::conversations::MessageDeliveryState::Delivered,
    );
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    assert!(notes_state.set_note_text_by_agent_uid(&agent_uid, "- [ ] task"));
    let mut aegis_policy = crate::aegis::AegisPolicyStore::default();
    assert!(aegis_policy.enable(&agent_uid, "continue cleanly".into()));
    let mut aegis_runtime = crate::aegis::AegisRuntimeStore::default();
    assert!(aegis_runtime.set_state(
        agent_id,
        crate::aegis::AegisRuntimeState::PostCheck {
            deadline_secs: 20.0
        }
    ));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(aegis_policy);
    world.insert_resource(aegis_runtime);
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world.insert_resource(task_store);
    world.insert_resource(conversations);
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(notes_state);
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(manager);

    world
        .run_system_once(crate::app::sync_agents_from_terminals)
        .unwrap();

    assert!(world.resource::<AgentCatalog>().order.is_empty());
    assert!(world
        .resource::<AgentRuntimeIndex>()
        .session_to_agent
        .is_empty());
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::None
    );
    assert_eq!(
        world
            .resource::<crate::conversations::AgentTaskStore>()
            .text(agent_id),
        None
    );
    assert!(world
        .resource::<crate::conversations::ConversationStore>()
        .conversation_for_agent(agent_id)
        .is_none());
    let notes_state = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(notes_state.note_text_by_agent_uid(&agent_uid), None);
    assert_eq!(notes_state.dirty_since_secs, Some(0.0));
    assert!(world
        .resource::<crate::aegis::AegisPolicyStore>()
        .policy(&agent_uid)
        .is_none());
    assert!(world
        .resource::<crate::aegis::AegisRuntimeStore>()
        .state(agent_id)
        .is_none());
    assert_eq!(
        world
            .resource::<crate::conversations::ConversationPersistenceState>()
            .dirty_since_secs,
        Some(0.0)
    );
    assert_eq!(
        world
            .resource::<AppStatePersistenceState>()
            .dirty_since_secs,
        Some(0.0)
    );
}

/// Verifies that focusing an agent clears any selected tmux terminal override.
#[test]
fn focusing_agent_clears_owned_tmux_selection() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(fake_runtime_spawner(Arc::new(FakeDaemonClient::default())));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    world.insert_resource(catalog);
    world.insert_resource(AgentRuntimeIndex::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), None);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Focus(agent_id)));
    run_app_commands(&mut world);

    assert!(world
        .resource::<crate::terminals::ActiveTerminalContentState>()
        .selected_owned_tmux_session_uid()
        .is_none());
    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(agent_id)
    );
}

/// Verifies the enum default for terminal visibility policy is the non-isolating `ShowAll` mode.
#[test]
fn terminal_visibility_policy_defaults_to_show_all() {
    assert_eq!(
        TerminalVisibilityPolicy::default(),
        TerminalVisibilityPolicy::ShowAll
    );
}
