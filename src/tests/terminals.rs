use super::{
    assert_glyph_has_visible_pixels, fake_runtime_spawner, surface_with_text, temp_dir,
    test_bridge, FakeDaemonClient, FakeTmuxClient,
};
use crate::{
    app_config::{DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX},
    hud::{AgentDirectory, HudModuleId, HudState},
    terminals::{
        active_terminal_viewport, blend_rgba_in_place, build_attach_command_argv,
        compute_terminal_damage, create_detached_session_tmux_commands,
        find_kitty_config_path_with, generate_unique_session_name,
        initialize_terminal_text_renderer, is_emoji_like, is_private_use_like,
        parse_kitty_config_file, parse_persisted_terminal_sessions, pixel_perfect_cell_size,
        pixel_perfect_terminal_logical_size, poll_terminal_snapshots, provision_terminal_target,
        rasterize_terminal_glyph, read_client_message, read_server_message,
        reconcile_terminal_sessions, resolve_alacritty_color, resolve_daemon_socket_path_with,
        resolve_terminal_font_report, resolve_terminal_sessions_path_with,
        save_terminal_sessions_if_dirty, send_bytes_tmux_commands, send_command_payload_bytes,
        serialize_persisted_terminal_sessions, snap_to_pixel_grid, sync_terminal_panel_frames,
        sync_terminal_presentations, write_client_message, write_server_message, xterm_indexed_rgb,
        ClientMessage, DaemonEvent, DaemonRequest, DaemonServerHandle, KittyFontConfig,
        PersistedTerminalSessions, PresentedTerminal, ServerMessage, SocketTerminalDaemonClient,
        TerminalAttachTarget, TerminalCommand, TerminalDaemonClient, TerminalDamage,
        TerminalDisplayMode, TerminalFontRole, TerminalFontState, TerminalFrameUpdate,
        TerminalGlyphCacheKey, TerminalLifecycle, TerminalManager, TerminalPanel,
        TerminalPanelFrame, TerminalPresentation, TerminalPresentationStore,
        TerminalProvisionTarget, TerminalRuntimeState, TerminalSessionClient,
        TerminalSessionPersistenceState, TerminalSessionRecord, TerminalSurface,
        TerminalTextRenderer, TerminalTextureState, TerminalUpdate, TerminalViewState,
        TmuxPaneClient, DAEMON_PROTOCOL_VERSION, PERSISTENT_SESSION_PREFIX,
        PERSISTENT_TMUX_SESSION_PREFIX,
    },
};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use bevy::{ecs::system::RunSystemOnce, prelude::*, window::PrimaryWindow};
use std::{collections::BTreeSet, fs, os::unix::net::UnixStream, sync::Arc, time::Duration};

struct UnavailableTmuxClient;

impl TerminalSessionClient for UnavailableTmuxClient {
    fn ensure_tmux_available(&self) -> Result<(), String> {
        Err("tmux unavailable".into())
    }

    fn create_detached_session(&self, _name: &str) -> Result<(), String> {
        Err("tmux unavailable".into())
    }

    fn list_sessions(&self) -> Result<Vec<String>, String> {
        Err("tmux unavailable".into())
    }

    fn has_session(&self, _name: &str) -> Result<bool, String> {
        Err("tmux unavailable".into())
    }

    fn kill_session(&self, _name: &str) -> Result<(), String> {
        Err("tmux unavailable".into())
    }
}

impl TmuxPaneClient for UnavailableTmuxClient {
    fn list_panes(
        &self,
        _session_name: &str,
    ) -> Result<Vec<crate::terminals::TmuxPaneDescriptor>, String> {
        Err("tmux unavailable".into())
    }

    fn pane_state(&self, _pane_target: &str) -> Result<crate::terminals::TmuxPaneState, String> {
        Err("tmux unavailable".into())
    }

    fn capture_pane(&self, _pane_target: &str, _history_limit: usize) -> Result<String, String> {
        Err("tmux unavailable".into())
    }

    fn send_bytes(&self, _pane_target: &str, _bytes: &[u8]) -> Result<(), String> {
        Err("tmux unavailable".into())
    }
}

#[test]
fn indexed_color_has_expected_blue_cube_entry() {
    let rgb = xterm_indexed_rgb(21);
    assert_eq!((rgb.r, rgb.g, rgb.b), (0, 0, 255));
}

#[test]
fn alpha_blend_preserves_transparent_glyph_background() {
    let mut pixel = [0, 0, 0, 0];
    blend_rgba_in_place(&mut pixel, [255, 255, 255, 0]);
    assert_eq!(pixel, [0, 0, 0, 0]);

    blend_rgba_in_place(&mut pixel, [255, 255, 255, 128]);
    assert_eq!(pixel[3], 128);
}

#[test]
fn pixel_perfect_cell_size_does_not_exceed_native_size() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let cell_size = pixel_perfect_cell_size(120, 38, &window, &hud_state);
    assert!(cell_size.x <= DEFAULT_CELL_WIDTH_PX);
    assert!(cell_size.y <= DEFAULT_CELL_HEIGHT_PX);
    assert!(cell_size.x >= 1);
    assert!(cell_size.y >= 1);
}

