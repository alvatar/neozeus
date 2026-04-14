use crate::{
    app::{
        format_startup_panic, normalize_output_for_x11_fallback, primary_window_config_for,
        primary_window_plugin_config_for, resolve_disable_pipelined_rendering_for,
        resolve_force_fallback_adapter, resolve_force_fallback_adapter_for,
        resolve_linux_window_backend, resolve_output_dimension, resolve_output_mode,
        resolve_window_mode, resolve_window_scale_factor, should_force_x11_backend,
        uses_headless_runner, AppOutputConfig, LinuxWindowBackend, OutputMode,
    },
    hud::{HudState, HudWidgetKey, TerminalVisibilityPolicy},
    startup::{
        advance_startup_connecting, choose_startup_focus_session_name,
        request_redraw_while_visuals_active, should_request_visual_redraw,
        startup_visibility_policy_for_focus, DaemonConnectionState, StartupConnectPhase,
        StartupConnectState,
    },
    terminals::{
        TerminalId, TerminalPanel, TerminalPresentation, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalTextureState,
    },
    tests::{
        fake_daemon_resource, fake_runtime_spawner, insert_default_hud_resources,
        insert_terminal_manager_resources, insert_test_hud_state, surface_with_text, temp_dir,
        test_bridge, FakeDaemonClient,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::*,
    window::{RequestRedraw, WindowMode},
};
use std::sync::Arc;

mod clone_cases;

fn run_synced_hud_view_models(world: &mut World) {
    if !world.contains_resource::<crate::visual_contract::VisualContractState>() {
        world.insert_resource(crate::visual_contract::VisualContractState::default());
    }
    world
        .run_system_once(crate::visual_contract::sync_visual_contract_state)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();
}

/// Verifies that the combined redraw predicate stays false when no terminal or HUD visual work is
/// pending.
#[test]
fn redraw_scheduler_stays_idle_without_visual_work() {
    assert!(!should_request_visual_redraw(false, false, false, false));
}

/// Verifies that any one of the three visual-work sources is enough to request another redraw.
#[test]
fn redraw_scheduler_runs_when_visual_work_exists() {
    assert!(should_request_visual_redraw(true, false, false, false));
    assert!(should_request_visual_redraw(false, true, false, false));
    assert!(should_request_visual_redraw(false, false, true, false));
    assert!(should_request_visual_redraw(false, false, false, true));
}

/// Verifies that a selected agent row changing from idle to working requests a redraw even when the
/// terminal texture is already fully uploaded and no panel animation is active.
#[test]
fn working_agent_row_transition_requests_redraw_for_hud_feedback() {
    let (bridge, _) = test_bridge();
    let mut terminal_manager = crate::terminals::TerminalManager::default();
    let terminal_id = terminal_manager.create_terminal(bridge);
    terminal_manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some(surface_with_text(8, 120, 0, "header"));

    let mut agent_catalog = crate::agents::AgentCatalog::default();
    let agent_id = agent_catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );
    let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);

    let mut app = App::new();
    app.insert_resource(Time::<()>::default());
    app.insert_resource(agent_catalog);
    app.insert_resource(runtime_index);
    app.insert_resource(crate::agents::AgentStatusStore::default());
    app.insert_resource(crate::app::AppSessionState::default());
    app.insert_resource(crate::aegis::AegisPolicyStore::default());
    app.insert_resource(crate::aegis::AegisRuntimeStore::default());
    app.insert_resource(crate::conversations::AgentTaskStore::default());
    app.insert_resource(crate::conversations::ConversationStore::default());
    app.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    app.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    insert_terminal_manager_resources(app.world_mut(), terminal_manager);
    insert_test_hud_state(app.world_mut(), hud_state);
    app.insert_resource(TerminalPresentationStore::default());
    app.init_resource::<Messages<RequestRedraw>>();
    app.add_systems(
        Update,
        (
            crate::agents::sync_agent_status,
            crate::visual_contract::sync_visual_contract_state,
            crate::hud::sync_hud_view_models,
            request_redraw_while_visuals_active,
        )
            .chain(),
    );

    let panel_entity = app
        .world_mut()
        .spawn((
            TerminalPanel { id: terminal_id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::new(320.0, 180.0),
                target_size: Vec2::new(320.0, 180.0),
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.3,
                target_z: 0.3,
            },
        ))
        .id();
    app.world_mut()
        .resource_mut::<TerminalPresentationStore>()
        .register(
            terminal_id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: UVec2::new(1200, 160),
                    cell_size: UVec2::new(10, 20),
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: UVec2::new(1200, 160),
                    cell_size: UVec2::new(10, 20),
                },
                display_mode: crate::terminals::TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

    app.update();
    app.world_mut()
        .resource_mut::<Messages<RequestRedraw>>()
        .clear();

    {
        let world = app.world_mut();
        let mut time = world.resource_mut::<Time<()>>();
        time.advance_by(std::time::Duration::from_secs(1));
    }
    {
        let world = app.world_mut();
        let mut terminal_manager = world.resource_mut::<crate::terminals::TerminalManager>();
        let terminal = terminal_manager.get_mut(terminal_id).unwrap();
        terminal.snapshot.surface = Some({
            let mut surface = surface_with_text(8, 120, 0, "header");
            surface.set_text_cell(1, 3, "⠋ Working...");
            surface
        });
        terminal.surface_revision = 1;
    }
    app.world_mut()
        .resource_mut::<TerminalPresentationStore>()
        .get_mut(terminal_id)
        .unwrap()
        .uploaded_revision = 1;

    app.update();

    let world = app.world();
    let agent_list = world.resource::<crate::hud::AgentListView>();
    match &agent_list.rows[0].kind {
        crate::hud::AgentListRowKind::Agent { activity, .. } => {
            assert_eq!(*activity, crate::hud::AgentListActivity::Working)
        }
        other => panic!("expected agent row, got {other:?}"),
    }

    assert_eq!(
        world.resource::<Messages<RequestRedraw>>().len(),
        1,
        "working-state HUD transition should request redraw even without pending terminal upload"
    );
}

