use crate::{
    app_config::{DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX},
    hud::{
        agent_rows, apply_persisted_layout, debug_toolbar_buttons, dispatch_hud_pointer_click,
        dispatch_hud_scroll, handle_hud_pointer_input, hud_needs_redraw, parse_persisted_hud_state,
        resolve_agent_label, resolve_hud_layout_path_with, save_hud_layout_if_dirty,
        serialize_persisted_hud_state, AgentDirectory, HudDispatcher, HudModuleId, HudRect,
        HudState, PersistedHudModuleState, PersistedHudState, TerminalVisibilityPolicy,
        TerminalVisibilityState,
    },
    input::{ctrl_sequence, keyboard_input_to_terminal_command},
    scene::{format_startup_panic, should_request_visual_redraw},
    terminals::{
        blend_rgba_in_place, compute_terminal_damage, find_kitty_config_path,
        initialize_terminal_text_renderer, is_emoji_like, is_private_use_like,
        parse_kitty_config_file, pixel_perfect_cell_size, pixel_perfect_terminal_logical_size,
        poll_terminal_snapshots, rasterize_terminal_glyph, resolve_alacritty_color,
        resolve_terminal_font_report, snap_to_pixel_grid, sync_terminal_presentations,
        xterm_indexed_rgb, CachedTerminalGlyph, KittyFontConfig, TerminalBridge,
        TerminalCellContent, TerminalCommand, TerminalDamage, TerminalDebugStats,
        TerminalDisplayMode, TerminalFontRole, TerminalFontState, TerminalFrameUpdate,
        TerminalGlyphCacheKey, TerminalLifecycle, TerminalManager, TerminalPanel,
        TerminalPresentation, TerminalPresentationStore, TerminalRuntimeState, TerminalSurface,
        TerminalTextRenderer, TerminalTextureState, TerminalUpdate, TerminalUpdateMailbox,
        TerminalViewState,
    },
};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{
        keyboard::{Key, KeyboardInput},
        ButtonState,
    },
    prelude::*,
    window::PrimaryWindow,
};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn pressed_text(key_code: KeyCode, text: Option<&str>) -> KeyboardInput {
    KeyboardInput {
        key_code,
        logical_key: Key::Character(text.unwrap_or("").into()),
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window: Entity::PLACEHOLDER,
    }
}

fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

fn test_bridge() -> (TerminalBridge, Arc<TerminalUpdateMailbox>) {
    let (input_tx, _input_rx) = mpsc::channel::<TerminalCommand>();
    let mailbox = Arc::new(TerminalUpdateMailbox::default());
    let bridge = TerminalBridge::new(
        input_tx,
        mailbox.clone(),
        Arc::new(Mutex::new(TerminalDebugStats::default())),
    );
    (bridge, mailbox)
}

fn surface_with_text(rows: usize, cols: usize, y: usize, text: &str) -> TerminalSurface {
    let mut surface = TerminalSurface::new(cols, rows);
    for (x, ch) in text.chars().enumerate() {
        surface.set_text_cell(x, y, &ch.to_string());
    }
    surface
}