#[test]
fn snap_to_pixel_grid_respects_window_scale_factor() {
    let mut window = Window::default();
    window.resolution.set_scale_factor_override(Some(1.5));
    let snapped = snap_to_pixel_grid(Vec2::new(10.2, -3.4), &window);
    assert_eq!(snapped, Vec2::new(10.0, -10.0 / 3.0));
}

#[test]
fn pixel_perfect_terminal_logical_size_uses_scale_factor() {
    let mut window = Window::default();
    window.resolution.set_scale_factor_override(Some(2.0));
    let texture_state = TerminalTextureState {
        texture_size: UVec2::new(200, 120),
        ..Default::default()
    };
    assert_eq!(
        pixel_perfect_terminal_logical_size(&texture_state, &window),
        Vec2::new(100.0, 60.0)
    );
}

#[test]
fn active_terminal_viewport_reserves_agent_list_column() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let rect = crate::hud::docked_agent_list_rect(&window);
    let agent_list = hud_state.get_mut(HudModuleId::AgentList).unwrap();
    agent_list.shell.enabled = true;
    agent_list.shell.current_rect = rect;
    agent_list.shell.target_rect = rect;

    assert_eq!(
        active_terminal_viewport(&window, &hud_state),
        (Vec2::new(1100.0, 900.0), Vec2::new(150.0, 0.0))
    );
}

#[test]
fn active_terminal_presentation_fills_remaining_viewport_exactly() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(120, 38));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(1200, 760),
                cell_size: UVec2::new(10, 20),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let rect = crate::hud::docked_agent_list_rect(&window);
    let agent_list = hud_state.get_mut(HudModuleId::AgentList).unwrap();
    agent_list.shell.enabled = true;
    agent_list.shell.current_rect = rect;
    agent_list.shell.target_rect = rect;

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(hud_state);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPresentation, &Transform)>();
    let (presentation, transform) = query.single(&world).unwrap();
    assert!(presentation.current_size.distance(Vec2::new(1100.0, 900.0)) < 0.2);
    assert!(
        presentation
            .current_position
            .distance(Vec2::new(150.0, 0.0))
            < 0.2
    );
    assert!((transform.translation.x - 150.0).abs() < 0.2);
    assert!(transform.translation.y.abs() < 0.2);
    assert!((transform.translation.z - 0.3).abs() < 0.01);
}

#[test]
fn compute_terminal_damage_marks_only_changed_rows() {
    let previous = surface_with_text(3, 4, 1, "ab");
    let next = surface_with_text(3, 4, 2, "cd");
    assert_eq!(
        compute_terminal_damage(Some(&previous), &next),
        TerminalDamage::Rows(vec![1, 2])
    );
}

#[test]
fn compute_terminal_damage_marks_resize_as_full() {
    let previous = TerminalSurface::new(4, 3);
    let next = TerminalSurface::new(5, 3);
    assert_eq!(
        compute_terminal_damage(Some(&previous), &next),
        TerminalDamage::Full
    );
}

#[test]
fn drain_terminal_updates_keeps_latest_frame_and_status() {
    let mailbox = crate::terminals::TerminalUpdateMailbox::default();

    assert!(
        mailbox
            .push(TerminalUpdate::Frame(TerminalFrameUpdate {
                surface: surface_with_text(2, 2, 0, "a"),
                damage: TerminalDamage::Rows(vec![0]),
                runtime: TerminalRuntimeState::running("one"),
            }))
            .should_wake
    );
    assert!(
        !mailbox
            .push(TerminalUpdate::Frame(TerminalFrameUpdate {
                surface: surface_with_text(2, 2, 1, "b"),
                damage: TerminalDamage::Rows(vec![1]),
                runtime: TerminalRuntimeState::running("two"),
            }))
            .should_wake
    );
    assert!(
        !mailbox
            .push(TerminalUpdate::Status {
                runtime: TerminalRuntimeState::running("done"),
                surface: None,
            })
            .should_wake
    );

    let (frame, status, dropped) = mailbox.drain();
    assert_eq!(dropped, 1);
    assert_eq!(frame.unwrap().runtime.status, "two");
    assert_eq!(status.unwrap().0.status, "done");
}

#[test]
fn poll_terminal_snapshots_keeps_latest_status_over_latest_frame_runtime() {
    let (bridge, mailbox) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    mailbox.push(TerminalUpdate::Frame(TerminalFrameUpdate {
        surface: surface_with_text(2, 2, 0, "a"),
        damage: TerminalDamage::Rows(vec![0]),
        runtime: TerminalRuntimeState::running("running"),
    }));
    mailbox.push(TerminalUpdate::Status {
        runtime: TerminalRuntimeState::failed("boom"),
        surface: None,
    });

    let mut world = World::default();
    world.insert_resource(manager);
    world.run_system_once(poll_terminal_snapshots).unwrap();
    let manager = world.resource::<TerminalManager>();
    let terminal = manager.get(terminal_id).unwrap();
    assert_eq!(terminal.snapshot.runtime.status, "boom");
    assert!(matches!(
        terminal.snapshot.runtime.lifecycle,
        TerminalLifecycle::Failed
    ));
}

#[test]
fn named_cursor_color_resolves() {
    let color = resolve_alacritty_color(
        AnsiColor::Named(NamedColor::Cursor),
        &Default::default(),
        true,
    );
    assert_eq!((color.r(), color.g(), color.b()), (82, 173, 112));
}