#[test]
fn stable_visual_contract_does_not_request_continuous_redraws() {
    let (bridge, _) = test_bridge();
    let mut terminal_manager = crate::terminals::TerminalManager::default();
    let terminal_id = terminal_manager.create_terminal(bridge);
    terminal_manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some({
        let mut surface = surface_with_text(8, 120, 0, "header");
        surface.set_text_cell(1, 3, "⠋ Working...");
        surface
    });

    let mut agent_catalog = crate::agents::AgentCatalog::default();
    let agent_id = agent_catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );
    let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs(1));
    app.insert_resource(time);
    app.insert_resource(agent_catalog);
    app.insert_resource(runtime_index);
    app.insert_resource(crate::agents::AgentStatusStore::default());
    app.insert_resource(crate::app::AppSessionState::default());
    app.insert_resource(crate::aegis::AegisPolicyStore::default());
    app.insert_resource(crate::aegis::AegisRuntimeStore::default());
    app.insert_resource(crate::conversations::AgentTaskStore::default());
    app.insert_resource(crate::conversations::ConversationStore::default());
    app.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    app.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    insert_terminal_manager_resources(app.world_mut(), terminal_manager);
    insert_test_hud_state(app.world_mut(), hud_state);
    app.insert_resource(TerminalPresentationStore::default());
    app.init_resource::<Messages<RequestRedraw>>();
    app.add_systems(
        Update,
        (
            crate::agents::sync_agent_status,
            crate::visual_contract::sync_visual_contract_state,
            crate::hud::sync_hud_view_models,
            request_redraw_while_visuals_active,
        )
            .chain(),
    );

    app.update();
    app.world_mut()
        .resource_mut::<Messages<RequestRedraw>>()
        .clear();
    app.update();

    assert_eq!(
        app.world().resource::<Messages<RequestRedraw>>().len(),
        0,
        "stable contract signatures must not keep the redraw loop alive"
    );
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
    let state = DaemonConnectionState::with_phase_for_test(
        StartupConnectPhase::Restoring,
        "Restoring sessions…",
    );
    assert_eq!(state.title(), "Connecting");
}

/// Verifies that setup installs the deferred background-connect receiver when the runtime is not yet ready.
#[test]
fn setup_scene_starts_background_connect_when_runtime_is_pending() {
    let mut world = World::default();
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(TerminalRuntimeSpawner::pending_headless());
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(DaemonConnectionState::default());
    world.insert_resource(StartupConnectState::default());
    world.insert_resource(Time::<()>::default());
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let startup_connect = world.resource::<StartupConnectState>();
    assert_eq!(
        world.resource::<DaemonConnectionState>().phase(),
        StartupConnectPhase::Connecting
    );
    assert!(startup_connect.has_receiver());
}

