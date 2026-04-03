use crate::{
    app::{
        format_startup_panic, normalize_output_for_x11_fallback, primary_window_config_for,
        primary_window_plugin_config_for, resolve_disable_pipelined_rendering_for,
        resolve_force_fallback_adapter, resolve_force_fallback_adapter_for,
        resolve_linux_window_backend, resolve_output_dimension, resolve_output_mode,
        resolve_window_mode, resolve_window_scale_factor, should_force_x11_backend,
        uses_headless_runner, AppOutputConfig, LinuxWindowBackend, OutputMode,
    },
    hud::TerminalVisibilityPolicy,
    startup::{
        advance_startup_connecting, choose_startup_focus_session_name,
        should_request_visual_redraw, startup_visibility_policy_for_focus, StartupConnectPhase,
        StartupConnectState,
    },
    terminals::{TerminalId, TerminalRuntimeSpawner},
    tests::{fake_daemon_resource, fake_runtime_spawner, temp_dir, FakeDaemonClient},
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::*,
    window::{RequestRedraw, WindowMode},
};
use std::sync::Arc;

/// Verifies that the combined redraw predicate stays false when no terminal or HUD visual work is
/// pending.
#[test]
fn redraw_scheduler_stays_idle_without_visual_work() {
    assert!(!should_request_visual_redraw(false, false, false));
}

/// Verifies that any one of the three visual-work sources is enough to request another redraw.
#[test]
fn redraw_scheduler_runs_when_visual_work_exists() {
    assert!(should_request_visual_redraw(true, false, false));
    assert!(should_request_visual_redraw(false, true, false));
    assert!(should_request_visual_redraw(false, false, true));
}

/// Verifies the startup focus-selection precedence: persisted focus first, then other restored
/// sessions, then imported live sessions.
#[test]
fn startup_focus_prefers_persisted_focus_then_restored_then_imported() {
    assert_eq!(
        choose_startup_focus_session_name(Some("session-b"), &["session-a", "session-b"], &[]),
        Some("session-b")
    );
    assert_eq!(
        choose_startup_focus_session_name(None, &["session-a", "session-b"], &["session-c"]),
        Some("session-a")
    );
    assert_eq!(
        choose_startup_focus_session_name(None, &[], &["session-c", "session-d"]),
        Some("session-c")
    );
    assert_eq!(choose_startup_focus_session_name(None, &[], &[]), None);
}

/// Verifies that startup visibility policy isolates a chosen focus target and otherwise falls back
/// to `ShowAll`.
#[test]
fn startup_visibility_isolate_focused_terminal() {
    assert_eq!(
        startup_visibility_policy_for_focus(Some(TerminalId(7))),
        TerminalVisibilityPolicy::Isolate(TerminalId(7))
    );
    assert_eq!(
        startup_visibility_policy_for_focus(None),
        TerminalVisibilityPolicy::ShowAll
    );
}

/// Verifies that pending runtime spawner becomes ready when daemon is installed.
#[test]
fn pending_runtime_spawner_becomes_ready_when_daemon_is_installed() {
    let spawner = TerminalRuntimeSpawner::pending_headless();
    assert!(!spawner.is_ready());
    spawner.install_daemon(fake_daemon_resource(Arc::new(FakeDaemonClient::default())));
    assert!(spawner.is_ready());
}

/// Verifies that the startup overlay keeps the user-facing title at `Connecting` through the
/// restore phase.
#[test]
fn startup_connect_title_stays_connecting_during_restore() {
    let state = StartupConnectState::with_receiver_for_test(
        StartupConnectPhase::Restoring,
        std::sync::mpsc::channel().1,
    );
    assert_eq!(state.title(), "Connecting");
}

/// Verifies that setup installs the deferred background-connect receiver when the runtime is not yet ready.
#[test]
fn setup_scene_starts_background_connect_when_runtime_is_pending() {
    let mut world = World::default();
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(TerminalRuntimeSpawner::pending_headless());
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::startup::StartupLoadingState::default());
    world.insert_resource(StartupConnectState::default());
    world.insert_resource(Time::<()>::default());
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let startup_connect = world.resource::<StartupConnectState>();
    assert_eq!(startup_connect.phase(), StartupConnectPhase::Connecting);
    assert!(startup_connect.has_receiver());
}

