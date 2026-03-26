use crate::{
    app::{
        format_startup_panic, primary_window_config_for, primary_window_config_for_with_config,
        primary_window_plugin_config_for, resolve_force_fallback_adapter,
        resolve_force_fallback_adapter_for, resolve_output_dimension, resolve_output_mode,
        resolve_window_mode, resolve_window_scale_factor, uses_headless_runner, AppOutputConfig,
        OutputMode,
    },
    app_config::{
        load_neozeus_config_from, parse_neozeus_config, resolve_neozeus_config_path_with,
        resolve_terminal_font_path,
    },
    hud::TerminalVisibilityPolicy,
    startup::{
        choose_startup_focus_session_name, should_request_visual_redraw,
        startup_visibility_policy_for_focus,
    },
    terminals::TerminalId,
    tests::{fake_runtime_spawner, temp_dir},
};
use bevy::{ecs::system::RunSystemOnce, prelude::*, window::WindowMode};
use std::sync::Arc;

// Verifies that redraw scheduler stays idle without visual work.
#[test]
fn redraw_scheduler_stays_idle_without_visual_work() {
    assert!(!should_request_visual_redraw(false, false, false));
}

// Verifies that redraw scheduler runs when visual work exists.
#[test]
fn redraw_scheduler_runs_when_visual_work_exists() {
    assert!(should_request_visual_redraw(true, false, false));
    assert!(should_request_visual_redraw(false, true, false));
    assert!(should_request_visual_redraw(false, false, true));
}

// Verifies that startup focus prefers persisted focus then restored then imported.
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

// Verifies that startup visibility isolate focused terminal.
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

// Verifies that defaults window mode to borderless fullscreen.
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

// Verifies that allows explicit windowed override.
#[test]
fn allows_explicit_windowed_override() {
    assert_eq!(resolve_window_mode(Some("windowed")), WindowMode::Windowed);
    assert_eq!(
        resolve_window_mode(Some(" WINDOWED ")),
        WindowMode::Windowed
    );
}

// Verifies that parses NeoZeus TOML config.
#[test]
fn parses_neozeus_toml_config() {
    let config = parse_neozeus_config(
        r#"
        [terminal]
        font_path = "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"
        font_size_px = 16.0
        baseline_offset_px = -0.5

        [window]
        title = "NeoZeus"
        app_id = "neozeus-dev"
        "#,
    )
    .expect("config should parse");

    assert_eq!(
        resolve_terminal_font_path(&config),
        Some(std::path::PathBuf::from(
            "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"
        ))
    );
    assert_eq!(config.terminal.font_size_px, Some(16.0));
    assert_eq!(config.terminal.baseline_offset_px, Some(-0.5));
    assert_eq!(config.window.title.as_deref(), Some("NeoZeus"));
    assert_eq!(config.window.app_id.as_deref(), Some("neozeus-dev"));
}

// Verifies that NeoZeus config path resolution prefers explicit then XDG then home then cwd.
#[test]
fn neozeus_config_path_resolution_prefers_explicit_then_xdg_then_home_then_cwd() {
    let dir = temp_dir("neozeus-config-resolution");
    let explicit = dir.join("explicit.toml");
    let xdg = dir.join("xdg/neozeus/config.toml");
    let home = dir.join("home/.config/neozeus/config.toml");
    let cwd = dir.join("cwd/neozeus.toml");
    for path in [&explicit, &xdg, &home, &cwd] {
        std::fs::create_dir_all(path.parent().unwrap()).expect("config dir should exist");
        std::fs::write(path, "").expect("config file should exist");
    }

    assert_eq!(
        resolve_neozeus_config_path_with(
            Some(explicit.as_os_str()),
            Some(dir.join("xdg").as_os_str()),
            Some(dir.join("home").as_os_str()),
            Some(&dir.join("cwd")),
        ),
        Some(explicit.clone())
    );
    std::fs::remove_file(&explicit).expect("explicit config should be removable");
    assert_eq!(
        resolve_neozeus_config_path_with(
            None,
            Some(dir.join("xdg").as_os_str()),
            Some(dir.join("home").as_os_str()),
            Some(&dir.join("cwd")),
        ),
        Some(xdg.clone())
    );
    std::fs::remove_file(&xdg).expect("xdg config should be removable");
    assert_eq!(
        resolve_neozeus_config_path_with(
            None,
            None,
            Some(dir.join("home").as_os_str()),
            Some(&dir.join("cwd")),
        ),
        Some(home.clone())
    );
    std::fs::remove_file(&home).expect("home config should be removable");
    assert_eq!(
        resolve_neozeus_config_path_with(None, None, None, Some(&dir.join("cwd"))),
        Some(cwd)
    );
}

