use super::{
    assert_glyph_has_visible_pixels, surface_with_text, temp_dir, test_bridge, FakeTmuxClient,
};
use crate::{
    app_config::{DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX},
    hud::AgentDirectory,
    terminals::{
        blend_rgba_in_place, build_attach_command_argv, compute_terminal_damage,
        create_detached_session_tmux_commands, find_kitty_config_path,
        generate_unique_session_name, initialize_terminal_text_renderer, is_emoji_like,
        is_private_use_like, parse_kitty_config_file, parse_persisted_terminal_sessions,
        pixel_perfect_cell_size, pixel_perfect_terminal_logical_size, poll_terminal_snapshots,
        provision_terminal_target, rasterize_terminal_glyph, reconcile_terminal_sessions,
        resolve_alacritty_color, resolve_terminal_font_report, resolve_terminal_sessions_path_with,
        save_terminal_sessions_if_dirty, send_bytes_tmux_commands,
        serialize_persisted_terminal_sessions, snap_to_pixel_grid, sync_terminal_presentations,
        xterm_indexed_rgb, KittyFontConfig, PersistedTerminalSessions, PresentedTerminal,
        TerminalAttachTarget, TerminalDamage, TerminalDisplayMode, TerminalFontRole,
        TerminalFontState, TerminalFrameUpdate, TerminalGlyphCacheKey, TerminalLifecycle,
        TerminalManager, TerminalPanel, TerminalPresentation, TerminalPresentationStore,
        TerminalProvisionTarget, TerminalRuntimeState, TerminalSessionPersistenceState,
        TerminalSessionRecord, TerminalSurface, TerminalTextRenderer, TerminalTextureState,
        TerminalUpdate, TerminalViewState, TmuxClient, PERSISTENT_TMUX_SESSION_PREFIX,
    },
};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use bevy::{ecs::system::RunSystemOnce, prelude::*, window::PrimaryWindow};
use std::{collections::BTreeSet, fs, time::Duration};

struct UnavailableTmuxClient;

impl TmuxClient for UnavailableTmuxClient {
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
fn pixel_perfect_cell_size_shrinks_native_raster_to_fit_window() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let cell_size = pixel_perfect_cell_size(120, 38, &window);
    assert!(cell_size.x < DEFAULT_CELL_WIDTH_PX);
    assert!(cell_size.y < DEFAULT_CELL_HEIGHT_PX);
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
fn current_host_has_no_user_kitty_config() {
    assert_eq!(find_kitty_config_path(), None);
}

#[test]
fn resolves_effective_terminal_font_stack_on_host() {
    let report = resolve_terminal_font_report().expect("failed to resolve terminal fonts");
    assert_eq!(report.requested_family, "monospace");
    assert_eq!(report.primary.family, "Adwaita Mono");
    assert!(report.primary.path.is_file());
    assert!(report
        .fallbacks
        .iter()
        .any(|face| face.family.contains("Nerd Font")));
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
    let commands = send_bytes_tmux_commands("neozeus-session-a", b"ab\x1b[A\r");
    assert_eq!(
        commands,
        vec![
            vec![
                std::ffi::OsString::from("send-keys"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("=neozeus-session-a:0.0"),
                std::ffi::OsString::from("-l"),
                std::ffi::OsString::from("ab"),
            ],
            vec![
                std::ffi::OsString::from("send-keys"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("=neozeus-session-a:0.0"),
                std::ffi::OsString::from("-H"),
                std::ffi::OsString::from("1b"),
            ],
            vec![
                std::ffi::OsString::from("send-keys"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("=neozeus-session-a:0.0"),
                std::ffi::OsString::from("-l"),
                std::ffi::OsString::from("[A"),
            ],
            vec![
                std::ffi::OsString::from("send-keys"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("=neozeus-session-a:0.0"),
                std::ffi::OsString::from("-H"),
                std::ffi::OsString::from("0d"),
            ],
        ]
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
fn terminal_presentations_stay_hidden_when_no_terminal_is_active() {
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
    assert_eq!(vis, vec![(id, Visibility::Hidden)]);
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