#[test]
fn parses_font_family_from_included_kitty_config() {
    let dir = temp_dir("neozeus-kitty-font-test");
    let main = dir.join("kitty.conf");
    let included = dir.join("fonts.conf");
    fs::write(&included, "font_family JetBrains Mono Nerd Font\n")
        .expect("failed to write include config");
    fs::write(&main, "include fonts.conf\n").expect("failed to write main config");

    let mut visited = BTreeSet::new();
    let mut config = KittyFontConfig::default();
    parse_kitty_config_file(&main, &mut visited, &mut config)
        .expect("failed to parse kitty config");

    assert_eq!(
        config.font_family.as_deref(),
        Some("JetBrains Mono Nerd Font")
    );
}

#[test]
fn kitty_config_lookup_prefers_explicit_directory_over_other_locations() {
    let dir = temp_dir("neozeus-kitty-config-path");
    let kitty_dir = dir.join("kitty-dir");
    let xdg_dir = dir.join("xdg");
    let home_dir = dir.join("home");
    fs::create_dir_all(&kitty_dir).expect("failed to create kitty dir");
    fs::create_dir_all(xdg_dir.join("kitty")).expect("failed to create xdg kitty dir");
    fs::create_dir_all(home_dir.join(".config/kitty")).expect("failed to create home kitty dir");
    fs::write(kitty_dir.join("kitty.conf"), "font_family Fira Code\n")
        .expect("failed to write kitty config");
    fs::write(xdg_dir.join("kitty/kitty.conf"), "font_family Hack\n")
        .expect("failed to write xdg kitty config");
    fs::write(
        home_dir.join(".config/kitty/kitty.conf"),
        "font_family Iosevka\n",
    )
    .expect("failed to write home kitty config");

    let found = find_kitty_config_path_with(
        Some(kitty_dir.as_os_str()),
        Some(xdg_dir.as_os_str()),
        Some(home_dir.as_os_str()),
        None,
        None,
    );
    assert_eq!(found, Some(kitty_dir.join("kitty.conf")));
}

#[test]
fn resolves_effective_terminal_font_stack_on_host() {
    let report = resolve_terminal_font_report().expect("failed to resolve terminal fonts");
    assert_eq!(report.requested_family, "monospace");
    assert!(report.primary.path.is_file());
    assert!(!report.primary.family.is_empty());
    assert!(!report.fallbacks.is_empty());
    assert!(report.fallbacks.iter().all(|face| face.path.is_file()));
}

#[test]
fn detects_special_font_ranges() {
    assert!(is_private_use_like('\u{e0b0}'));
    assert!(is_emoji_like('🚀'));
    assert!(!is_private_use_like('a'));
}

#[test]
fn standalone_text_renderer_rasterizes_ascii_glyph() {
    let report = resolve_terminal_font_report().expect("failed to resolve terminal fonts");
    let mut renderer = TerminalTextRenderer::default();
    initialize_terminal_text_renderer(&report, &mut renderer)
        .expect("failed to initialize terminal text renderer");
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
    };
    let glyph = rasterize_terminal_glyph(
        &TerminalGlyphCacheKey {
            content: crate::terminals::TerminalCellContent::Single('A'),
            font_role: TerminalFontRole::Primary,
            width_cells: 1,
            cell_width: 14,
            cell_height: 24,
        },
        TerminalFontRole::Primary,
        false,
        &mut renderer,
        &font_state,
    );
    assert_glyph_has_visible_pixels(&glyph);
}

#[test]
fn terminal_sessions_path_prefers_state_home_then_home_state_then_config() {
    assert_eq!(
        resolve_terminal_sessions_path_with(
            Some("/tmp/state"),
            Some("/tmp/home"),
            Some("/tmp/config")
        ),
        Some(std::path::PathBuf::from("/tmp/state/neozeus/terminals.v1"))
    );
    assert_eq!(
        resolve_terminal_sessions_path_with(None, Some("/tmp/home"), Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/home/.local/state/neozeus/terminals.v1"
        ))
    );
    assert_eq!(
        resolve_terminal_sessions_path_with(None, None, Some("/tmp/config")),
        Some(std::path::PathBuf::from("/tmp/config/neozeus/terminals.v1"))
    );
}

#[test]
fn terminal_sessions_parse_and_serialize_roundtrip() {
    let persisted = PersistedTerminalSessions {
        sessions: vec![
            TerminalSessionRecord {
                session_name: "neozeus-session-a".into(),
                label: Some("agent 1".into()),
                creation_index: 0,
                last_focused: true,
            },
            TerminalSessionRecord {
                session_name: "neozeus-session-b".into(),
                label: None,
                creation_index: 1,
                last_focused: false,
            },
        ],
    };

    let serialized = serialize_persisted_terminal_sessions(&persisted);
    assert_eq!(parse_persisted_terminal_sessions(&serialized), persisted);
}

#[test]
fn malformed_terminal_sessions_version_falls_back_to_default() {
    assert_eq!(
        parse_persisted_terminal_sessions(
            "version 99\nsession name=a creation_index=0 focused=1\n"
        ),
        PersistedTerminalSessions::default()
    );
}