/// Verifies that startup connecting advances to restoring when background connect completes.
#[test]
fn setup_scene_auto_verify_uses_shared_spawn_attach_flow_and_isolates_verifier_terminal() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(DaemonConnectionState::default());
    world.insert_resource(StartupConnectState::default());
    world.insert_resource(Time::<()>::default());
    world.insert_resource(crate::verification::AutoVerifyConfig {
        command: "echo verify".to_owned(),
        delay_ms: 0,
    });
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let terminal_ids = world
        .resource::<crate::terminals::TerminalManager>()
        .terminal_ids()
        .to_vec();
    assert_eq!(terminal_ids.len(), 1);
    let terminal_id = terminal_ids[0];
    assert_eq!(
        world
            .resource::<crate::app::AppSessionState>()
            .focus_intent
            .target,
        crate::app::FocusIntentTarget::Terminal(terminal_id)
    );
    assert_eq!(
        world
            .resource::<crate::hud::TerminalVisibilityState>()
            .policy,
        TerminalVisibilityPolicy::Isolate(terminal_id)
    );
    let presentation_store = world.resource::<crate::terminals::TerminalPresentationStore>();
    assert!(presentation_store.any_startup_pending());
    assert!(presentation_store.is_startup_pending(terminal_id));
    let created = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert!(created[0]
        .0
        .starts_with(crate::terminals::VERIFIER_SESSION_PREFIX));
    std::thread::sleep(std::time::Duration::from_millis(20));
    let sent = client.sent_commands.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0],
        (
            created[0].0.clone(),
            crate::terminals::TerminalCommand::SendCommand("echo verify".to_owned()),
        )
    );
}

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
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(spawner.clone());
    world.insert_resource(crate::app::AppStatePersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(DaemonConnectionState::default());
    world.insert_resource(StartupConnectState::with_receiver_for_test(rx));
    world.insert_resource(Time::<()>::default());
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(advance_startup_connecting).unwrap();

    assert!(spawner.is_ready());
    assert_eq!(
        world.resource::<DaemonConnectionState>().phase(),
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
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
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
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
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

/// Verifies that startup restore only marks interactive sessions as startup-pending.
#[test]
fn startup_restore_does_not_mark_disconnected_sessions_as_startup_pending() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-live",
        crate::terminals::TerminalRuntimeState::running("live session"),
    );
    client.set_session_runtime(
        "neozeus-session-dead",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let dir = temp_dir("neozeus-startup-disconnected-not-pending");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        concat!(
            "neozeus state version 1\n",
            "[agent]\n",
            "agent_uid=\"agent-live\"\n",
            "session_name=\"neozeus-session-live\"\n",
            "label=\"LIVE\"\n",
            "kind=\"pi\"\n",
            "order_index=0\n",
            "focused=1\n",
            "[/agent]\n",
            "[agent]\n",
            "agent_uid=\"agent-dead\"\n",
            "session_name=\"neozeus-session-dead\"\n",
            "label=\"DEAD\"\n",
            "kind=\"pi\"\n",
            "order_index=1\n",
            "focused=0\n",
            "[/agent]\n",
        ),
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
    let live_terminal = runtime_index
        .agent_for_session("neozeus-session-live")
        .and_then(|agent_id| runtime_index.primary_terminal(agent_id))
        .expect("live terminal should be attached");
    let dead_terminal = runtime_index
        .agent_for_session("neozeus-session-dead")
        .and_then(|agent_id| runtime_index.primary_terminal(agent_id))
        .expect("dead terminal should be attached");
    let presentation_store = world.resource::<crate::terminals::TerminalPresentationStore>();
    assert!(presentation_store.is_startup_pending(live_terminal));
    assert!(
        !presentation_store.is_startup_pending(dead_terminal),
        "disconnected restored terminals must not stay startup-pending forever"
    );
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
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
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
    let session_metadata = client.session_metadata.lock().unwrap();
    let mirrored = session_metadata
        .get("neozeus-session-a")
        .expect("restored session should mirror app-owned identity back into daemon metadata");
    assert_eq!(mirrored.agent_uid.as_deref(), Some(restored_uid));
    assert_eq!(mirrored.agent_label.as_deref(), Some("ALPHA"));
    assert_eq!(
        mirrored.agent_kind,
        Some(crate::shared::daemon_wire::DaemonAgentKind::Pi)
    );
}

#[test]
fn startup_restore_rehydrates_aegis_policy_from_persisted_snapshot() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-legacy-aegis-ignored");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 2\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\naegis_enabled=1\naegis_prompt_text=\"continue cleanly\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog.order.first().expect("restored agent should exist");
    assert_eq!(catalog.uid(restored_agent), Some("agent-uid-1"));
    let _ = catalog;

    let policy = world
        .resource::<crate::aegis::AegisPolicyStore>()
        .policy("agent-uid-1")
        .expect("persisted Aegis policy should restore");
    assert!(policy.enabled);
    assert_eq!(policy.prompt_text, "continue cleanly");
    assert!(world
        .resource::<crate::aegis::AegisRuntimeStore>()
        .state(restored_agent)
        .is_none());
}

