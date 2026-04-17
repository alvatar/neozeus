//! Test submodule: `hud_navigation` — extracted from the centralized test bucket.

#![allow(unused_imports)]

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


use super::support::*;

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