#[test]
fn reconcile_terminal_sessions_restores_prunes_and_imports() {
    let persisted = PersistedTerminalSessions {
        sessions: vec![
            TerminalSessionRecord {
                session_name: "neozeus-session-a".into(),
                label: Some("one".into()),
                creation_index: 0,
                last_focused: true,
            },
            TerminalSessionRecord {
                session_name: "neozeus-session-b".into(),
                label: None,
                creation_index: 1,
                last_focused: false,
            },
        ],
    };

    let reconciled = reconcile_terminal_sessions(
        &persisted,
        &[
            "neozeus-session-a".into(),
            "neozeus-session-c".into(),
            "neozeus-verifier-x".into(),
        ],
    );

    assert_eq!(reconciled.restore.len(), 1);
    assert_eq!(reconciled.restore[0].session_name, "neozeus-session-a");
    assert_eq!(reconciled.prune.len(), 1);
    assert_eq!(reconciled.prune[0].session_name, "neozeus-session-b");
    assert_eq!(reconciled.import.len(), 1);
    assert_eq!(reconciled.import[0].session_name, "neozeus-session-c");
    assert_eq!(reconciled.import[0].creation_index, 2);
}

#[test]
fn saving_terminal_sessions_persists_focus_order_and_labels() {
    let dir = temp_dir("neozeus-terminal-sessions-save");
    let path = dir.join("terminals.v1");
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_two);

    let mut directory = AgentDirectory::default();
    directory.labels.insert(id_one, "oracle one".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(manager);
    world.insert_resource(directory);
    world.insert_resource(TerminalSessionPersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world
        .run_system_once(save_terminal_sessions_if_dirty)
        .unwrap();
    let serialized = fs::read_to_string(&path).expect("terminal sessions file missing");
    let persisted = parse_persisted_terminal_sessions(&serialized);
    assert_eq!(persisted.sessions.len(), 2);
    assert_eq!(persisted.sessions[0].session_name, "neozeus-session-a");
    assert_eq!(persisted.sessions[0].label.as_deref(), Some("oracle one"));
    assert!(!persisted.sessions[0].last_focused);
    assert_eq!(persisted.sessions[1].session_name, "neozeus-session-b");
    assert!(persisted.sessions[1].last_focused);
}

#[test]
fn terminal_sessions_save_waits_for_debounce_window() {
    let dir = temp_dir("neozeus-terminal-sessions-save-debounce");
    let path = dir.join("terminals.v1");
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal_with_session(bridge, "neozeus-session-a".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(100));
    world.insert_resource(time);
    world.insert_resource(manager);
    world.insert_resource(AgentDirectory::default());
    world.insert_resource(TerminalSessionPersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world
        .run_system_once(save_terminal_sessions_if_dirty)
        .unwrap();
    assert!(!path.exists(), "debounced save should not run yet");

    world
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_millis(300));
    world
        .run_system_once(save_terminal_sessions_if_dirty)
        .unwrap();
    assert!(path.exists(), "save should run after debounce window");
}

#[test]
fn create_detached_session_tmux_commands_keep_sessions_alive_when_unattached() {
    let commands = create_detached_session_tmux_commands("neozeus-session-a");
    assert_eq!(
        commands,
        vec![
            vec![
                std::ffi::OsString::from("new-session"),
                std::ffi::OsString::from("-d"),
                std::ffi::OsString::from("-x"),
                std::ffi::OsString::from("120"),
                std::ffi::OsString::from("-y"),
                std::ffi::OsString::from("38"),
                std::ffi::OsString::from("-s"),
                std::ffi::OsString::from("neozeus-session-a"),
            ],
            vec![
                std::ffi::OsString::from("set-option"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("neozeus-session-a"),
                std::ffi::OsString::from("destroy-unattached"),
                std::ffi::OsString::from("off"),
            ],
            vec![
                std::ffi::OsString::from("set-option"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("neozeus-session-a"),
                std::ffi::OsString::from("status"),
                std::ffi::OsString::from("off"),
            ],
        ]
    );
}

#[test]
fn send_bytes_tmux_commands_split_control_and_literal_sequences() {
    let commands = send_bytes_tmux_commands("%42", b"ab\x1b[A\r");
    assert_eq!(
        commands,
        vec![
            vec![
                std::ffi::OsString::from("send-keys"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("%42"),
                std::ffi::OsString::from("-l"),
                std::ffi::OsString::from("ab"),
            ],
            vec![
                std::ffi::OsString::from("send-keys"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("%42"),
                std::ffi::OsString::from("-H"),
                std::ffi::OsString::from("1b"),
            ],
            vec![
                std::ffi::OsString::from("send-keys"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("%42"),
                std::ffi::OsString::from("-l"),
                std::ffi::OsString::from("[A"),
            ],
            vec![
                std::ffi::OsString::from("send-keys"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("%42"),
                std::ffi::OsString::from("-H"),
                std::ffi::OsString::from("0d"),
            ],
        ]
    );
}

#[test]
fn send_command_payload_bytes_turn_multiline_text_into_enter_sequences() {
    assert_eq!(
        send_command_payload_bytes("echo hi\npwd"),
        b"echo hi\rpwd\r"
    );
    assert_eq!(
        send_command_payload_bytes("echo hi\r\npwd"),
        b"echo hi\rpwd\r"
    );
}

#[test]
fn build_attach_command_argv_uses_tmux_attach_for_tmux_target() {
    let (program, args) = build_attach_command_argv(&TerminalAttachTarget::TmuxAttach {
        session_name: "neozeus-session-a".into(),
    });
    assert_eq!(program, std::ffi::OsString::from("tmux"));
    assert_eq!(
        args,
        vec![
            std::ffi::OsString::from("attach-session"),
            std::ffi::OsString::from("-t"),
            std::ffi::OsString::from("neozeus-session-a"),
        ]
    );
}

#[test]
fn build_attach_command_argv_uses_shell_for_raw_target() {
    let (program, args) = build_attach_command_argv(&TerminalAttachTarget::RawShell);
    assert!(!program.is_empty());
    assert!(args.is_empty());
}

#[test]
fn provision_terminal_target_creates_detached_tmux_session() {
    let client = FakeTmuxClient::default();
    let target = TerminalProvisionTarget::TmuxDetached {
        session_name: "neozeus-session-a".into(),
    };
    provision_terminal_target(&client, &target).expect("tmux provision should succeed");
    assert_eq!(
        client
            .list_sessions()
            .expect("list sessions should succeed"),
        vec!["neozeus-session-a".to_owned()]
    );
}

#[test]
fn generate_unique_session_name_retries_collisions() {
    let client = FakeTmuxClient::with_collisions(2);
    let name = generate_unique_session_name(&client, PERSISTENT_TMUX_SESSION_PREFIX)
        .expect("session name should be generated");
    assert!(name.starts_with(PERSISTENT_TMUX_SESSION_PREFIX));
}

#[test]
fn provision_terminal_target_reports_tmux_unavailable() {
    let error = provision_terminal_target(
        &UnavailableTmuxClient,
        &TerminalProvisionTarget::TmuxDetached {
            session_name: "neozeus-session-a".into(),
        },
    )
    .expect_err("tmux provisioning should fail");
    assert!(error.contains("tmux unavailable"));
}

#[test]
fn terminal_view_state_restores_offsets_per_terminal() {
    let id_one = crate::terminals::TerminalId(1);
    let id_two = crate::terminals::TerminalId(2);
    let mut view_state = TerminalViewState::default();

    view_state.apply_offset_delta(Some(id_one), Vec2::new(120.0, -30.0));
    assert_eq!(view_state.offset, Vec2::new(120.0, -30.0));

    view_state.focus_terminal(Some(id_two));
    assert_eq!(view_state.offset, Vec2::ZERO);

    view_state.apply_offset_delta(Some(id_two), Vec2::new(-48.0, 64.0));
    assert_eq!(view_state.offset, Vec2::new(-48.0, 64.0));

    view_state.focus_terminal(Some(id_one));
    assert_eq!(view_state.offset, Vec2::new(120.0, -30.0));

    view_state.focus_terminal(Some(id_two));
    assert_eq!(view_state.offset, Vec2::new(-48.0, 64.0));
}

#[test]
fn terminal_creation_order_stays_stable_when_focus_changes() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

    manager.focus_terminal(id_one);

    assert_eq!(manager.terminal_ids(), &[id_one, id_two]);
    assert_eq!(manager.focus_order(), &[id_two, id_one]);
}

#[test]
fn terminal_can_be_created_without_becoming_active() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);

    assert_eq!(manager.terminal_ids(), &[id]);
    assert_eq!(manager.active_id(), None);
    assert!(manager.focus_order().is_empty());
}

#[test]
fn terminal_with_session_name_is_retained_in_manager_state() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());

    assert_eq!(manager.get(id).unwrap().session_name, "neozeus-session-a");
}

#[test]
fn remove_terminal_clears_orders_and_active_state() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_one);

    let removed = manager
        .remove_terminal(id_one)
        .expect("terminal should exist");

    assert_eq!(removed.session_name, "neozeus-session-a");
    assert_eq!(manager.active_id(), None);
    assert_eq!(manager.terminal_ids(), &[id_two]);
    assert_eq!(manager.focus_order(), &[id_two]);
}