/// Verifies that startup connecting advances to restoring when background connect completes.
#[test]
fn startup_connecting_advances_to_restoring_when_background_connect_completes() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let spawner = TerminalRuntimeSpawner::pending_headless();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Ok(fake_daemon_resource(Arc::new(
        FakeDaemonClient::default(),
    ))))
    .expect("test daemon resource should send");

    let mut world = World::default();
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(spawner.clone());
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::startup::StartupLoadingState::default());
    world.insert_resource(StartupConnectState::with_receiver_for_test(
        StartupConnectPhase::Connecting,
        rx,
    ));
    world.insert_resource(Time::<()>::default());
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(advance_startup_connecting).unwrap();

    assert!(spawner.is_ready());
    assert_eq!(
        world.resource::<StartupConnectState>().phase(),
        StartupConnectPhase::Restoring
    );
}

/// Verifies the default window-mode policy is borderless fullscreen unless overridden.
#[test]
fn defaults_window_mode_to_borderless_fullscreen() {
    assert_eq!(
        resolve_window_mode(None),
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    );
    assert_eq!(
        resolve_window_mode(Some("fullscreen")),
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    );
}

/// Verifies that explicit `windowed` configuration overrides the fullscreen default, including with
/// surrounding whitespace.
#[test]
fn allows_explicit_windowed_override() {
    assert_eq!(resolve_window_mode(Some("windowed")), WindowMode::Windowed);
    assert_eq!(
        resolve_window_mode(Some(" WINDOWED ")),
        WindowMode::Windowed
    );
}

/// Verifies environment-style parsing of output mode and output dimension overrides.
#[test]
fn parses_output_mode_and_dimensions() {
    assert_eq!(resolve_output_mode(None), OutputMode::Desktop);
    assert_eq!(resolve_output_mode(Some("")), OutputMode::Desktop);
    assert_eq!(
        resolve_output_mode(Some("offscreen")),
        OutputMode::OffscreenVerify
    );
    assert_eq!(
        resolve_output_mode(Some("offscreen-verify")),
        OutputMode::OffscreenVerify
    );
    assert_eq!(resolve_output_dimension(None, 42), 42);
    assert_eq!(resolve_output_dimension(Some(""), 42), 42);
    assert_eq!(resolve_output_dimension(Some("1600"), 42), 1600);
    assert_eq!(resolve_output_dimension(Some("0"), 42), 42);
    assert_eq!(resolve_output_dimension(Some("abc"), 42), 42);
}

/// Verifies the synthetic offscreen window is hidden, undecorated, unfocused, and forced into
/// windowed mode.
#[test]
fn offscreen_synthetic_window_config_is_hidden_and_windowed() {
    let window = primary_window_config_for(&AppOutputConfig {
        mode: OutputMode::OffscreenVerify,
        width: 1600,
        height: 1000,
        scale_factor_override: Some(1.5),
    });
    assert!(!window.visible);
    assert!(!window.decorations);
    assert!(!window.focused);
    assert_eq!(window.mode, WindowMode::Windowed);
    assert_eq!(window.physical_width(), 1600);
    assert_eq!(window.physical_height(), 1000);
    assert_eq!(window.resolution.scale_factor_override(), Some(1.5));
}

/// Verifies that offscreen mode switches to the headless runner and suppresses the normal primary
/// window plugin.
#[test]
fn offscreen_mode_uses_headless_runner_and_no_os_primary_window() {
    let output = AppOutputConfig {
        mode: OutputMode::OffscreenVerify,
        width: 1600,
        height: 1000,
        scale_factor_override: None,
    };
    assert!(uses_headless_runner(&output));
    assert!(primary_window_plugin_config_for(&output).is_none());
}

/// Verifies parsing/validation of the optional window scale-factor override.
#[test]
fn parses_optional_window_scale_factor_override() {
    assert_eq!(resolve_window_scale_factor(None), None);
    assert_eq!(resolve_window_scale_factor(Some("")), None);
    assert_eq!(resolve_window_scale_factor(Some("  ")), None);
    assert_eq!(resolve_window_scale_factor(Some("1.0")), Some(1.0));
    assert_eq!(resolve_window_scale_factor(Some(" 2.5 ")), Some(2.5));
    assert_eq!(resolve_window_scale_factor(Some("0")), None);
    assert_eq!(resolve_window_scale_factor(Some("-1")), None);
    assert_eq!(resolve_window_scale_factor(Some("abc")), None);
}