#[test]
fn ctrl_sequence_maps_common_shortcuts() {
    assert_eq!(ctrl_sequence(KeyCode::KeyC), Some("\u{3}"));
    assert_eq!(ctrl_sequence(KeyCode::KeyL), Some("\u{c}"));
    assert_eq!(ctrl_sequence(KeyCode::Enter), None);
}

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
    let mailbox = TerminalUpdateMailbox::default();

    assert!(
        mailbox
            .push(TerminalUpdate::Frame(TerminalFrameUpdate {
                surface: surface_with_text(2, 2, 0, "a"),
                damage: TerminalDamage::Rows(vec![0]),
                runtime: crate::terminals::TerminalRuntimeState::running("one"),
            }))
            .should_wake
    );
    assert!(
        !mailbox
            .push(TerminalUpdate::Frame(TerminalFrameUpdate {
                surface: surface_with_text(2, 2, 1, "b"),
                damage: TerminalDamage::Rows(vec![1]),
                runtime: crate::terminals::TerminalRuntimeState::running("two"),
            }))
            .should_wake
    );
    assert!(
        !mailbox
            .push(TerminalUpdate::Status {
                runtime: crate::terminals::TerminalRuntimeState::running("done"),
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
fn redraw_scheduler_stays_idle_without_visual_work() {
    assert!(!should_request_visual_redraw(false, false, false));
}

#[test]
fn redraw_scheduler_runs_when_visual_work_exists() {
    assert!(should_request_visual_redraw(true, false, false));
    assert!(should_request_visual_redraw(false, true, false));
    assert!(should_request_visual_redraw(false, false, true));
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
            content: TerminalCellContent::Single('A'),
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

fn assert_glyph_has_visible_pixels(glyph: &CachedTerminalGlyph) {
    let non_zero_alpha = glyph
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 0)
        .count();
    assert!(
        non_zero_alpha > 0,
        "glyph rasterized to fully transparent image"
    );
}

#[test]
fn hud_layout_path_prefers_xdg_then_home() {
    assert_eq!(
        resolve_hud_layout_path_with(Some("/tmp/xdg"), Some("/tmp/home")),
        Some(PathBuf::from("/tmp/xdg/neozeus/hud-layout.v1"))
    );
    assert_eq!(
        resolve_hud_layout_path_with(None, Some("/tmp/home")),
        Some(PathBuf::from("/tmp/home/.config/neozeus/hud-layout.v1"))
    );
    assert_eq!(resolve_hud_layout_path_with(None, None), None);
}

#[test]
fn hud_layout_parse_and_serialize_roundtrip() {
    let mut persisted = PersistedHudState::default();
    persisted.modules.insert(
        HudModuleId::AgentList,
        PersistedHudModuleState {
            enabled: true,
            rect: HudRect {
                x: 24.0,
                y: 96.0,
                w: 300.0,
                h: 420.0,
            },
        },
    );
    let text = serialize_persisted_hud_state(&persisted);
    assert_eq!(parse_persisted_hud_state(&text), persisted);
}

#[test]
fn apply_persisted_layout_overrides_defaults() {
    let mut persisted = PersistedHudState::default();
    persisted.modules.insert(
        HudModuleId::AgentList,
        PersistedHudModuleState {
            enabled: false,
            rect: HudRect {
                x: 11.0,
                y: 22.0,
                w: 333.0,
                h: 444.0,
            },
        },
    );
    let hud_state =
        apply_persisted_layout(crate::hud::HUD_MODULE_DEFINITIONS.as_slice(), &persisted);
    let module = hud_state.get(HudModuleId::AgentList).unwrap();
    assert!(!module.shell.enabled);
    assert_eq!(module.shell.target_rect.x, 11.0);
    assert_eq!(module.shell.target_rect.w, 333.0);
}

#[test]
fn resolve_agent_label_prefers_directory_over_fallback() {
    let terminal_ids = [
        crate::terminals::TerminalId(1),
        crate::terminals::TerminalId(2),
    ];
    let mut directory = AgentDirectory::default();
    directory
        .labels
        .insert(crate::terminals::TerminalId(2), "oracle".into());

    assert_eq!(
        resolve_agent_label(&terminal_ids, &directory, crate::terminals::TerminalId(1)),
        "agent-1"
    );
    assert_eq!(
        resolve_agent_label(&terminal_ids, &directory, crate::terminals::TerminalId(2)),
        "oracle"
    );
}

#[test]
fn agent_rows_follow_terminal_order_and_focus() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);

    let rows = agent_rows(
        HudRect {
            x: 24.0,
            y: 96.0,
            w: 300.0,
            h: 420.0,
        },
        0.0,
        None,
        &manager,
        &AgentDirectory::default(),
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].terminal_id, id_one);
    assert_eq!(rows[0].label, "agent-1");
    assert_eq!(rows[1].terminal_id, id_two);
    assert!(rows[1].focused);
}

