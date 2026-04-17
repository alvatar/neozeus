//! Shared test-only helpers for this area.
//!
//! Holds the imports, constants, and builders used by per-topic test submodules.
//! Private items are promoted to `pub(super)` so sibling submodules can reach them.

#![allow(unused_imports, dead_code)]

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

pub(super) fn home_env_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(super) fn sqlite3_available() -> bool {
    Command::new("sqlite3")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(super) fn write_codex_state_db(path: &std::path::Path, rows: &[(&str, &str, i64, &str)]) {
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

/// Initializes the app-command message resource in a test world.
pub(super) fn init_hud_commands(world: &mut World) {
    world.init_resource::<Messages<AppCommand>>();
}

/// Drains queued app commands from a test world.
pub(super) fn drain_hud_commands(world: &mut World) -> Vec<AppCommand> {
    world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap()
}

/// Handles run app commands.
pub(super) fn run_app_commands(world: &mut World) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    ensure_shared_app_command_test_resources(world);
    crate::app::run_apply_app_commands(world);
    world
        .run_system_once(crate::conversations::sync_task_notes_projection)
        .unwrap();
}

pub(super) fn clone_test_world(client: Arc<FakeDaemonClient>) -> World {
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

pub(super) fn build_agent_list_navigation_world() -> (World, crate::agents::AgentId, crate::agents::AgentId) {
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

pub(super) fn dispatch_agent_list_nav_step(world: &mut World, event: KeyboardInput) {
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

pub(super) fn assert_exactly_one_selected_row(world: &World, expected_key: crate::hud::AgentListRowKey) {
    let rows = &world.resource::<crate::hud::AgentListView>().rows;
    assert_eq!(rows.iter().filter(|row| row.focused).count(), 1);
    assert!(rows
        .iter()
        .any(|row| row.focused && row.key == expected_key));
}