/// Verifies parsing of the force-fallback-adapter override and the opt-in default.
#[test]
fn parses_force_fallback_adapter_override() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    assert!(!resolve_force_fallback_adapter(None));
    assert!(!resolve_force_fallback_adapter(Some("")));
    assert!(resolve_force_fallback_adapter(Some("true")));
    assert!(resolve_force_fallback_adapter(Some("1")));
    assert!(!resolve_force_fallback_adapter(Some("false")));
    assert!(!resolve_force_fallback_adapter(Some("0")));
    assert!(!resolve_force_fallback_adapter_for(
        None,
        OutputMode::Desktop
    ));
    assert!(!resolve_force_fallback_adapter_for(
        None,
        OutputMode::OffscreenVerify
    ));
    assert!(resolve_force_fallback_adapter_for(
        Some("yes"),
        OutputMode::Desktop
    ));
}

/// Verifies the auto-disable policy for pipelined rendering on desktop Wayland.
#[test]
fn resolves_disable_pipelined_rendering_for_wayland_desktop_only() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    assert!(resolve_disable_pipelined_rendering_for(
        None,
        OutputMode::Desktop,
        Some("wayland"),
        Some("wayland-1")
    ));
    assert!(resolve_disable_pipelined_rendering_for(
        None,
        OutputMode::Desktop,
        None,
        Some("wayland-1")
    ));
    assert!(!resolve_disable_pipelined_rendering_for(
        None,
        OutputMode::Desktop,
        Some("x11"),
        None
    ));
    assert!(!resolve_disable_pipelined_rendering_for(
        None,
        OutputMode::OffscreenVerify,
        Some("wayland"),
        Some("wayland-1")
    ));
    assert!(resolve_disable_pipelined_rendering_for(
        Some("true"),
        OutputMode::Desktop,
        Some("x11"),
        None
    ));
    assert!(!resolve_disable_pipelined_rendering_for(
        Some("false"),
        OutputMode::Desktop,
        Some("wayland"),
        Some("wayland-1")
    ));
}

/// Verifies that resolves linux window backend policy.
#[test]
fn resolves_linux_window_backend_policy() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    assert_eq!(resolve_linux_window_backend(None), LinuxWindowBackend::Auto);
    assert_eq!(
        resolve_linux_window_backend(Some("x11")),
        LinuxWindowBackend::X11
    );
    assert_eq!(
        resolve_linux_window_backend(Some("wayland")),
        LinuxWindowBackend::Wayland
    );
    assert!(should_force_x11_backend(
        OutputMode::Desktop,
        LinuxWindowBackend::Auto,
        Some("wayland"),
        Some("wayland-1"),
        Some(":0")
    ));
    assert!(should_force_x11_backend(
        OutputMode::Desktop,
        LinuxWindowBackend::X11,
        Some("wayland"),
        Some("wayland-1"),
        Some(":0")
    ));
    assert!(!should_force_x11_backend(
        OutputMode::Desktop,
        LinuxWindowBackend::Wayland,
        Some("wayland"),
        Some("wayland-1"),
        Some(":0")
    ));
    assert!(!should_force_x11_backend(
        OutputMode::Desktop,
        LinuxWindowBackend::Auto,
        Some("wayland"),
        Some("wayland-1"),
        None
    ));
    assert!(!should_force_x11_backend(
        OutputMode::OffscreenVerify,
        LinuxWindowBackend::Auto,
        Some("wayland"),
        Some("wayland-1"),
        Some(":0")
    ));

    let normalized = normalize_output_for_x11_fallback(
        AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1400,
            height: 900,
            scale_factor_override: None,
        },
        true,
        None,
    );
    assert_eq!(normalized.scale_factor_override, Some(1.0));

    let preserved = normalize_output_for_x11_fallback(
        AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1400,
            height: 900,
            scale_factor_override: Some(1.5),
        },
        true,
        None,
    );
    assert_eq!(preserved.scale_factor_override, Some(1.5));

    let explicit_env = normalize_output_for_x11_fallback(
        AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1400,
            height: 900,
            scale_factor_override: None,
        },
        true,
        Some("2.0"),
    );
    assert_eq!(explicit_env.scale_factor_override, None);
}