#[test]
fn agent_rows_mark_hovered_terminal() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

    let rows = agent_rows(
        HudRect {
            x: 24.0,
            y: 96.0,
            w: 300.0,
            h: 420.0,
        },
        0.0,
        Some(id_one),
        &manager,
        &AgentDirectory::default(),
    );
    assert!(
        rows.iter()
            .find(|row| row.terminal_id == id_one)
            .unwrap()
            .hovered
    );
    assert!(
        !rows
            .iter()
            .find(|row| row.terminal_id == id_two)
            .unwrap()
            .hovered
    );
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
fn hud_pointer_drag_updates_module_target_rect_and_marks_layout_dirty() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.set_cursor_position(Some(Vec2::new(40.0, 110.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<bevy::input::mouse::MouseWheel>::default());
    world.insert_resource(hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentDirectory::default());
    world.insert_resource(HudDispatcher::default());
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    {
        let hud_state = world.resource::<HudState>();
        assert!(hud_state.drag.is_some());
    }

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear_just_pressed(MouseButton::Left);
    {
        let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        let mut window = query
            .single_mut(&mut world)
            .expect("primary window missing");
        window.set_cursor_position(Some(Vec2::new(220.0, 180.0)));
    }
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let hud_state = world.resource::<HudState>();
    let module = hud_state.get(HudModuleId::AgentList).unwrap();
    assert!(hud_state.dirty_layout);
    assert!(module.shell.target_rect.x > crate::hud::HUD_MODULE_DEFINITIONS[1].default_rect.x);
    assert!(module.shell.target_rect.y > crate::hud::HUD_MODULE_DEFINITIONS[1].default_rect.y);
}

#[test]
fn animate_hud_modules_moves_current_rect_and_alpha_toward_target() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    let mut module =
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]);
    module.shell.current_rect.x = 24.0;
    module.shell.target_rect.x = 124.0;
    module.shell.current_alpha = 0.2;
    module.shell.target_alpha = 1.0;
    hud_state.insert(HudModuleId::AgentList, module);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    world.insert_resource(hud_state);

    world
        .run_system_once(crate::hud::animate_hud_modules)
        .unwrap();

    let hud_state = world.resource::<HudState>();
    let module = hud_state.get(HudModuleId::AgentList).unwrap();
    assert!(module.shell.current_rect.x > 24.0);
    assert!(module.shell.current_rect.x < 124.0);
    assert!(module.shell.current_alpha > 0.2);
    assert!(module.shell.current_alpha < 1.0);
}

#[test]
fn saving_hud_layout_persists_target_rect() {
    let dir = temp_dir("neozeus-hud-layout-save");
    let path = dir.join("hud-layout.v1");
    let mut world = World::default();
    let mut hud_state = HudState::default();
    let mut module =
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]);
    module.shell.target_rect = HudRect {
        x: 321.0,
        y: 222.0,
        w: 333.0,
        h: 444.0,
    };
    hud_state.insert(HudModuleId::AgentList, module);
    hud_state.dirty_layout = true;
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(hud_state);
    world.insert_resource(crate::hud::HudPersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: None,
    });
    world.insert_resource(TerminalVisibilityState::default());

    world.run_system_once(save_hud_layout_if_dirty).unwrap();
    world
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(1));
    world.run_system_once(save_hud_layout_if_dirty).unwrap();

    let serialized = fs::read_to_string(&path).expect("hud layout file missing");
    assert!(serialized.contains("AgentList enabled=1 x=321 y=222 w=333 h=444"));
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
            crate::terminals::PresentedTerminal {
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
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_one),
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

    world.resource_mut::<TerminalVisibilityState>().policy = TerminalVisibilityPolicy::ShowAll;
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

#[test]
fn clicking_debug_toolbar_button_emits_spawn_terminal_command() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut dispatcher = HudDispatcher::default();
    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state,
    );
    let new_terminal = buttons
        .iter()
        .find(|button| button.label == "new terminal")
        .expect("new terminal button missing");
    let click_point = Vec2::new(
        new_terminal.rect.x + new_terminal.rect.w * 0.5,
        new_terminal.rect.y + new_terminal.rect.h * 0.5,
    );

    dispatch_hud_pointer_click(
        HudModuleId::DebugToolbar,
        hud_state
            .get(HudModuleId::DebugToolbar)
            .map(|module| &module.model)
            .expect("toolbar module missing"),
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        click_point,
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state,
        &mut dispatcher,
    );

    assert_eq!(
        dispatcher.commands,
        vec![crate::hud::HudCommand::SpawnTerminal]
    );
}

#[test]
fn clicking_debug_toolbar_command_button_emits_terminal_command() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut dispatcher = HudDispatcher::default();
    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state,
    );
    let pwd = buttons
        .iter()
        .find(|button| button.label == "pwd")
        .expect("pwd button missing");
    let click_point = Vec2::new(pwd.rect.x + pwd.rect.w * 0.5, pwd.rect.y + pwd.rect.h * 0.5);

    dispatch_hud_pointer_click(
        HudModuleId::DebugToolbar,
        hud_state
            .get(HudModuleId::DebugToolbar)
            .map(|module| &module.model)
            .expect("toolbar module missing"),
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        click_point,
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state,
        &mut dispatcher,
    );

    assert_eq!(
        dispatcher.commands,
        vec![crate::hud::HudCommand::SendActiveTerminalCommand(
            "pwd".into()
        )]
    );
}