#[test]
fn startup_restore_migrates_legacy_session_notes_into_task_store_and_marks_notes_dirty() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-legacy-notes-migration");
    let app_state_path = dir.join("neozeus-state.v1");
    let notes_path = dir.join("notes.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");
    std::fs::write(
        &notes_path,
        "version 2\nnote name=neozeus-session-a\n- [ ] legacy task\n.\n",
    )
    .expect("legacy notes should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    notes_state.path = Some(notes_path);
    world.insert_resource(notes_state);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let restored_agent = *world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .first()
        .expect("restored agent should exist");
    assert_eq!(
        world
            .resource::<crate::conversations::AgentTaskStore>()
            .text(restored_agent),
        Some("- [ ] legacy task")
    );
    let notes_state = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(notes_state.note_text("neozeus-session-a"), None);
    assert_eq!(notes_state.dirty_since_secs, Some(0.0));
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
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();
    assert_eq!(
        world
            .resource::<crate::terminals::OwnedTmuxSessionStore>()
            .sessions
            .len(),
        1,
        "startup should hydrate owned tmux state before the first interactive poke"
    );
    run_synced_hud_view_models(&mut world);

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
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();
    assert_eq!(
        world
            .resource::<crate::terminals::OwnedTmuxSessionStore>()
            .sessions
            .len(),
        4,
        "startup should hydrate every owned tmux child before the first interactive poke"
    );
    run_synced_hud_view_models(&mut world);

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
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
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

#[test]
fn startup_respawns_claude_agent_from_recovery_spec_when_daemon_is_empty() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-claude-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "claude --resume claude-session-1"
    ));
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog
        .order
        .first()
        .expect("restored Claude agent should exist");
    assert_eq!(catalog.uid(restored_agent), Some("agent-uid-1"));
    assert!(matches!(
        catalog.recovery_spec(restored_agent),
        Some(crate::agents::AgentRecoverySpec::Claude { session_id, cwd, .. })
            if session_id == "claude-session-1" && cwd == "/tmp/demo"
    ));
}

#[test]
fn startup_restore_reattaches_live_agent_by_runtime_session_name_when_available() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-live-claude",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-runtime-session-reattach");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-live-claude\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/claude-demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert!(client.sent_commands.lock().unwrap().is_empty());
    let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
    let agent_id = runtime_index
        .agent_for_session("neozeus-live-claude")
        .expect("live session should be reattached");
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.uid(agent_id), Some("agent-uid-1"));
    assert!(matches!(
        catalog.recovery_spec(agent_id),
        Some(crate::agents::AgentRecoverySpec::Claude { session_id, cwd, .. })
            if session_id == "claude-session-1" && cwd == "/tmp/claude-demo"
    ));
}

#[test]
fn startup_restore_preserves_paused_agents_and_projects_them_to_display_bottom() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-live-alpha",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    client.set_session_runtime(
        "neozeus-live-beta",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-restore-paused-agents");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-live-alpha\"\nlabel=\"ALPHA\"\nkind=\"terminal\"\npaused=1\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-uid-2\"\nruntime_session_name=\"neozeus-live-beta\"\nlabel=\"BETA\"\nkind=\"terminal\"\npaused=0\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let display_order = catalog.display_order();
    assert_eq!(display_order.len(), 2);
    assert_eq!(catalog.label(display_order[0]), Some("BETA"));
    assert_eq!(catalog.label(display_order[1]), Some("ALPHA"));
    let alpha_id = catalog
        .find_by_uid("agent-uid-1")
        .expect("alpha should exist");
    assert!(catalog.is_paused(alpha_id));
}

#[test]
fn startup_restore_falls_back_to_recovery_when_runtime_session_is_gone() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-stale-runtime-fallback");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-stale\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "claude --resume claude-session-1"
    ));
}

#[test]
fn startup_restore_reattaches_live_agent_and_respawns_missing_recoverable_agent() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-live-claude",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    client.session_metadata.lock().unwrap().insert(
        "neozeus-live-claude".into(),
        crate::shared::daemon_wire::DaemonSessionMetadata {
            agent_uid: Some("agent-uid-1".into()),
            agent_label: Some("ALPHA".into()),
            agent_kind: Some(crate::shared::daemon_wire::DaemonAgentKind::Claude),
        },
    );
    let dir = temp_dir("neozeus-startup-mixed-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/claude-demo\"\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-uid-2\"\nlabel=\"BETA\"\nkind=\"codex\"\nrecovery_mode=\"codex\"\nrecovery_session_id=\"codex-thread-1\"\nrecovery_cwd=\"/tmp/codex-demo\"\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert_eq!(client.created_sessions.lock().unwrap().len(), 1);
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "codex resume codex-thread-1 -C /tmp/codex-demo"
    ));
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.order.len(), 2);
    assert!(catalog
        .order
        .iter()
        .copied()
        .any(|agent_id| catalog.uid(agent_id) == Some("agent-uid-1")));
    assert!(catalog
        .order
        .iter()
        .copied()
        .any(|agent_id| catalog.uid(agent_id) == Some("agent-uid-2")));
}