/// Verifies that startup focus restoration skips a persisted `last_focused` session if that session
/// comes back disconnected and instead focuses a live restored session.
#[test]
fn startup_focus_skips_disconnected_restored_session() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let dir = temp_dir("neozeus-startup-focus-running-session");
    let sessions_path = dir.join("terminals.v1");
    let persisted = crate::terminals::PersistedTerminalSessions {
        sessions: vec![
            crate::terminals::TerminalSessionRecord {
                session_name: "neozeus-session-dead".to_owned(),
                label: Some("dead".to_owned()),
                creation_index: 0,
                last_focused: true,
            },
            crate::terminals::TerminalSessionRecord {
                session_name: "neozeus-session-live".to_owned(),
                label: Some("live".to_owned()),
                creation_index: 1,
                last_focused: false,
            },
        ],
    };
    std::fs::write(
        &sessions_path,
        crate::terminals::serialize_persisted_terminal_sessions(&persisted),
    )
    .expect("persisted sessions should write");

    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-dead",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );
    client.set_session_runtime(
        "neozeus-session-live",
        crate::terminals::TerminalRuntimeState::running("live session"),
    );

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(sessions_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let focus = world.resource::<crate::terminals::TerminalFocusState>();
    let manager = world.resource::<crate::terminals::TerminalManager>();
    let active_id = focus
        .active_id()
        .expect("startup should focus a live terminal");
    let active = manager
        .get(active_id)
        .expect("active terminal should exist");
    assert_eq!(active.session_name, "neozeus-session-live");
    assert_eq!(manager.terminal_ids().len(), 2);
    assert_eq!(client.sessions.lock().unwrap().len(), 2);
}

/// Verifies that when only disconnected sessions are restored, startup leaves them visible but
/// unfocused instead of isolating a dead session.
#[test]
fn startup_leaves_only_disconnected_sessions_visible_and_unfocused() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let dir = temp_dir("neozeus-startup-disconnected-visible");
    let sessions_path = dir.join("terminals.v1");
    let persisted = crate::terminals::PersistedTerminalSessions {
        sessions: vec![crate::terminals::TerminalSessionRecord {
            session_name: "neozeus-session-dead".to_owned(),
            label: Some("dead".to_owned()),
            creation_index: 0,
            last_focused: true,
        }],
    };
    std::fs::write(
        &sessions_path,
        crate::terminals::serialize_persisted_terminal_sessions(&persisted),
    )
    .expect("persisted sessions should write");

    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-dead",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(sessions_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let focus = world.resource::<crate::terminals::TerminalFocusState>();
    let manager = world.resource::<crate::terminals::TerminalManager>();
    assert_eq!(focus.active_id(), None);
    assert_eq!(manager.terminal_ids().len(), 1);
    let only_terminal = manager
        .get(manager.terminal_ids()[0])
        .expect("restored terminal should exist");
    assert_eq!(only_terminal.session_name, "neozeus-session-dead");
    assert_eq!(
        world
            .resource::<crate::hud::TerminalVisibilityState>()
            .policy,
        TerminalVisibilityPolicy::ShowAll
    );
    assert_eq!(client.sessions.lock().unwrap().len(), 1);
}

/// Verifies that restoring legacy app-state entries without stable agent uids backfills a new uid
/// and marks app state dirty for rewrite.
#[test]
fn startup_restore_backfills_missing_agent_uid_and_marks_app_state_dirty() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-agent-uid-backfill");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("legacy app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog.order.first().expect("restored agent should exist");
    let restored_uid = catalog.uid(restored_agent).expect("uid should backfill");
    assert!(!restored_uid.trim().is_empty());
    assert_eq!(catalog.find_by_uid(restored_uid), Some(restored_agent));
    assert_eq!(
        world
            .resource::<crate::app::AppStatePersistenceState>()
            .dirty_since_secs,
        Some(0.0)
    );
}

/// Verifies that startup restore plus owned-tmux sync rebinds recovered tmux children under the
/// restored agent using the stable persisted agent uid.
#[test]
fn startup_restore_rebinds_owned_tmux_children_under_agent() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-owned-tmux-bind");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");
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
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::hud::AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();
    world
        .resource_mut::<Time<()>>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::terminals::sync_owned_tmux_sessions)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();

    let rows = &world.resource::<crate::hud::AgentListView>().rows;
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[0].key, crate::hud::AgentListRowKey::Agent(_)));
    assert!(matches!(
        rows[1].key,
        crate::hud::AgentListRowKey::OwnedTmux(_)
    ));
}