#[test]
fn clicking_agent_list_row_emits_focus_and_isolate_commands() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut dispatcher = HudDispatcher::default();
    let rows = agent_rows(
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 392.0,
        },
        0.0,
        None,
        &manager,
        &AgentDirectory::default(),
    );
    let target_row = rows
        .iter()
        .find(|row| row.terminal_id == id_two)
        .expect("agent row for second terminal missing");
    let click_point = Vec2::new(
        target_row.rect.x + target_row.rect.w * 0.5,
        target_row.rect.y + target_row.rect.h * 0.5,
    );

    dispatch_hud_pointer_click(
        HudModuleId::AgentList,
        hud_state
            .get(HudModuleId::AgentList)
            .map(|module| &module.model)
            .expect("agent list module missing"),
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 392.0,
        },
        click_point,
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state,
        &mut dispatcher,
    );

    assert_eq!(dispatcher.commands.len(), 2);
    assert_eq!(
        dispatcher.commands[0],
        crate::hud::HudCommand::FocusTerminal(id_two)
    );
    assert_eq!(
        dispatcher.commands[1],
        crate::hud::HudCommand::HideAllButTerminal(id_two)
    );
}

#[test]
fn agent_list_scroll_clamps_to_content_height() {
    let mut model = crate::hud::HudModuleModel::AgentList(Default::default());
    let mut manager = TerminalManager::default();
    for _ in 0..5 {
        let (bridge, _) = test_bridge();
        manager.create_terminal(bridge);
    }

    dispatch_hud_scroll(
        HudModuleId::AgentList,
        &mut model,
        -500.0,
        &manager,
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 112.0,
        },
    );

    let crate::hud::HudModuleModel::AgentList(state) = model else {
        panic!("expected agent list model");
    };
    assert_eq!(state.scroll_offset, 28.0);
}

#[test]
fn debug_toolbar_buttons_include_module_toggle_entries() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 24.0,
            w: 920.0,
            h: 64.0,
        },
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state,
    );
    assert!(buttons.iter().any(|button| button.label == "0 toolbar"));
    assert!(buttons.iter().any(|button| button.label == "1 agents"));
}

#[test]
fn debug_toolbar_module_toggle_buttons_reflect_enabled_state() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    hud_state.set_module_enabled(HudModuleId::AgentList, false);

    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 24.0,
            w: 920.0,
            h: 64.0,
        },
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state,
    );

    let toolbar = buttons
        .iter()
        .find(|button| button.label == "0 toolbar")
        .expect("toolbar toggle button missing");
    let agents = buttons
        .iter()
        .find(|button| button.label == "1 agents")
        .expect("agent toggle button missing");
    assert!(toolbar.active);
    assert!(!agents.active);
}

#[test]
fn hud_state_topmost_enabled_at_prefers_frontmost_module() {
    let mut state = HudState::default();
    state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    state.raise_to_front(HudModuleId::AgentList);

    assert_eq!(
        state.topmost_enabled_at(Vec2::new(40.0, 110.0)),
        Some(HudModuleId::AgentList)
    );
}

#[test]
fn hud_needs_redraw_when_drag_or_animation_is_active() {
    let mut state = HudState::default();
    state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    assert!(!hud_needs_redraw(&state));
    state.drag = Some(crate::hud::HudDragState {
        module_id: HudModuleId::AgentList,
        grab_offset: Vec2::ZERO,
    });
    assert!(hud_needs_redraw(&state));
    state.drag = None;
    let module = state.get_mut(HudModuleId::AgentList).unwrap();
    module.shell.current_rect.x = 0.0;
    module.shell.target_rect.x = 10.0;
    assert!(hud_needs_redraw(&state));
}

#[test]
fn terminal_visibility_policy_defaults_to_show_all() {
    assert_eq!(
        TerminalVisibilityPolicy::default(),
        TerminalVisibilityPolicy::ShowAll
    );
}

#[test]
fn formats_missing_gpu_startup_panics_as_user_facing_errors() {
    let error = format_startup_panic(&"Unable to find a GPU! renderer init failed")
        .expect("missing gpu panic should be formatted");
    assert!(error.contains("could not find a usable graphics adapter"));
    assert!(format_startup_panic(&"some other panic").is_none());
}