#[test]
fn show_all_presentations_remain_visible_when_no_terminal_is_active() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(2, 2));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    world.insert_resource(manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(crate::hud::HudState::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query
        .iter(&world)
        .map(|(panel, visibility)| (panel.id, *visibility))
        .collect::<Vec<_>>();
    assert_eq!(vis, vec![(id, Visibility::Visible)]);
}

#[test]
fn terminal_panel_frames_are_hidden_without_direct_input_mode() {
    let mut world = World::default();
    world.insert_resource(crate::hud::HudState::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.spawn((
        TerminalPanelFrame {
            id: crate::terminals::TerminalId(1),
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_panel_frames).unwrap();

    let mut query = world.query::<(&TerminalPanelFrame, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Hidden);
}

#[test]
fn direct_input_mode_shows_orange_terminal_frame() {
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);

    let mut world = World::default();
    world.insert_resource(hud_state);
    let panel_entity = world
        .spawn((
            TerminalPanel { id: terminal_id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::new(30.0, -20.0),
                target_position: Vec2::ZERO,
                current_size: Vec2::new(320.0, 180.0),
                target_size: Vec2::ZERO,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.5,
                target_z: 0.0,
            },
            Visibility::Visible,
        ))
        .id();
    let frame_entity = world
        .spawn((
            TerminalPanelFrame { id: terminal_id },
            Transform::default(),
            Sprite::default(),
            Visibility::Hidden,
        ))
        .id();
    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        terminal_id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            panel_entity,
            frame_entity,
        },
    );
    world.insert_resource(presentation_store);

    world.run_system_once(sync_terminal_panel_frames).unwrap();

    let mut query = world.query::<(&TerminalPanelFrame, &Transform, &Sprite, &Visibility)>();
    let frames = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(frames.len(), 1);
    assert_eq!(*frames[0].3, Visibility::Visible);
    assert_eq!(frames[0].1.translation, Vec3::new(30.0, -20.0, 0.48));
    assert_eq!(frames[0].2.custom_size, Some(Vec2::new(320.0, 180.0)));
}

#[test]
fn message_box_keeps_terminal_presentations_visible() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(2, 2));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_message_box(id);
    world.insert_resource(time);
    world.insert_resource(manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(hud_state);
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Visible);
}