#[test]
fn startup_imported_live_sessions_serialize_runtime_binding_truthfully_on_save() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-live-terminal",
        crate::terminals::TerminalRuntimeState::running("imported"),
    );
    client.session_metadata.lock().unwrap().insert(
        "neozeus-session-live-terminal".into(),
        crate::shared::daemon_wire::DaemonSessionMetadata {
            agent_uid: Some("agent-live".into()),
            agent_label: Some("LIVE".into()),
            agent_kind: Some(crate::shared::daemon_wire::DaemonAgentKind::Terminal),
        },
    );
    let dir = temp_dir("neozeus-startup-import-save-runtime-binding");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path.clone()),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::app::save_app_state_if_dirty)
        .unwrap();

    let persisted = crate::app::load_persisted_app_state_from(&app_state_path);
    let imported = persisted
        .agents
        .iter()
        .find(|record| record.agent_uid.as_deref() == Some("agent-live"))
        .expect("imported live session should persist");
    assert_eq!(imported.label.as_deref(), Some("LIVE"));
    assert_eq!(
        imported.kind,
        crate::shared::app_state_file::PersistedAgentKind::Terminal
    );
    assert_eq!(
        imported.runtime_session_name.as_deref(),
        Some("neozeus-session-live-terminal")
    );
    assert!(imported.recovery.is_none());
}

#[test]
fn startup_restore_reattaches_live_only_agent_when_runtime_is_still_live() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-live-only",
        crate::terminals::TerminalRuntimeState::running("live-only"),
    );
    let dir = temp_dir("neozeus-startup-live-only-reattach");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-live\"\nruntime_session_name=\"neozeus-session-live-only\"\nlabel=\"LIVE\"\nkind=\"terminal\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert!(client.sent_commands.lock().unwrap().is_empty());
    let runtime_index = world.resource::<crate::agents::AgentRuntimeIndex>();
    let agent_id = runtime_index
        .agent_for_session("neozeus-session-live-only")
        .expect("live-only agent should be reattached");
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.uid(agent_id), Some("agent-live"));
    assert_eq!(catalog.label(agent_id), Some("LIVE"));
    assert_eq!(
        catalog.kind(agent_id),
        Some(crate::agents::AgentKind::Terminal)
    );
    assert!(catalog.recovery_spec(agent_id).is_none());
}

#[test]
fn startup_restore_reports_invalid_pi_recovery_without_blocking_other_agents() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-invalid-pi-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        format!(
            "neozeus state version 4\n[agent]\nagent_uid=\"agent-bad\"\nlabel=\"BROKEN-PI\"\nkind=\"pi\"\nrecovery_mode=\"pi\"\nrecovery_session_path=\"{}\"\nrecovery_cwd=\"/tmp/missing\"\norder_index=0\n[/agent]\n[agent]\nagent_uid=\"agent-good\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=1\n[/agent]\n",
            dir.join("missing-session.jsonl").display()
        ),
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "claude --resume claude-session-1"
    ));
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.order.len(), 1);
    assert_eq!(catalog.uid(catalog.order[0]), Some("agent-good"));
    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Automatic recovery completed: 1 restored, 1 failed")
    );
    assert!(status
        .details
        .iter()
        .any(|line| line.contains("BROKEN-PI") && line.contains("Pi session path missing")));
}

#[test]
fn startup_restore_reports_invalid_claude_recovery_and_does_not_spawn_default_agent() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-invalid-claude-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert!(world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .is_empty());
    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Automatic recovery completed: 0 restored, 1 failed")
    );
    assert!(status
        .details
        .iter()
        .any(|line| line.contains("ALPHA") && line.contains("Claude session id missing")));
}

#[test]
fn startup_restore_reports_invalid_codex_recovery_and_does_not_spawn_default_agent() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-invalid-codex-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-2\"\nlabel=\"BETA\"\nkind=\"codex\"\nrecovery_mode=\"codex\"\nrecovery_session_id=\"codex-thread-1\"\nrecovery_cwd=\"\"\norder_index=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert!(world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .is_empty());
    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Automatic recovery completed: 0 restored, 1 failed")
    );
    assert!(status
        .details
        .iter()
        .any(|line| line.contains("BETA") && line.contains("Codex cwd missing")));
}