/// Verifies that startup reaps an unpersisted disconnected persistent session instead of importing
/// it back as a dead agent, then falls back to spawning a fresh initial terminal.
#[test]
fn startup_restore_rebinds_multiple_owned_tmux_children_under_correct_agents_and_orphans() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored-a"),
    );
    client.set_session_runtime(
        "neozeus-session-b",
        crate::terminals::TerminalRuntimeState::running("restored-b"),
    );
    let dir = temp_dir("neozeus-startup-owned-tmux-multi-bind");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-uid-2\"\nsession_name=\"neozeus-session-b\"\nlabel=\"BETA\"\nkind=\"terminal\"\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");
    client.owned_tmux_sessions.lock().unwrap().extend([
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-2".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-2".into(),
            display_name: "TEST".into(),
            cwd: "/tmp/a-2".into(),
            attached: false,
            created_unix: 2,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-1".into(),
            owner_agent_uid: "agent-uid-1".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "BUILD".into(),
            cwd: "/tmp/a-1".into(),
            attached: false,
            created_unix: 1,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-3".into(),
            owner_agent_uid: "agent-uid-2".into(),
            tmux_name: "neozeus-tmux-3".into(),
            display_name: "BETA BUILD".into(),
            cwd: "/tmp/b-1".into(),
            attached: true,
            created_unix: 3,
        },
        crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-session-4".into(),
            owner_agent_uid: "missing-agent".into(),
            tmux_name: "neozeus-tmux-4".into(),
            display_name: "LOST".into(),
            cwd: "/tmp/orphan".into(),
            attached: false,
            created_unix: 4,
        },
    ]);

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::hud::AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();
    world
        .resource_mut::<Time<()>>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::terminals::sync_owned_tmux_sessions)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();

    let rows = &world.resource::<crate::hud::AgentListView>().rows;
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[0].label, "ALPHA");
    assert_eq!(rows[1].label, "BUILD");
    assert_eq!(rows[2].label, "TEST");
    assert_eq!(rows[3].label, "BETA");
    assert_eq!(rows[4].label, "BETA BUILD");
    assert_eq!(rows[5].label, "LOST");
    assert!(matches!(
        rows[1].key,
        crate::hud::AgentListRowKey::OwnedTmux(_)
    ));
    assert!(matches!(
        rows[2].key,
        crate::hud::AgentListRowKey::OwnedTmux(_)
    ));
    assert!(matches!(
        rows[4].key,
        crate::hud::AgentListRowKey::OwnedTmux(_)
    ));
    assert!(matches!(
        rows[5].kind,
        crate::hud::AgentListRowKind::OwnedTmux {
            owner: crate::hud::OwnedTmuxOwnerBinding::Orphan,
            ..
        }
    ));
}

#[test]
fn startup_reaps_unpersisted_disconnected_session_instead_of_restoring_it() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-dead",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let manager = world.resource::<crate::terminals::TerminalManager>();
    assert_eq!(manager.terminal_ids().len(), 1);
    let session_names = manager
        .terminal_ids()
        .iter()
        .map(|terminal_id| {
            manager
                .get(*terminal_id)
                .expect("terminal should exist")
                .session_name
                .clone()
        })
        .collect::<Vec<_>>();
    assert!(!session_names
        .iter()
        .any(|name| name == "neozeus-session-dead"));
    assert!(!client
        .sessions
        .lock()
        .unwrap()
        .contains("neozeus-session-dead"));
    assert_eq!(client.created_sessions.lock().unwrap().len(), 1);
}

/// Verifies the cold-start fallback path that spawns a brand-new initial terminal when restore/import
/// finds nothing usable.
#[test]
fn startup_spawns_initial_terminal_when_no_sessions_exist() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let manager = world.resource::<crate::terminals::TerminalManager>();
    let terminal_ids = manager.terminal_ids();
    assert_eq!(terminal_ids.len(), 1);
    let terminal_id = terminal_ids[0];
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(terminal_id)
    );
    assert_eq!(
        world
            .resource::<crate::hud::TerminalVisibilityState>()
            .policy,
        TerminalVisibilityPolicy::Isolate(terminal_id)
    );
    assert_eq!(client.sessions.lock().unwrap().len(), 1);
}

/// Verifies that a known missing-GPU startup panic is converted into a friendly user-facing error,
/// while unrelated panics are ignored.
#[test]
fn formats_missing_gpu_startup_panics_as_user_facing_errors() {
    let error = format_startup_panic(&"Unable to find a GPU! renderer init failed")
        .expect("missing gpu panic should be formatted");
    assert!(error.contains("could not find a usable graphics adapter"));
    assert!(format_startup_panic(&"some other panic").is_none());
}
