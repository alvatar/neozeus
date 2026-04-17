//! Test submodule: `owned_tmux` — extracted from the centralized test bucket.

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


/// Verifies that selecting an orphaned owned tmux entry does not disturb the currently focused
/// valid agent/terminal state.
#[test]
fn selecting_orphaned_owned_tmux_leaves_existing_focus_unchanged() {
    let client = Arc::new(FakeDaemonClient::default());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    manager.focus_terminal(terminal_id);

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "alpha-session".into(), None);

    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    world
        .resource_mut::<crate::terminals::OwnedTmuxSessionStore>()
        .sessions
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-orphan".into(),
            owner_agent_uid: "missing-agent".into(),
            tmux_name: "neozeus-tmux-orphan".into(),
            display_name: "B-1".into(),
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
                session_uid: "tmux-orphan".into(),
            },
        ));
    run_app_commands(&mut world);

    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(agent_id)
    );
    assert_eq!(
        world.resource::<crate::terminals::TerminalFocusState>().active_id(),
        Some(terminal_id)
    );
    assert!(world
        .resource::<crate::terminals::ActiveTerminalContentState>()
        .selected_owned_tmux_session_uid()
        .is_none());
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


/// Verifies that switching to a newly selected tmux child bypasses the periodic capture throttle.
#[test]
fn selecting_new_tmux_child_captures_immediately_without_waiting_for_poll_interval() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .tmux_captures
        .lock()
        .unwrap()
        .insert("tmux-session-0".into(), "old\ncontent\n".into());
    client
        .tmux_captures
        .lock()
        .unwrap()
        .insert("tmux-session-1".into(), "new\ncontent\n".into());

    let mut owned_tmux_sessions = crate::terminals::OwnedTmuxSessionStore::default();
    for session_uid in ["tmux-session-0", "tmux-session-1"] {
        owned_tmux_sessions.sessions.push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: session_uid.into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: format!("neozeus-{session_uid}"),
            display_name: "BUILD".into(),
            cwd: "/tmp/work".into(),
            attached: false,
            created_unix: 0,
        });
    }

    let mut world = World::default();
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(owned_tmux_sessions);
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0));
    world.insert_resource(time);

    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-0".into(), Some(crate::terminals::TerminalId(7)));
    world
        .run_system_once(crate::terminals::sync_active_terminal_content)
        .unwrap();

    world.clear_trackers();
    world
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_secs_f32(0.1));
    world
        .resource_mut::<crate::terminals::ActiveTerminalContentState>()
        .select_owned_tmux("tmux-session-1".into(), Some(crate::terminals::TerminalId(7)));
    world
        .run_system_once(crate::terminals::sync_active_terminal_content)
        .unwrap();

    let active_terminal_content = world.resource::<crate::terminals::ActiveTerminalContentState>();
    assert!(
        active_terminal_content
            .owned_tmux_surface_for(crate::terminals::TerminalId(7))
            .is_some(),
        "new tmux selection should capture immediately instead of waiting for the polling interval"
    );
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