#[test]
fn startup_respawns_codex_agent_from_recovery_spec_when_daemon_is_empty() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-codex-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-2\"\nlabel=\"BETA\"\nkind=\"codex\"\nrecovery_mode=\"codex\"\nrecovery_session_id=\"codex-thread-1\"\nrecovery_cwd=\"/tmp/codex-demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/codex-demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "codex resume codex-thread-1 -C /tmp/codex-demo"
    ));
    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog
        .order
        .first()
        .expect("restored Codex agent should exist");
    assert_eq!(catalog.uid(restored_agent), Some("agent-uid-2"));
    assert!(matches!(
        catalog.recovery_spec(restored_agent),
        Some(crate::agents::AgentRecoverySpec::Codex { session_id, cwd, .. })
            if session_id == "codex-thread-1" && cwd == "/tmp/codex-demo"
    ));
}

#[test]
fn reset_runtime_kills_live_sessions_and_rebuilds_from_snapshot() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-live-a",
        crate::terminals::TerminalRuntimeState::running("live"),
    );
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-1".into(),
            owner_agent_uid: "agent-uid-live".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "tmux child".into(),
            cwd: "/tmp/demo".into(),
            attached: false,
            created_unix: 1,
        });
    let dir = temp_dir("neozeus-reset-rebuild");
    let app_state_path = dir.join("neozeus-state.v1");
    let conversations_path = dir.join("conversations.v1");
    let notes_path = dir.join("notes.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState {
        path: Some(conversations_path.clone()),
        dirty_since_secs: Some(1.0),
    });
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path.clone()),
        dirty_since_secs: None,
    });
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    notes_state.path = Some(notes_path.clone());
    notes_state.dirty_since_secs = Some(1.0);
    assert!(notes_state.set_note_text_by_agent_uid("agent-uid-live", "- [ ] stale task"));
    assert!(notes_state.set_note_text("neozeus-live-a", "legacy note"));
    world.insert_resource(notes_state);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    assert!(client.owned_tmux_sessions.lock().unwrap().is_empty());
    assert!(!client.sessions.lock().unwrap().contains("neozeus-live-a"));
    let notes_state = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(notes_state.path.as_ref(), Some(&notes_path));
    assert_eq!(notes_state.dirty_since_secs, None);
    assert!(notes_state
        .note_text_by_agent_uid("agent-uid-live")
        .is_none());
    assert!(notes_state.note_text("neozeus-live-a").is_none());
    let app_state_persistence = world.resource::<crate::app::AppStatePersistenceState>();
    assert_eq!(app_state_persistence.path.as_ref(), Some(&app_state_path));
    let conversation_persistence =
        world.resource::<crate::conversations::ConversationPersistenceState>();
    assert_eq!(
        conversation_persistence.path.as_ref(),
        Some(&conversations_path)
    );
    assert_eq!(conversation_persistence.dirty_since_secs, None);
    let commands = client.sent_commands.lock().unwrap().clone();
    assert!(commands.iter().any(|(_, command)| matches!(
        command,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "claude --resume claude-session-1"
    )));
    assert_eq!(
        world.resource::<crate::agents::AgentCatalog>().order.len(),
        1
    );
    let focused_terminal = world
        .resource::<crate::terminals::TerminalFocusState>()
        .active_id()
        .expect("reset restore should refocus the restored terminal");
    assert_eq!(
        world
            .resource::<crate::hud::TerminalVisibilityState>()
            .policy,
        TerminalVisibilityPolicy::Isolate(focused_terminal)
    );
    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Reset recovery completed: 1 restored, 0 failed")
    );
    assert!(status.details.iter().any(|line| line == "Reset confirmed"));
    assert!(status
        .details
        .iter()
        .any(|line| line == "Runtime clear started"));
    assert!(status
        .details
        .iter()
        .any(|line| line == "Runtime clear completed"));
    assert!(status
        .details
        .iter()
        .any(|line| line == "Automatic recovery started from saved snapshot"));
}