#[test]
fn isolate_visibility_policy_with_missing_terminal_degrades_to_show_all() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(2, 2));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    world.insert_resource(manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::Isolate(crate::terminals::TerminalId(999)),
    });
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(crate::hud::HudState::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Visible);
}

#[test]
fn terminal_visibility_policy_hides_only_presentation_and_show_all_restores_it() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(2, 2));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    for id in [id_one, id_two] {
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: UVec2::new(100, 100),
                    cell_size: UVec2::new(10, 20),
                },
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    world.insert_resource(manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::Isolate(id_one),
    });
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(crate::hud::HudState::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world.spawn((
        TerminalPanel { id: id_one },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));
    world.spawn((
        TerminalPanel { id: id_two },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();
    {
        let manager = world.resource::<TerminalManager>();
        assert_eq!(manager.terminal_ids().len(), 2);
    }
    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let mut vis = query
        .iter(&world)
        .map(|(panel, visibility)| (panel.id, *visibility))
        .collect::<Vec<_>>();
    vis.sort_by_key(|(id, _)| id.0);
    assert_eq!(vis[0], (id_one, Visibility::Visible));
    assert_eq!(vis[1], (id_two, Visibility::Hidden));

    world
        .resource_mut::<crate::hud::TerminalVisibilityState>()
        .policy = crate::hud::TerminalVisibilityPolicy::ShowAll;
    world.run_system_once(sync_terminal_presentations).unwrap();
    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let mut vis = query
        .iter(&world)
        .map(|(panel, visibility)| (panel.id, *visibility))
        .collect::<Vec<_>>();
    vis.sort_by_key(|(id, _)| id.0);
    assert_eq!(vis[0], (id_one, Visibility::Visible));
    assert_eq!(vis[1], (id_two, Visibility::Visible));
}

fn start_test_daemon(prefix: &str) -> (DaemonServerHandle, std::path::PathBuf) {
    let dir = temp_dir(prefix);
    let socket_path = dir.join("daemon.sock");
    let handle = DaemonServerHandle::start(socket_path.clone()).expect("daemon should start");
    (handle, socket_path)
}

fn surface_to_text(surface: &TerminalSurface) -> String {
    let mut text = String::new();
    for y in 0..surface.rows {
        if y > 0 {
            text.push('\n');
        }
        for x in 0..surface.cols {
            text.push_str(&surface.cell(x, y).content.to_owned_string());
        }
    }
    text
}

fn wait_for_surface_containing(
    updates: &std::sync::mpsc::Receiver<TerminalUpdate>,
    needle: &str,
) -> TerminalSurface {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("timed out waiting for daemon update");
        let update = updates
            .recv_timeout(remaining)
            .expect("timed out waiting for daemon update");
        let surface = match update {
            TerminalUpdate::Frame(frame) => frame.surface,
            TerminalUpdate::Status {
                surface: Some(surface),
                ..
            } => surface,
            TerminalUpdate::Status { .. } => continue,
        };
        if surface_to_text(&surface).contains(needle) {
            return surface;
        }
    }
}

#[test]
fn daemon_socket_path_prefers_xdg_runtime_then_tmp_user() {
    let path = resolve_daemon_socket_path_with(
        Some("/run/user/1000"),
        Some("/home/alvatar"),
        Some("oracle"),
    )
    .expect("xdg runtime path should resolve");
    assert_eq!(
        path,
        std::path::PathBuf::from("/run/user/1000/neozeus/daemon.sock")
    );

    let fallback = resolve_daemon_socket_path_with(None, Some("/home/alvatar"), Some("oracle"))
        .expect("tmp fallback should resolve");
    assert!(fallback.ends_with("neozeus-oracle/daemon.sock"));
}

#[test]
fn daemon_protocol_roundtrip_preserves_terminal_messages() {
    let message = ClientMessage::Request {
        request_id: 7,
        request: DaemonRequest::SendCommand {
            session_id: "neozeus-session-7".into(),
            command: TerminalCommand::SendCommand("printf 'hi'".into()),
        },
    };
    let mut bytes = Vec::new();
    write_client_message(&mut bytes, &message).expect("client message should encode");
    let decoded = read_client_message(&mut bytes.as_slice()).expect("client message should decode");
    assert_eq!(decoded, message);

    let mut surface = TerminalSurface::new(3, 1);
    surface.set_text_cell(0, 0, "h");
    surface.set_text_cell(1, 0, "i");
    let response = ServerMessage::Event(DaemonEvent::SessionUpdated {
        session_id: "neozeus-session-7".into(),
        update: TerminalUpdate::Status {
            runtime: TerminalRuntimeState::running("daemon"),
            surface: Some(surface.clone()),
        },
        revision: 9,
    });
    let mut server_bytes = Vec::new();
    write_server_message(&mut server_bytes, &response).expect("server message should encode");
    let decoded =
        read_server_message(&mut server_bytes.as_slice()).expect("server message should decode");
    assert_eq!(decoded, response);
}

#[test]
fn daemon_server_cleans_up_stale_socket_file() {
    let dir = temp_dir("neozeus-daemon-stale-socket");
    let socket_path = dir.join("daemon.sock");
    fs::write(&socket_path, b"stale").expect("failed to write stale socket file");

    let _server =
        DaemonServerHandle::start(socket_path.clone()).expect("server should replace stale socket");
    let _client = SocketTerminalDaemonClient::connect(&socket_path)
        .expect("client should connect after stale cleanup");
}

#[test]
fn daemon_handshake_rejects_protocol_mismatch() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-mismatch");
    let mut stream =
        UnixStream::connect(&socket_path).expect("raw daemon socket connect should succeed");
    let request = ClientMessage::Request {
        request_id: 1,
        request: DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION + 1,
        },
    };
    write_client_message(&mut stream, &request).expect("handshake request should write");
    let response = read_server_message(&mut stream).expect("handshake response should read");
    match response {
        ServerMessage::Response { response, .. } => {
            let error = response.expect_err("mismatched handshake should fail");
            assert!(error.contains("protocol version mismatch"));
        }
        other => panic!("unexpected handshake response: {other:?}"),
    }
}