// Verifies that primary window config can use loaded TOML overrides.
#[test]
fn primary_window_config_can_use_loaded_toml_overrides() {
    let dir = temp_dir("neozeus-config-load");
    let path = dir.join("neozeus.toml");
    std::fs::write(
        &path,
        r#"
        [terminal]
        font_path = "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"

        [window]
        title = "NeoZeus Configured"
        app_id = "neozeus-configured"
        "#,
    )
    .expect("config file should be written");
    let config = load_neozeus_config_from(&path).expect("config should load");

    let window = primary_window_config_for_with_config(
        &AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1600,
            height: 1000,
            scale_factor_override: None,
        },
        &config,
    );

    assert_eq!(window.title, "NeoZeus Configured");
    assert_eq!(window.name.as_deref(), Some("neozeus-configured"));
    assert_eq!(
        resolve_terminal_font_path(&config),
        Some(std::path::PathBuf::from(
            "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"
        ))
    );
}

// Verifies that parses output mode and dimensions.
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

// Verifies that offscreen synthetic window config is hidden and windowed.
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

// Verifies that offscreen mode uses headless runner and no OS primary window.
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

// Verifies that parses optional window scale factor override.
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

// Verifies that parses force fallback adapter override.
#[test]
fn parses_force_fallback_adapter_override() {
    assert!(resolve_force_fallback_adapter(None));
    assert!(resolve_force_fallback_adapter(Some("")));
    assert!(resolve_force_fallback_adapter(Some("true")));
    assert!(resolve_force_fallback_adapter(Some("1")));
    assert!(!resolve_force_fallback_adapter(Some("false")));
    assert!(!resolve_force_fallback_adapter(Some("0")));
    assert!(resolve_force_fallback_adapter_for(
        None,
        OutputMode::Desktop
    ));
    assert!(!resolve_force_fallback_adapter_for(
        None,
        OutputMode::OffscreenVerify
    ));
}

// Verifies that startup focus skips disconnected restored session.
#[test]
fn startup_focus_skips_disconnected_restored_session() {
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
    world.insert_resource(crate::hud::AgentDirectory::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::terminals::TerminalSessionPersistenceState {
        path: Some(sessions_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());

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

// Verifies that startup leaves only disconnected sessions visible and unfocused.
#[test]
fn startup_leaves_only_disconnected_sessions_visible_and_unfocused() {
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
    world.insert_resource(crate::hud::AgentDirectory::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::terminals::TerminalSessionPersistenceState {
        path: Some(sessions_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());

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

// Verifies that startup spawns initial terminal when no sessions exist.
#[test]
fn startup_spawns_initial_terminal_when_no_sessions_exist() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::hud::AgentDirectory::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::terminals::TerminalSessionPersistenceState::default());
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());

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

// Verifies that formats missing GPU startup panics as user facing errors.
#[test]
fn formats_missing_gpu_startup_panics_as_user_facing_errors() {
    let error = format_startup_panic(&"Unable to find a GPU! renderer init failed")
        .expect("missing gpu panic should be formatted");
    assert!(error.contains("could not find a usable graphics adapter"));
    assert!(format_startup_panic(&"some other panic").is_none());
}