#[test]
fn reset_runtime_rehydrates_persisted_conversations_and_task_notes_after_restore() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-rehydrate-projections");
    let app_state_path = dir.join("neozeus-state.v1");
    let conversations_path = dir.join("conversations.v1");
    let notes_path = dir.join("notes.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");
    std::fs::write(
        &conversations_path,
        "version 2\n[conversation]\nagent_uid=\"agent-uid-1\"\n[message]\nauthor=\"user\"\ndelivery=\"delivered\"\nbody=\"hello after reset\"\n",
    )
    .expect("conversations should write");
    std::fs::write(
        &notes_path,
        "version 2\nnote agent_uid=agent-uid-1\n- [ ] restored task\n.\n",
    )
    .expect("notes should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState {
        path: Some(conversations_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    notes_state.path = Some(notes_path);
    world.insert_resource(notes_state);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let restored_agent = world.resource::<crate::agents::AgentCatalog>().order[0];
    assert_eq!(
        world
            .resource::<crate::conversations::AgentTaskStore>()
            .text(restored_agent),
        Some("- [ ] restored task")
    );
    let notes_state = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(
        notes_state.note_text_by_agent_uid("agent-uid-1"),
        Some("- [ ] restored task")
    );
    let conversations = world.resource::<crate::conversations::ConversationStore>();
    let conversation_id = conversations
        .conversation_for_agent(restored_agent)
        .expect("restored conversation should exist");
    assert_eq!(
        conversations.messages_for(conversation_id),
        vec![(
            "hello after reset".to_owned(),
            crate::conversations::MessageDeliveryState::Delivered,
        )]
    );
}

#[test]
fn reset_runtime_followed_by_conversation_mutation_still_persists_conversations() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-persists-conversations");
    let app_state_path = dir.join("neozeus-state.v1");
    let conversations_path = dir.join("conversations.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState {
        path: Some(conversations_path.clone()),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let agent_id = world.resource::<crate::agents::AgentCatalog>().order[0];
    {
        let mut conversations = world.resource_mut::<crate::conversations::ConversationStore>();
        let conversation_id = conversations.ensure_conversation(agent_id);
        let _ = conversations.push_message(
            conversation_id,
            crate::conversations::MessageAuthor::User,
            "hello after reset".into(),
            crate::conversations::MessageDeliveryState::Delivered,
        );
    }
    {
        let time = *world.resource::<Time>();
        let mut persistence =
            world.resource_mut::<crate::conversations::ConversationPersistenceState>();
        crate::conversations::mark_conversations_dirty(&mut persistence, Some(&time));
    }
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::conversations::save_conversations_if_dirty)
        .unwrap();

    let mut restored = crate::conversations::ConversationStore::default();
    crate::conversations::restore_persisted_conversations_from_path(
        &conversations_path,
        world.resource::<crate::agents::AgentCatalog>(),
        world.resource::<crate::agents::AgentRuntimeIndex>(),
        &mut restored,
    );
    let conversation_id = restored
        .conversation_for_agent(agent_id)
        .expect("restored conversation should exist");
    assert_eq!(
        restored.messages_for(conversation_id),
        vec![(
            "hello after reset".to_owned(),
            crate::conversations::MessageDeliveryState::Delivered,
        )]
    );
}

#[test]
fn reset_runtime_followed_by_task_mutation_still_persists_notes() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-persists-notes");
    let app_state_path = dir.join("neozeus-state.v1");
    let notes_path = dir.join("notes.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    notes_state.path = Some(notes_path.clone());
    world.insert_resource(notes_state);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let agent_id = world.resource::<crate::agents::AgentCatalog>().order[0];
    {
        let mut task_store = world.resource_mut::<crate::conversations::AgentTaskStore>();
        let _ = task_store.set_text(agent_id, "- [ ] persisted after reset");
    }
    world
        .run_system_once(crate::conversations::sync_task_notes_projection)
        .unwrap();
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::terminals::save_terminal_notes_if_dirty)
        .unwrap();

    let agent_uid = world
        .resource::<crate::agents::AgentCatalog>()
        .uid(agent_id)
        .expect("restored agent uid should exist")
        .to_owned();
    let restored = crate::terminals::load_terminal_notes_from(&notes_path);
    assert_eq!(
        restored.notes_by_agent_uid.get(&agent_uid),
        Some(&"- [ ] persisted after reset".to_owned())
    );
}

#[test]
fn reset_restore_still_supports_truthful_app_state_save() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-save-app-state");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path.clone()),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    {
        let time = *world.resource::<Time>();
        let mut persistence = world.resource_mut::<crate::app::AppStatePersistenceState>();
        crate::app::mark_app_state_dirty(&mut persistence, Some(&time));
    }
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::app::save_app_state_if_dirty)
        .unwrap();

    let persisted = crate::app::load_persisted_app_state_from(&app_state_path);
    assert_eq!(persisted.agents.len(), 1);
    let restored = &persisted.agents[0];
    assert_eq!(restored.agent_uid.as_deref(), Some("agent-uid-1"));
    assert_eq!(restored.label.as_deref(), Some("ALPHA"));
    assert_eq!(
        restored.kind,
        crate::shared::app_state_file::PersistedAgentKind::Claude
    );
    assert!(restored.runtime_session_name.is_some());
    assert!(matches!(
        restored.recovery,
        Some(crate::shared::app_state_file::PersistedAgentRecoverySpec::Claude {
            ref session_id,
            ref cwd,
            ..
        }) if session_id == "claude-session-1" && cwd == "/tmp/demo"
    ));
    assert!(restored.last_focused);
}