#[test]
fn daemon_create_attach_command_output_and_kill_roundtrip() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-roundtrip");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    let session_id = client
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("daemon session should be created");
    let sessions = client.list_sessions().expect("daemon sessions should list");
    assert!(sessions
        .iter()
        .any(|session| session.session_id == session_id));

    let attached = client
        .attach_session(&session_id)
        .expect("daemon session should attach");
    assert!(attached.snapshot.surface.is_some());

    client
        .send_command(
            &session_id,
            TerminalCommand::SendCommand("printf 'neozeus-daemon-ok'".into()),
        )
        .expect("daemon command should send");
    let surface = wait_for_surface_containing(&attached.updates, "neozeus-daemon-ok");
    assert!(surface_to_text(&surface).contains("neozeus-daemon-ok"));

    client
        .kill_session(&session_id)
        .expect("daemon session should kill");
    let sessions = client
        .list_sessions()
        .expect("daemon sessions should relist");
    assert!(!sessions
        .iter()
        .any(|session| session.session_id == session_id));
}

#[test]
fn daemon_sessions_survive_client_reconnect() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-reconnect");
    let client_a =
        SocketTerminalDaemonClient::connect(&socket_path).expect("first client should connect");
    let session_id = client_a
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("daemon session should be created");
    let attached_a = client_a
        .attach_session(&session_id)
        .expect("first client should attach");
    client_a
        .send_command(
            &session_id,
            TerminalCommand::SendCommand("printf 'persist-across-ui'".into()),
        )
        .expect("first client command should send");
    let _ = wait_for_surface_containing(&attached_a.updates, "persist-across-ui");
    drop(client_a);

    let client_b =
        SocketTerminalDaemonClient::connect(&socket_path).expect("second client should connect");
    let sessions = client_b
        .list_sessions()
        .expect("sessions should still exist after reconnect");
    assert!(sessions
        .iter()
        .any(|session| session.session_id == session_id));
    let attached_b = client_b
        .attach_session(&session_id)
        .expect("second client should reattach");
    let snapshot = attached_b
        .snapshot
        .surface
        .expect("reattach snapshot should include surface");
    assert!(surface_to_text(&snapshot).contains("persist-across-ui"));
}

#[test]
fn daemon_runtime_bridge_pushes_initial_snapshot_and_forwards_commands() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-1".into());
    let spawner = fake_runtime_spawner(client.clone());
    let bridge = spawner
        .spawn_attached("neozeus-session-1")
        .expect("daemon bridge should attach");

    let (frame, status, _) = bridge.drain_updates();
    assert!(frame.is_none());
    assert!(status.is_some());
    bridge.send(TerminalCommand::SendCommand("pwd".into()));
    std::thread::sleep(Duration::from_millis(20));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert!(commands.iter().any(|(session_id, command)| {
        session_id == "neozeus-session-1"
            && matches!(command, TerminalCommand::SendCommand(value) if value == "pwd")
    }));
}

#[test]
fn daemon_resize_session_request_succeeds() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-resize");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    let session_id = client
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("daemon session should be created");
    client
        .resize_session(&session_id, 100, 30)
        .expect("daemon resize should succeed");
}

#[test]
fn daemon_runtime_bridge_applies_streamed_updates() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-2".into());
    let spawner = fake_runtime_spawner(client.clone());
    let bridge = spawner
        .spawn_attached("neozeus-session-2")
        .expect("daemon bridge should attach");
    client.emit_update(
        "neozeus-session-2",
        TerminalUpdate::Status {
            runtime: TerminalRuntimeState::running("fake daemon streamed"),
            surface: Some(surface_with_text(1, 4, 0, "ok")),
        },
    );
    std::thread::sleep(Duration::from_millis(20));
    let (_, status, _) = bridge.drain_updates();
    let surface = status
        .expect("bridge should receive streamed update")
        .1
        .expect("streamed status should carry surface");
    assert!(surface_to_text(&surface).contains("ok"));
}