#[test]
fn reset_runtime_is_idempotent_when_triggered_twice() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-idempotent");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    for _ in 0..2 {
        world
            .resource_mut::<Messages<crate::app::AppCommand>>()
            .write(crate::app::AppCommand::Recovery(
                crate::app::RecoveryCommand::ResetAll,
            ));
        world
            .run_system_once(crate::app::run_apply_app_commands)
            .unwrap();
    }

    assert_eq!(
        world.resource::<crate::agents::AgentCatalog>().order.len(),
        1
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalManager>()
            .terminal_ids()
            .len(),
        1
    );
    assert_eq!(client.sessions.lock().unwrap().len(), 1);
    assert_eq!(
        world
            .resource::<crate::app::AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset recovery completed: 1 restored, 0 failed")
    );
}

#[test]
fn reset_runtime_tolerates_partial_daemon_kill_failures_without_corrupting_local_state() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    *client.fail_owned_tmux_kill.lock().unwrap() = true;
    client.set_session_runtime(
        "neozeus-live-a",
        crate::terminals::TerminalRuntimeState::running("live"),
    );
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-1".into(),
            owner_agent_uid: "agent-uid-live".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "tmux child".into(),
            cwd: "/tmp/demo".into(),
            attached: false,
            created_unix: 1,
        });
    let dir = temp_dir("neozeus-reset-kill-failures");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    assert_eq!(
        world.resource::<crate::agents::AgentCatalog>().order.len(),
        1
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalManager>()
            .terminal_ids()
            .len(),
        1
    );
    assert_eq!(
        world
            .resource::<crate::app::AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset recovery completed: 1 restored, 0 failed")
    );
    assert!(client.sessions.lock().unwrap().contains("neozeus-live-a"));
    assert_eq!(client.owned_tmux_sessions.lock().unwrap().len(), 1);
}

#[test]
fn reset_runtime_reports_success_when_no_saved_snapshot_exists() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-no-snapshot");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(&app_state_path, "neozeus state version 4\n").expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Reset completed: runtime cleared; no saved snapshot to restore")
    );
    assert!(status
        .details
        .iter()
        .any(|line| line == "No saved snapshot to restore"));
}

#[test]
fn reset_runtime_reports_daemon_discovery_failure_without_corrupting_clear_state() {
    let dir = temp_dir("neozeus-reset-discovery-failure");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(crate::terminals::TerminalRuntimeSpawner::pending_headless());
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Reset recovery completed: 0 restored, 1 failed")
    );
    assert!(status.details.iter().any(|line| {
        line == "daemon session discovery failed: terminal runtime still connecting"
    }));
    assert!(world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .is_empty());
    assert!(world
        .resource::<crate::terminals::TerminalManager>()
        .terminal_ids()
        .is_empty());
}

#[test]
fn reset_runtime_reports_missing_live_only_agents_as_skipped() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-skipped-live-only");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-recoverable\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-live-only\"\nruntime_session_name=\"neozeus-session-missing\"\nlabel=\"BETA\"\nkind=\"terminal\"\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Reset recovery completed: 1 restored, 0 failed, 1 skipped")
    );
    assert!(status.details.iter().any(|line| {
        line == "startup skipped live-only agent BETA: runtime session unavailable"
    }));
}

#[test]
fn startup_recovery_status_includes_skipped_live_only_agents_in_title_and_details() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-skipped-live-only-status");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-recoverable\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-live-only\"\nruntime_session_name=\"neozeus-session-missing\"\nlabel=\"BETA\"\nkind=\"terminal\"\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
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
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Automatic recovery completed: 1 restored, 0 failed, 1 skipped")
    );
    assert!(status.details.iter().any(|line| {
        line == "startup skipped live-only agent BETA: runtime session unavailable"
    }));
}

/// Verifies the cold-start fallback path that spawns a brand-new initial terminal when restore/import
/// finds nothing usable.
#[test]
fn startup_spawns_initial_terminal_when_no_sessions_exist() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-startup-no-sessions");
    let app_state_path = dir.join("empty-state.v1");
    std::fs::write(&app_state_path, "neozeus state version 4\n").expect("app state should write");
    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
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
    assert!(world
        .resource::<crate::app::AppSessionState>()
        .recovery_status
        .title
        .is_none());
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