#[test]
fn daemon_attach_missing_session_returns_error() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-missing-attach");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    let error = client
        .attach_session("neozeus-session-missing")
        .expect_err("missing daemon session attach should fail");
    assert!(error.contains("not found"));
}

#[test]
fn daemon_kill_missing_session_returns_error() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-missing-kill");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    let error = client
        .kill_session("neozeus-session-missing")
        .expect_err("missing daemon session kill should fail");
    assert!(error.contains("not found"));
}

#[test]
fn daemon_multiple_clients_receive_updates_for_same_session() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-multi-client");
    let client_a =
        SocketTerminalDaemonClient::connect(&socket_path).expect("first client should connect");
    let session_id = client_a
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("daemon session should be created");
    let attached_a = client_a
        .attach_session(&session_id)
        .expect("first client should attach");

    let client_b =
        SocketTerminalDaemonClient::connect(&socket_path).expect("second client should connect");
    let attached_b = client_b
        .attach_session(&session_id)
        .expect("second client should attach");

    client_a
        .send_command(
            &session_id,
            TerminalCommand::SendCommand("printf 'fanout'".into()),
        )
        .expect("daemon command should send");

    let surface_a = wait_for_surface_containing(&attached_a.updates, "fanout");
    let surface_b = wait_for_surface_containing(&attached_b.updates, "fanout");
    assert!(surface_to_text(&surface_a).contains("fanout"));
    assert!(surface_to_text(&surface_b).contains("fanout"));
}

fn wait_for_surface_dimensions(
    updates: &std::sync::mpsc::Receiver<TerminalUpdate>,
    cols: usize,
    rows: usize,
) -> TerminalSurface {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("timed out waiting for resized surface");
        let update = updates
            .recv_timeout(remaining)
            .expect("timed out waiting for resized surface");
        let surface = match update {
            TerminalUpdate::Frame(frame) => frame.surface,
            TerminalUpdate::Status {
                surface: Some(surface),
                ..
            } => surface,
            TerminalUpdate::Status { .. } => continue,
        };
        if surface.cols == cols && surface.rows == rows {
            return surface;
        }
    }
}

#[test]
fn daemon_protocol_rejects_truncated_frame() {
    let bytes = vec![8, 0, 0, 0, 1, 2, 3];
    let error = read_client_message(&mut bytes.as_slice())
        .expect_err("truncated protocol frame should fail");
    assert!(error.contains("frame payload") || error.contains("truncated"));
}

#[test]
fn daemon_protocol_rejects_trailing_bytes_in_frame() {
    let message = ClientMessage::Request {
        request_id: 11,
        request: DaemonRequest::ListSessions,
    };
    let mut bytes = Vec::new();
    write_client_message(&mut bytes, &message).expect("client message should encode");
    let original_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let payload = &bytes[4..4 + original_len];
    let mut corrupted = Vec::new();
    corrupted.extend_from_slice(&((original_len + 1) as u32).to_le_bytes());
    corrupted.extend_from_slice(payload);
    corrupted.push(0xff);
    let error = read_client_message(&mut corrupted.as_slice())
        .expect_err("protocol frame with trailing payload bytes should fail");
    assert!(error.contains("trailing bytes"));
}

#[test]
fn daemon_resize_session_updates_attached_surface_dimensions() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-resize-surface");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    let session_id = client
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("daemon session should be created");
    let attached = client
        .attach_session(&session_id)
        .expect("daemon session should attach");
    client
        .resize_session(&session_id, 100, 30)
        .expect("daemon resize should succeed");
    let surface = wait_for_surface_dimensions(&attached.updates, 100, 30);
    assert_eq!((surface.cols, surface.rows), (100, 30));
}

#[test]
fn daemon_duplicate_attach_in_same_client_is_rejected() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-duplicate-attach");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    let session_id = client
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("daemon session should be created");
    let _attached = client
        .attach_session(&session_id)
        .expect("first attach should succeed");
    let error = client
        .attach_session(&session_id)
        .expect_err("duplicate attach in same client should fail");
    assert!(error.contains("already attached"));
}

#[test]
fn daemon_killing_one_session_preserves_other_sessions() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-isolated-kill");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    let first = client
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("first daemon session should be created");
    let second = client
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("second daemon session should be created");
    client
        .kill_session(&first)
        .expect("first session should kill");
    let sessions = client
        .list_sessions()
        .expect("sessions should list after kill");
    assert!(!sessions.iter().any(|session| session.session_id == first));
    assert!(sessions.iter().any(|session| session.session_id == second));
}

#[test]
fn daemon_session_lifecycle_churn_stays_consistent() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-churn");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    for _ in 0..5 {
        let session_id = client
            .create_session(PERSISTENT_SESSION_PREFIX)
            .expect("daemon session should be created during churn");
        let _attached = client
            .attach_session(&session_id)
            .expect("daemon session should attach during churn");
        client
            .kill_session(&session_id)
            .expect("daemon session should kill during churn");
    }
    let sessions = client
        .list_sessions()
        .expect("sessions should list after churn");
    assert!(sessions.is_empty());
}
