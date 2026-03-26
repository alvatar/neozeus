use super::{
    assert_glyph_has_visible_pixels, fake_runtime_spawner, insert_default_hud_resources,
    insert_terminal_manager_resources, insert_terminal_manager_resources_into_app,
    insert_test_hud_state, insert_test_hud_state_into_app, surface_with_text, temp_dir,
    test_bridge, FakeDaemonClient,
};
use crate::{
    app_config::{
        load_neozeus_config, resolve_terminal_baseline_offset_px, resolve_terminal_font_path,
        resolve_terminal_font_size_px, DEFAULT_BG, DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX,
    },
    hud::{AgentDirectory, HudModuleId, HudState},
    startup::StartupLoadingState,
    terminals::{
        active_terminal_cell_size, active_terminal_dimensions, active_terminal_layout,
        active_terminal_viewport, blend_rgba_in_place, clear_done_tasks, compute_terminal_damage,
        create_terminal_image, extract_next_task, find_kitty_config_path_with,
        initialize_terminal_text_renderer_with_locale, is_emoji_like, is_private_use_like,
        parse_kitty_config_file, parse_persisted_terminal_sessions, pixel_perfect_cell_size,
        pixel_perfect_terminal_logical_size, poll_terminal_snapshots, rasterize_terminal_glyph,
        read_client_message, read_server_message, reconcile_terminal_sessions,
        resolve_alacritty_color, resolve_daemon_socket_path_with,
        resolve_terminal_font_report_for_family, resolve_terminal_font_report_for_path,
        resolve_terminal_notes_path_with, resolve_terminal_sessions_path_with,
        save_terminal_notes_if_dirty, save_terminal_sessions_if_dirty, send_command_payload_bytes,
        serialize_persisted_terminal_sessions, serialize_terminal_notes, snap_to_pixel_grid,
        sync_active_terminal_dimensions, sync_terminal_panel_frames, sync_terminal_presentations,
        sync_terminal_texture, task_entry_from_text, terminal_texture_screen_size,
        write_client_message, write_server_message, xterm_indexed_rgb, ClientMessage, DaemonEvent,
        DaemonRequest, DaemonServerHandle, KittyFontConfig, PersistedTerminalSessions,
        PresentedTerminal, ServerMessage, SocketTerminalDaemonClient, TerminalCommand,
        TerminalDaemonClient, TerminalDamage, TerminalDisplayMode, TerminalFontRole,
        TerminalFontState, TerminalFrameUpdate, TerminalGlyphCacheKey, TerminalLifecycle,
        TerminalManager, TerminalNotesState, TerminalPanel, TerminalPanelFrame,
        TerminalPresentation, TerminalPresentationStore, TerminalRuntimeState,
        TerminalSessionPersistenceState, TerminalSessionRecord, TerminalSurface,
        TerminalTextRenderer, TerminalTextureState, TerminalUpdate, TerminalViewState,
        PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
    },
};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use bevy::{ecs::system::RunSystemOnce, prelude::*, window::PrimaryWindow};
use bevy_egui::egui;
use std::{collections::BTreeSet, fs, sync::Arc, time::Duration};

fn test_terminal_font_report() -> crate::terminals::TerminalFontReport {
    resolve_terminal_font_report_for_family("monospace")
        .expect("failed to resolve terminal fonts for test family")
}

fn configured_terminal_font_report() -> crate::terminals::TerminalFontReport {
    let config = load_neozeus_config().unwrap_or_default();
    if let Some(path) = resolve_terminal_font_path(&config) {
        resolve_terminal_font_report_for_path(&path)
            .expect("failed to resolve configured terminal font report")
    } else {
        test_terminal_font_report()
    }
}

fn initialize_test_terminal_text_renderer(
    report: &crate::terminals::TerminalFontReport,
    renderer: &mut TerminalTextRenderer,
) {
    initialize_terminal_text_renderer_with_locale(report, renderer, "en-US")
        .expect("failed to initialize terminal text renderer");
}

fn configured_test_font_raster() -> crate::terminals::TerminalFontRasterConfig {
    let config = load_neozeus_config().unwrap_or_default();
    let defaults = crate::terminals::TerminalFontRasterConfig::default();
    crate::terminals::TerminalFontRasterConfig {
        font_size_px: resolve_terminal_font_size_px(&config, defaults.font_size_px),
        baseline_offset_px: resolve_terminal_baseline_offset_px(
            &config,
            defaults.baseline_offset_px,
        ),
    }
}

fn set_colored_text(
    surface: &mut TerminalSurface,
    row: usize,
    col: usize,
    text: &str,
    fg: egui::Color32,
) {
    for (offset, ch) in text.chars().enumerate() {
        if col + offset >= surface.cols {
            break;
        }
        surface.set_cell(
            col + offset,
            row,
            crate::terminals::TerminalCell {
                content: crate::terminals::TerminalCellContent::Single(ch),
                fg,
                bg: DEFAULT_BG,
                width: 1,
            },
        );
    }
}

fn render_surface_to_terminal_image(surface: TerminalSurface) -> (Image, TerminalTextureState) {
    let report = configured_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
        raster: configured_test_font_raster(),
    };

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);
    let terminal = manager.get_mut(id).expect("terminal should exist");
    terminal.snapshot.surface = Some(surface);
    terminal.surface_revision = 1;
    terminal.pending_damage = Some(TerminalDamage::Full);

    let mut images = Assets::<Image>::default();
    let image = images.add(create_terminal_image(UVec2::ONE));
    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: image.clone(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(crate::terminals::TerminalGlyphCache::default());
    world.insert_resource(renderer);
    world.insert_resource(images);
    world.spawn((window, PrimaryWindow));

    world
        .run_system_once(sync_terminal_texture)
        .expect("texture sync should succeed");

    let store = world.resource::<TerminalPresentationStore>();
    let presented = store.get(id).expect("missing presented terminal");
    let texture_state = presented.texture_state.clone();
    let images = world.resource::<Assets<Image>>();
    let image = images
        .get(&presented.image)
        .expect("rendered image should exist")
        .clone();
    (image, texture_state)
}

fn count_non_background_pixels_in_band(image: &Image, y_start: u32, y_end: u32) -> usize {
    let width = image.texture_descriptor.size.width as usize;
    let data = image.data.as_ref().expect("image data should exist");
    let y_end = y_end.min(image.texture_descriptor.size.height);
    let mut count = 0usize;
    for y in y_start.min(y_end)..y_end {
        let row = &data[y as usize * width * 4..(y as usize + 1) * width * 4];
        for pixel in row.chunks_exact(4) {
            if pixel[0] != DEFAULT_BG.r()
                || pixel[1] != DEFAULT_BG.g()
                || pixel[2] != DEFAULT_BG.b()
                || pixel[3] != DEFAULT_BG.a()
            {
                count += 1;
            }
        }
    }
    count
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
fn pixel_perfect_cell_size_stays_positive_and_scales_uniformly() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let cell_size = pixel_perfect_cell_size(120, 38, &window, &hud_state.layout_state());
    assert!(cell_size.x >= 1);
    assert!(cell_size.y >= 1);

    let width_scale = cell_size.x as f32 / DEFAULT_CELL_WIDTH_PX as f32;
    let height_scale = cell_size.y as f32 / DEFAULT_CELL_HEIGHT_PX as f32;
    assert!((width_scale - height_scale).abs() < 0.1);
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
        active_terminal_viewport(&window, &hud_state.layout_state()),
        (Vec2::new(1100.0, 900.0), Vec2::new(150.0, 0.0))
    );
}

#[test]
fn active_terminal_presentation_uses_texture_logical_size_and_centers_in_viewport() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

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

    let view_state = TerminalViewState::default();
    let dimensions = active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(dimensions.cols, dimensions.rows));
    }
    let cell_size = active_terminal_cell_size(&window, &view_state);
    let texture_state = TerminalTextureState {
        texture_size: UVec2::new(
            dimensions.cols as u32 * cell_size.x,
            dimensions.rows as u32 * cell_size.y,
        ),
        cell_size,
    };
    let expected_size = terminal_texture_screen_size(
        &texture_state,
        &view_state,
        &window,
        &hud_state.layout_state(),
        false,
    );

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: texture_state.clone(),
            desired_texture_state: texture_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
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
    assert!(presentation.current_size.distance(expected_size) < 0.2);
    assert!(
        presentation
            .current_position
            .distance(Vec2::new(150.0, 0.0))
            < 0.2
    );
    assert!((transform.translation.x - 150.0).abs() < 0.2);
    assert!(transform.translation.y.abs() < 0.2);
    assert!((transform.translation.z - 0.3).abs() < 0.01);
    assert!(expected_size.x <= 1068.0);
    assert!(expected_size.y <= 868.0);
}

#[test]
fn active_terminal_snaps_immediately_when_active_layout_changes() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let initial_window = Window {
        resolution: (800, 600).into(),
        ..Default::default()
    };
    let final_window = Window {
        resolution: bevy::window::WindowResolution::new(2880, 1800).with_scale_factor_override(1.5),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let initial_layout =
        active_terminal_layout(&initial_window, &hud_state.layout_state(), &view_state);
    let final_layout =
        active_terminal_layout(&final_window, &hud_state.layout_state(), &view_state);
    manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
        initial_layout.dimensions.cols,
        initial_layout.dimensions.rows,
    ));
    manager.get_mut(id).unwrap().surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: initial_layout.texture_size,
                cell_size: initial_layout.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: initial_layout.texture_size,
                cell_size: initial_layout.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    app.insert_resource(time);
    insert_terminal_manager_resources_into_app(&mut app, manager);
    app.insert_resource(presentation_store);
    app.insert_resource(crate::hud::TerminalVisibilityState::default());
    app.insert_resource(view_state);
    insert_test_hud_state_into_app(&mut app, hud_state);
    app.add_systems(Update, sync_terminal_presentations);
    let window_entity = app.world_mut().spawn((initial_window, PrimaryWindow)).id();
    app.world_mut().spawn((
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

    app.update();

    {
        let world = app.world_mut();
        let mut window = world.get_mut::<Window>(window_entity).unwrap();
        *window = final_window.clone();
    }
    {
        let world = app.world_mut();
        let mut manager = world.resource_mut::<TerminalManager>();
        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            final_layout.dimensions.cols,
            final_layout.dimensions.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 2;
    }
    {
        let world = app.world_mut();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).unwrap();
        presented.texture_state = TerminalTextureState {
            texture_size: final_layout.texture_size,
            cell_size: final_layout.cell_size,
        };
        presented.desired_texture_state = presented.texture_state.clone();
        presented.uploaded_revision = 2;
    }

    app.update();

    let expected_size = terminal_texture_screen_size(
        &TerminalTextureState {
            texture_size: final_layout.texture_size,
            cell_size: final_layout.cell_size,
        },
        &TerminalViewState::default(),
        &final_window,
        &HudState::default().layout_state(),
        false,
    );
    let world = app.world_mut();
    let mut query = world.query::<&TerminalPresentation>();
    let presentation = query.single(world).unwrap();
    assert_eq!(presentation.current_size, expected_size);
    assert_eq!(presentation.target_size, expected_size);
}

#[test]
fn switching_active_terminal_snaps_immediately_without_animation() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);

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

    let view_state = TerminalViewState::default();
    let dimensions = active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state);
    let cell_size = active_terminal_cell_size(&window, &view_state);
    let active_texture_state = TerminalTextureState {
        texture_size: UVec2::new(
            dimensions.cols as u32 * cell_size.x,
            dimensions.rows as u32 * cell_size.y,
        ),
        cell_size,
    };
    let stale_background_texture_state = TerminalTextureState {
        texture_size: UVec2::new(
            dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
            dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
        ),
        cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
    };
    let expected_size = terminal_texture_screen_size(
        &active_texture_state,
        &view_state,
        &window,
        &hud_state.layout_state(),
        false,
    );

    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(dimensions.cols, dimensions.rows));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id_one,
        PresentedTerminal {
            image: Default::default(),
            texture_state: active_texture_state.clone(),
            desired_texture_state: active_texture_state,
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );
    presentation_store.register(
        id_two,
        PresentedTerminal {
            image: Default::default(),
            texture_state: stale_background_texture_state.clone(),
            desired_texture_state: stale_background_texture_state,
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    app.insert_resource(time);
    insert_terminal_manager_resources_into_app(&mut app, manager);
    app.insert_resource(presentation_store);
    app.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::ShowAll,
    });
    app.insert_resource(view_state);
    insert_test_hud_state_into_app(&mut app, hud_state);
    app.add_systems(Update, sync_terminal_presentations);
    app.world_mut().spawn((window, PrimaryWindow));
    app.world_mut().spawn((
        TerminalPanel { id: id_one },
        TerminalPresentation {
            home_position: Vec2::new(-360.0, 120.0),
            current_position: Vec2::new(-360.0, 120.0),
            target_position: Vec2::new(-360.0, 120.0),
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.3,
            target_z: 0.3,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));
    app.world_mut().spawn((
        TerminalPanel { id: id_two },
        TerminalPresentation {
            home_position: Vec2::new(0.0, 120.0),
            current_position: Vec2::new(0.0, 120.0),
            target_position: Vec2::new(0.0, 120.0),
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 0.84,
            target_alpha: 0.84,
            current_z: -0.05,
            target_z: -0.05,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    app.update();

    {
        let focus_state = {
            let mut manager = app.world_mut().resource_mut::<TerminalManager>();
            manager.focus_terminal(id_two);
            manager.clone_focus_state()
        };
        app.world_mut().insert_resource(focus_state);
    }
    app.world_mut()
        .resource_mut::<crate::hud::TerminalVisibilityState>()
        .policy = crate::hud::TerminalVisibilityPolicy::Isolate(id_two);

    app.update();

    let world = app.world_mut();
    let mut query = world.query::<(&TerminalPanel, &TerminalPresentation, &Visibility)>();
    let rows = query.iter(world).collect::<Vec<_>>();
    let first = rows
        .iter()
        .find(|(panel, _, _)| panel.id == id_one)
        .unwrap();
    let second = rows
        .iter()
        .find(|(panel, _, _)| panel.id == id_two)
        .unwrap();
    assert_eq!(*first.2, Visibility::Hidden);
    assert_eq!(second.1.current_position, Vec2::new(150.0, 0.0));
    assert_eq!(second.1.current_size, expected_size);
    assert_eq!(second.1.current_alpha, 1.0);
    assert_eq!(second.1.current_z, 0.3);
}

#[test]
fn switching_active_terminal_keeps_cached_frame_visible_until_resized_surface_arrives() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

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

    let mut view_state = TerminalViewState::default();
    view_state.distance = 5.0;
    let layout_state = hud_state.layout_state();
    let dimensions = active_terminal_dimensions(&window, &layout_state, &view_state);
    let active_cell_size = active_terminal_cell_size(&window, &view_state);
    assert!(active_cell_size.x > DEFAULT_CELL_WIDTH_PX);
    assert!(active_cell_size.y > DEFAULT_CELL_HEIGHT_PX);
    let active_texture_state = TerminalTextureState {
        texture_size: UVec2::new(
            dimensions.cols as u32 * active_cell_size.x,
            dimensions.rows as u32 * active_cell_size.y,
        ),
        cell_size: active_cell_size,
    };
    let cached_background_state = TerminalTextureState {
        texture_size: UVec2::new(
            dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
            dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
        ),
        cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
    };
    let expected_size = terminal_texture_screen_size(
        &active_texture_state,
        &view_state,
        &window,
        &layout_state,
        false,
    );

    manager.focus_terminal(id_one);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(dimensions.cols, dimensions.rows));
        terminal.surface_revision = 1;
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id_one,
        PresentedTerminal {
            image: Default::default(),
            texture_state: active_texture_state.clone(),
            desired_texture_state: active_texture_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );
    presentation_store.register(
        id_two,
        PresentedTerminal {
            image: Default::default(),
            texture_state: cached_background_state.clone(),
            desired_texture_state: active_texture_state,
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    app.insert_resource(time);
    insert_terminal_manager_resources_into_app(&mut app, manager);
    app.insert_resource(presentation_store);
    app.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::ShowAll,
    });
    app.insert_resource(view_state);
    insert_test_hud_state_into_app(&mut app, hud_state);
    app.add_systems(Update, sync_terminal_presentations);
    app.world_mut().spawn((window, PrimaryWindow));
    app.world_mut().spawn((
        TerminalPanel { id: id_one },
        TerminalPresentation {
            home_position: Vec2::new(-360.0, 120.0),
            current_position: Vec2::new(-360.0, 120.0),
            target_position: Vec2::new(-360.0, 120.0),
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.3,
            target_z: 0.3,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));
    app.world_mut().spawn((
        TerminalPanel { id: id_two },
        TerminalPresentation {
            home_position: Vec2::new(0.0, 120.0),
            current_position: Vec2::new(0.0, 120.0),
            target_position: Vec2::new(0.0, 120.0),
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 0.84,
            target_alpha: 0.84,
            current_z: -0.05,
            target_z: -0.05,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    app.update();

    {
        let focus_state = {
            let mut manager = app.world_mut().resource_mut::<TerminalManager>();
            manager.focus_terminal(id_two);
            manager.clone_focus_state()
        };
        app.world_mut().insert_resource(focus_state);
    }
    app.world_mut()
        .resource_mut::<crate::hud::TerminalVisibilityState>()
        .policy = crate::hud::TerminalVisibilityPolicy::Isolate(id_two);

    app.update();

    let world = app.world_mut();
    let mut query = world.query::<(&TerminalPanel, &TerminalPresentation, &Visibility)>();
    let rows = query.iter(world).collect::<Vec<_>>();
    let first = rows
        .iter()
        .find(|(panel, _, _)| panel.id == id_one)
        .unwrap();
    let second = rows
        .iter()
        .find(|(panel, _, _)| panel.id == id_two)
        .unwrap();
    assert_eq!(*first.2, Visibility::Hidden);
    assert_eq!(*second.2, Visibility::Visible);
    assert_eq!(second.1.current_position, Vec2::new(150.0, 0.0));
    assert_eq!(second.1.current_size, expected_size);
    assert_eq!(second.1.current_alpha, 1.0);
    assert_eq!(second.1.current_z, 0.3);
}

#[test]
fn sync_terminal_texture_keeps_cached_switch_frame_until_resized_surface_arrives() {
    let report = test_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
        ..Default::default()
    };

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

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

    let view_state = TerminalViewState::default();
    let active_dimensions =
        active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state);
    let active_cell_size = active_terminal_cell_size(&window, &view_state);
    let active_texture_state = TerminalTextureState {
        texture_size: UVec2::new(
            active_dimensions.cols as u32 * active_cell_size.x,
            active_dimensions.rows as u32 * active_cell_size.y,
        ),
        cell_size: active_cell_size,
    };
    let cached_background_state = TerminalTextureState {
        texture_size: UVec2::new(80 * DEFAULT_CELL_WIDTH_PX, 24 * DEFAULT_CELL_HEIGHT_PX),
        cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
    };

    manager.focus_terminal(id_two);
    let first = manager.get_mut(id_one).unwrap();
    first.snapshot.surface = Some(TerminalSurface::new(
        active_dimensions.cols,
        active_dimensions.rows,
    ));
    first.surface_revision = 1;
    let second = manager.get_mut(id_two).unwrap();
    second.snapshot.surface = Some(TerminalSurface::new(80, 24));
    second.surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id_one,
        PresentedTerminal {
            image: Default::default(),
            texture_state: active_texture_state.clone(),
            desired_texture_state: active_texture_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );
    presentation_store.register(
        id_two,
        PresentedTerminal {
            image: Default::default(),
            texture_state: cached_background_state.clone(),
            desired_texture_state: cached_background_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(crate::terminals::TerminalGlyphCache::default());
    world.insert_resource(renderer);
    world.insert_resource(Assets::<Image>::default());
    world.spawn((window, PrimaryWindow));

    world.run_system_once(sync_terminal_texture).unwrap();

    let store = world.resource::<TerminalPresentationStore>();
    let inactive = store.get(id_one).expect("missing inactive terminal");
    assert_eq!(inactive.texture_state, active_texture_state);
    assert_eq!(inactive.desired_texture_state, active_texture_state);

    let active = store.get(id_two).expect("missing active terminal");
    assert_eq!(active.texture_state, cached_background_state);
    assert_eq!(active.desired_texture_state, active_texture_state);
}

#[test]
fn sync_terminal_texture_promotes_active_terminal_once_resized_surface_arrives() {
    let report = test_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
        ..Default::default()
    };

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

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

    let view_state = TerminalViewState::default();
    let active_dimensions =
        active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state);
    let active_cell_size = active_terminal_cell_size(&window, &view_state);
    let active_texture_state = TerminalTextureState {
        texture_size: UVec2::new(
            active_dimensions.cols as u32 * active_cell_size.x,
            active_dimensions.rows as u32 * active_cell_size.y,
        ),
        cell_size: active_cell_size,
    };
    let cached_background_state = TerminalTextureState {
        texture_size: UVec2::new(
            active_dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
            active_dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
        ),
        cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
    };

    let terminal = manager.get_mut(id).unwrap();
    terminal.snapshot.surface = Some(TerminalSurface::new(
        active_dimensions.cols,
        active_dimensions.rows,
    ));
    terminal.surface_revision = 2;
    terminal.pending_damage = Some(TerminalDamage::Full);

    let mut images = Assets::<Image>::default();
    let image = images.add(create_terminal_image(UVec2::ONE));

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image,
            texture_state: cached_background_state.clone(),
            desired_texture_state: cached_background_state,
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(crate::terminals::TerminalGlyphCache::default());
    world.insert_resource(renderer);
    world.insert_resource(images);
    world.spawn((window, PrimaryWindow));

    world.run_system_once(sync_terminal_texture).unwrap();

    let store = world.resource::<TerminalPresentationStore>();
    let presented = store.get(id).expect("missing presented terminal");
    assert_eq!(presented.texture_state, active_texture_state);
    assert_eq!(presented.desired_texture_state, active_texture_state);
    assert_eq!(presented.uploaded_revision, 2);
}

#[test]
fn active_terminal_resize_requests_follow_zoom_distance() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-1".into());
    let runtime_spawner = fake_runtime_spawner(client.clone());
    let (bridge, _) = test_bridge();

    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-1".into());
    manager.get_mut(terminal_id).unwrap().snapshot.surface = Some(TerminalSurface::new(120, 38));

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

    let mut view_state = TerminalViewState::default();
    view_state.distance = 5.0;
    let expected = active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state);

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(runtime_spawner);
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((window, PrimaryWindow));

    world
        .run_system_once(sync_active_terminal_dimensions)
        .unwrap();

    let requests = client.resize_requests.lock().unwrap().clone();
    assert_eq!(
        requests,
        vec![("neozeus-session-1".into(), expected.cols, expected.rows)]
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
    insert_terminal_manager_resources(&mut world, manager);
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
fn configured_terminal_font_path_resolves_exact_primary_face() {
    let report = resolve_terminal_font_report_for_path(std::path::Path::new(
        "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf",
    ))
    .expect("configured font path should resolve");

    assert_eq!(report.primary.family, "Adwaita Mono");
    assert_eq!(
        report.primary.path,
        std::path::PathBuf::from("/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf")
    );
    assert_eq!(report.primary.source, "neozeus config terminal.font_path");
    assert!(!report.fallbacks.is_empty());
}

#[test]
#[ignore = "manual offscreen font-reference verifier"]
fn dump_terminal_font_reference_sample() {
    let report = resolve_terminal_font_report_for_path(std::path::Path::new(
        "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf",
    ))
    .expect("configured font path should resolve");
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
        raster: configured_test_font_raster(),
    };

    let window = Window {
        resolution: bevy::window::WindowResolution::new(1908, 243).with_scale_factor_override(1.45),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let active_layout = active_terminal_layout(&window, &hud_state.layout_state(), &view_state);

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);
    let terminal = manager.get_mut(id).unwrap();
    let mut surface =
        TerminalSurface::new(active_layout.dimensions.cols, active_layout.dimensions.rows);
    let gray = egui::Color32::from_rgb(138, 150, 150);
    let info = egui::Color32::from_rgb(45, 240, 160);
    let warn = egui::Color32::from_rgb(198, 216, 92);
    let line0_a = "2026-03-26T15:07:52.729339Z  ";
    let line0_b = "INFO";
    let line0_c = " bevy_diagnostics::system_information_diagnostics_plugin::internal: SystemInfo { os: \"Linux (Arch Linux)\", kernel:";
    set_colored_text(&mut surface, 0, 0, line0_a, gray);
    set_colored_text(&mut surface, 0, line0_a.chars().count(), line0_b, info);
    set_colored_text(
        &mut surface,
        0,
        line0_a.chars().count() + line0_b.chars().count(),
        line0_c,
        gray,
    );
    set_colored_text(&mut surface, 1, 0, "memory: \"62.3 GiB\" }", gray);
    let line6_a = "2026-03-26T15:07:53.637782Z  ";
    let line6_b = "INFO";
    let line6_c = " bevy_winit::system: Creating new window neozeus (0v0)";
    set_colored_text(&mut surface, 6, 0, line6_a, gray);
    set_colored_text(&mut surface, 6, line6_a.chars().count(), line6_b, info);
    set_colored_text(
        &mut surface,
        6,
        line6_a.chars().count() + line6_b.chars().count(),
        line6_c,
        gray,
    );
    let line7_a = "2026-03-26T15:07:53.6378787Z ";
    let line7_b = "WARN";
    let line7_c = " bevy_winit::winit_windows: Can't select current monitor on window creation or cannot find current monitor!";
    set_colored_text(&mut surface, 7, 0, line7_a, gray);
    set_colored_text(&mut surface, 7, line7_a.chars().count(), line7_b, warn);
    set_colored_text(
        &mut surface,
        7,
        line7_a.chars().count() + line7_b.chars().count(),
        line7_c,
        gray,
    );
    terminal.snapshot.surface = Some(surface);
    terminal.surface_revision = 1;
    terminal.pending_damage = Some(TerminalDamage::Full);

    let mut images = Assets::<Image>::default();
    let image = images.add(create_terminal_image(UVec2::ONE));
    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: image.clone(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(crate::terminals::TerminalGlyphCache::default());
    world.insert_resource(renderer);
    world.insert_resource(images);
    world.spawn((window, PrimaryWindow));

    world.run_system_once(sync_terminal_texture).unwrap();

    let store = world.resource::<TerminalPresentationStore>();
    let presented = store.get(id).expect("missing presented terminal");
    let images = world.resource::<Assets<Image>>();
    let image = images
        .get(&presented.image)
        .expect("rendered image should exist");
    let size = image.texture_descriptor.size;
    let data = image.data.as_ref().expect("image data should exist");
    let mut ppm = Vec::with_capacity(data.len());
    ppm.extend_from_slice(format!("P6\n{} {}\n255\n", size.width, size.height).as_bytes());
    for pixel in data.chunks_exact(4) {
        ppm.extend_from_slice(&pixel[..3]);
    }
    std::fs::write("/tmp/neozeus-terminal-font-reference.ppm", ppm).expect("ppm should write");
}

#[test]
fn resolves_effective_terminal_font_stack_on_host() {
    let report = test_terminal_font_report();
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
    let report = test_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
        ..Default::default()
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
fn sync_terminal_texture_renders_visible_text_on_last_row() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let active_layout = active_terminal_layout(&window, &hud_state.layout_state(), &view_state);

    let mut surface =
        TerminalSurface::new(active_layout.dimensions.cols, active_layout.dimensions.rows);
    set_colored_text(
        &mut surface,
        active_layout.dimensions.rows - 1,
        0,
        "typed text",
        egui::Color32::from_rgb(220, 220, 220),
    );

    let (image, texture_state) = render_surface_to_terminal_image(surface);
    let y_start = (active_layout.dimensions.rows as u32 - 1) * texture_state.cell_size.y;
    let visible_pixels =
        count_non_background_pixels_in_band(&image, y_start, y_start + texture_state.cell_size.y);
    assert!(
        visible_pixels > 0,
        "last terminal row rendered no visible text pixels"
    );
}

#[test]
fn sync_terminal_texture_updates_pixels_when_last_row_text_changes() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let active_layout = active_terminal_layout(&window, &hud_state.layout_state(), &view_state);

    let mut before =
        TerminalSurface::new(active_layout.dimensions.cols, active_layout.dimensions.rows);
    set_colored_text(
        &mut before,
        active_layout.dimensions.rows - 1,
        0,
        "$ ",
        egui::Color32::from_rgb(220, 220, 220),
    );
    let (before_image, texture_state) = render_surface_to_terminal_image(before);

    let mut after =
        TerminalSurface::new(active_layout.dimensions.cols, active_layout.dimensions.rows);
    set_colored_text(
        &mut after,
        active_layout.dimensions.rows - 1,
        0,
        "$ abc",
        egui::Color32::from_rgb(220, 220, 220),
    );
    let (after_image, _) = render_surface_to_terminal_image(after);

    let before_data = before_image
        .data
        .as_ref()
        .expect("before image data should exist");
    let after_data = after_image
        .data
        .as_ref()
        .expect("after image data should exist");
    assert_ne!(
        before_data, after_data,
        "typed text did not change terminal image pixels"
    );

    let y_start = (active_layout.dimensions.rows as u32 - 1) * texture_state.cell_size.y;
    let before_pixels = count_non_background_pixels_in_band(
        &before_image,
        y_start,
        y_start + texture_state.cell_size.y,
    );
    let after_pixels = count_non_background_pixels_in_band(
        &after_image,
        y_start,
        y_start + texture_state.cell_size.y,
    );
    assert!(
        after_pixels > before_pixels,
        "typed text did not add visible pixels on the active input row"
    );
}

#[test]
fn task_entry_from_text_matches_zeus_checkbox_format() {
    assert_eq!(
        task_entry_from_text("first line\n  detail line\nsecond detail"),
        Some("- [ ] first line\n  detail line\nsecond detail".to_owned())
    );
    assert_eq!(task_entry_from_text("  \n \t"), None);
}

#[test]
fn terminal_notes_path_prefers_state_home_then_home_state_then_config() {
    assert_eq!(
        resolve_terminal_notes_path_with(
            Some("/tmp/state"),
            Some("/tmp/home"),
            Some("/tmp/config")
        ),
        Some(std::path::PathBuf::from("/tmp/state/neozeus/notes.v1"))
    );
    assert_eq!(
        resolve_terminal_notes_path_with(None, Some("/tmp/home"), Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/home/.local/state/neozeus/notes.v1"
        ))
    );
    assert_eq!(
        resolve_terminal_notes_path_with(None, None, Some("/tmp/config")),
        Some(std::path::PathBuf::from("/tmp/config/neozeus/notes.v1"))
    );
}

#[test]
fn terminal_notes_parse_and_serialize_roundtrip() {
    let mut notes = std::collections::HashMap::new();
    notes.insert("session-a".to_owned(), "- [ ] first\n  detail".to_owned());
    notes.insert("session-b".to_owned(), ".starts with dot".to_owned());

    let serialized = serialize_terminal_notes(&notes);
    let reparsed = crate::terminals::parse_terminal_notes(&serialized);

    assert_eq!(reparsed, notes);
}

#[test]
fn terminal_notes_append_and_prepend_tasks_follow_zeus_ordering() {
    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.append_task_from_text("session-a", "second task"));
    assert!(notes_state.prepend_task_from_text("session-a", "first task\n  detail"));
    assert_eq!(
        notes_state.note_text("session-a"),
        Some("- [ ] first task\n  detail\n- [ ] second task")
    );
    assert!(notes_state.has_note_text("session-a"));
}

#[test]
fn clear_done_tasks_removes_done_task_blocks() {
    let (updated, removed) =
        clear_done_tasks("- [x] done one\n  detail\n- [ ] keep\n- [X] done two\n  more\ntrailing");
    assert_eq!(removed, 2);
    assert_eq!(updated, "- [ ] keep");
}

#[test]
fn extract_next_task_marks_first_pending_block_done() {
    let extracted = extract_next_task("- [ ] first\n  detail\n- [x] done\n- [ ] second")
        .expect("pending task should be extracted");
    assert_eq!(extracted.0, "first\n  detail");
    assert_eq!(
        extracted.1,
        "- [x] first\n  detail\n- [x] done\n- [ ] second"
    );
}

#[test]
fn extract_next_task_falls_back_to_first_non_empty_line_without_headers() {
    let extracted =
        extract_next_task("\nfirst line\nsecond line").expect("fallback task should be extracted");
    assert_eq!(extracted.0, "first line");
    assert_eq!(extracted.1, "\nsecond line");
}

#[test]
fn terminal_notes_save_waits_for_debounce_window() {
    let dir = temp_dir("neozeus-terminal-notes-save-debounce");
    let path = dir.join("notes.v1");
    let mut notes_state = TerminalNotesState::default();
    notes_state.path = Some(path.clone());
    notes_state.dirty_since_secs = Some(0.0);
    assert!(notes_state.append_task_from_text("session-a", "first line"));

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(100));
    world.insert_resource(time);
    world.insert_resource(notes_state);

    world.run_system_once(save_terminal_notes_if_dirty).unwrap();

    assert!(!path.exists(), "debounced save should not run yet");

    world
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(300));
    world.run_system_once(save_terminal_notes_if_dirty).unwrap();

    assert!(path.exists(), "save should run after debounce window");
    let saved = std::fs::read_to_string(&path).expect("failed to read notes file");
    let reparsed = crate::terminals::parse_terminal_notes(&saved);
    assert_eq!(
        reparsed.get("session-a").map(String::as_str),
        Some("- [ ] first line")
    );
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
fn terminal_sessions_v2_quoted_labels_roundtrip() {
    let persisted = PersistedTerminalSessions {
        sessions: vec![TerminalSessionRecord {
            session_name: "neozeus-session-a".into(),
            label: Some("agent = \"alpha\"\\beta\nnext".into()),
            creation_index: 0,
            last_focused: true,
        }],
    };

    let serialized = serialize_persisted_terminal_sessions(&persisted);
    assert!(serialized.contains("version 2"));
    assert_eq!(parse_persisted_terminal_sessions(&serialized), persisted);
}

#[test]
fn terminal_sessions_v1_parser_remains_backward_compatible() {
    let persisted = parse_persisted_terminal_sessions(
        "version 1\nsession name=neozeus-session-a label=agent\\s1 creation_index=0 focused=1\n",
    );
    assert_eq!(persisted.sessions.len(), 1);
    assert_eq!(persisted.sessions[0].label.as_deref(), Some("agent 1"));
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
    insert_terminal_manager_resources(&mut world, manager);
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
    insert_terminal_manager_resources(&mut world, manager);
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
            desired_texture_state: TerminalTextureState {
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
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    insert_default_hud_resources(&mut world);
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
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalManager::default());
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
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);

    let mut world = World::default();
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(manager);
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
            desired_texture_state: Default::default(),
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
    assert_eq!(frames[0].2.custom_size, Some(Vec2::new(332.0, 192.0)));
    assert_eq!(frames[0].2.color, Color::srgba(1.0, 0.48, 0.08, 0.96));
}

#[test]
fn disconnected_terminal_shows_red_status_frame() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
    world.insert_resource(manager);
    let panel_entity = world
        .spawn((
            TerminalPanel { id: terminal_id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::new(10.0, 15.0),
                target_position: Vec2::ZERO,
                current_size: Vec2::new(300.0, 160.0),
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
            desired_texture_state: Default::default(),
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
    assert_eq!(frames[0].1.translation, Vec3::new(10.0, 15.0, 0.48));
    assert_eq!(frames[0].2.custom_size, Some(Vec2::new(308.0, 168.0)));
    assert_eq!(frames[0].2.color, Color::srgba(0.86, 0.20, 0.20, 0.92));
}

#[test]
fn startup_loading_shows_active_placeholder_before_first_surface_arrives() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut startup_loading = StartupLoadingState::default();
    startup_loading.register(id);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(startup_loading);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    insert_test_hud_state(&mut world, HudState::default());
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
        Visibility::Hidden,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Sprite, &Visibility)>();
    let (_, sprite, visibility) = query.single(&world).unwrap();
    assert_eq!(*visibility, Visibility::Visible);
    assert_ne!(sprite.color, Color::WHITE);
    assert!(sprite
        .custom_size
        .is_some_and(|size| size.x > 10.0 && size.y > 10.0));
}

#[test]
fn startup_loading_temporarily_overrides_isolate_to_show_all_pending_terminals() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_without_focus(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

    let mut presentation_store = TerminalPresentationStore::default();
    for id in [id_one, id_two] {
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut startup_loading = StartupLoadingState::default();
    startup_loading.register(id_one);
    startup_loading.register(id_two);

    let visibility_state = crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::Isolate(id_two),
    };

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(startup_loading);
    world.insert_resource(visibility_state);
    world.insert_resource(TerminalViewState::default());
    insert_test_hud_state(&mut world, HudState::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    for id in [id_one, id_two] {
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
            Visibility::Hidden,
        ));
    }

    world.run_system_once(sync_terminal_presentations).unwrap();

    let visible_count = world
        .query::<(&TerminalPanel, &Visibility)>()
        .iter(&world)
        .filter(|(_, visibility)| **visibility == Visibility::Visible)
        .count();
    assert_eq!(visible_count, 2);
}

#[test]
fn active_terminal_presentation_keeps_cached_frame_visible_until_active_layout_upload_is_ready() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);
    manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(80, 24));
    manager.get_mut(id).unwrap().surface_revision = 1;

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = crate::hud::HudState::default();
    let view_state = TerminalViewState::default();
    let active_layout = active_terminal_layout(&window, &hud_state.layout_state(), &view_state);

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(80 * DEFAULT_CELL_WIDTH_PX, 24 * DEFAULT_CELL_HEIGHT_PX),
                cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: active_layout.texture_size,
                cell_size: active_layout.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
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

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Visible);
}

#[test]
fn active_terminal_reappears_snapped_after_becoming_ready_for_new_layout() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let initial_window = Window {
        resolution: (800, 600).into(),
        ..Default::default()
    };
    let final_window = Window {
        resolution: bevy::window::WindowResolution::new(2880, 1800).with_scale_factor_override(1.5),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let initial_layout =
        active_terminal_layout(&initial_window, &hud_state.layout_state(), &view_state);
    let final_layout =
        active_terminal_layout(&final_window, &hud_state.layout_state(), &view_state);

    manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
        initial_layout.dimensions.cols,
        initial_layout.dimensions.rows,
    ));
    manager.get_mut(id).unwrap().surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: initial_layout.texture_size,
                cell_size: initial_layout.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: initial_layout.texture_size,
                cell_size: initial_layout.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    app.insert_resource(time);
    insert_terminal_manager_resources_into_app(&mut app, manager);
    app.insert_resource(presentation_store);
    app.insert_resource(crate::hud::TerminalVisibilityState::default());
    app.insert_resource(view_state);
    insert_test_hud_state_into_app(&mut app, hud_state);
    app.add_systems(Update, sync_terminal_presentations);
    let window_entity = app.world_mut().spawn((initial_window, PrimaryWindow)).id();
    app.world_mut().spawn((
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

    app.update();

    {
        let world = app.world_mut();
        let mut window = world.get_mut::<Window>(window_entity).unwrap();
        *window = final_window.clone();
    }
    app.update();

    {
        let world = app.world_mut();
        let mut manager = world.resource_mut::<TerminalManager>();
        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            final_layout.dimensions.cols,
            final_layout.dimensions.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 2;
    }
    {
        let world = app.world_mut();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).unwrap();
        presented.texture_state = TerminalTextureState {
            texture_size: final_layout.texture_size,
            cell_size: final_layout.cell_size,
        };
        presented.desired_texture_state = presented.texture_state.clone();
        presented.uploaded_revision = 2;
    }

    app.update();

    let expected_size = terminal_texture_screen_size(
        &TerminalTextureState {
            texture_size: final_layout.texture_size,
            cell_size: final_layout.cell_size,
        },
        &TerminalViewState::default(),
        &final_window,
        &HudState::default().layout_state(),
        false,
    );
    let world = app.world_mut();
    let mut query = world.query::<(&TerminalPresentation, &Visibility)>();
    let (presentation, visibility) = query.single(world).unwrap();
    assert_eq!(*visibility, Visibility::Visible);
    assert_eq!(presentation.current_size, expected_size);
    assert_eq!(presentation.target_size, expected_size);
}

#[test]
fn active_terminal_presentation_becomes_visible_once_active_layout_upload_is_ready() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = crate::hud::HudState::default();
    let view_state = TerminalViewState::default();
    let active_layout = active_terminal_layout(&window, &hud_state.layout_state(), &view_state);

    manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
        active_layout.dimensions.cols,
        active_layout.dimensions.rows,
    ));
    manager.get_mut(id).unwrap().surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: active_layout.texture_size,
                cell_size: active_layout.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: active_layout.texture_size,
                cell_size: active_layout.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
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
        Visibility::Hidden,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Visible);
}

#[test]
fn message_box_keeps_terminal_presentations_visible() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = crate::hud::HudState::default();
    let view_state = TerminalViewState::default();
    let active_layout = active_terminal_layout(&window, &hud_state.layout_state(), &view_state);
    hud_state.open_message_box(id);

    let terminal = manager.get_mut(id).unwrap();
    terminal.snapshot.surface = Some(TerminalSurface::new(
        active_layout.dimensions.cols,
        active_layout.dimensions.rows,
    ));
    terminal.surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: active_layout.texture_size,
                cell_size: active_layout.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: active_layout.texture_size,
                cell_size: active_layout.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
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
            desired_texture_state: TerminalTextureState {
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
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::Isolate(crate::terminals::TerminalId(999)),
    });
    world.insert_resource(TerminalViewState::default());
    insert_default_hud_resources(&mut world);
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
fn terminal_visibility_policy_show_all_keeps_only_active_terminal_visible() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = crate::hud::HudState::default();
    let view_state = TerminalViewState::default();
    let active_layout = active_terminal_layout(&window, &hud_state.layout_state(), &view_state);

    manager.get_mut(id_one).unwrap().snapshot.surface = Some(TerminalSurface::new(
        active_layout.dimensions.cols,
        active_layout.dimensions.rows,
    ));
    manager.get_mut(id_one).unwrap().surface_revision = 1;
    manager.get_mut(id_two).unwrap().snapshot.surface = Some(TerminalSurface::new(2, 2));
    manager.get_mut(id_two).unwrap().surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id_one,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: active_layout.texture_size,
                cell_size: active_layout.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: active_layout.texture_size,
                cell_size: active_layout.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );
    presentation_store.register(
        id_two,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::Isolate(id_one),
    });
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((window, PrimaryWindow));
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
    assert_eq!(vis[1], (id_two, Visibility::Hidden));
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

fn wait_for_lifecycle(
    updates: &std::sync::mpsc::Receiver<TerminalUpdate>,
    predicate: impl Fn(&TerminalLifecycle) -> bool,
) -> TerminalRuntimeState {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("timed out waiting for daemon lifecycle update");
        let update = updates
            .recv_timeout(remaining)
            .expect("timed out waiting for daemon lifecycle update");
        let runtime = match update {
            TerminalUpdate::Frame(frame) => frame.runtime,
            TerminalUpdate::Status { runtime, .. } => runtime,
        };
        if predicate(&runtime.lifecycle) {
            return runtime;
        }
    }
}

#[test]
fn daemon_socket_path_prefers_override_then_xdg_runtime_then_tmp_user() {
    let override_path = resolve_daemon_socket_path_with(
        Some("/tmp/neozeus-test/daemon.sock"),
        Some("/run/user/1000"),
        Some("/home/alvatar"),
        Some("oracle"),
    )
    .expect("override path should resolve");
    assert_eq!(
        override_path,
        std::path::PathBuf::from("/tmp/neozeus-test/daemon.sock")
    );

    let path = resolve_daemon_socket_path_with(
        None,
        Some("/run/user/1000"),
        Some("/home/alvatar"),
        Some("oracle"),
    )
    .expect("xdg runtime path should resolve");
    assert_eq!(
        path,
        std::path::PathBuf::from("/run/user/1000/neozeus/daemon.sock")
    );

    let fallback =
        resolve_daemon_socket_path_with(None, None, Some("/home/alvatar"), Some("oracle"))
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
fn daemon_exited_sessions_remain_listed_until_explicit_kill() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-exited-listed");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");
    let session_id = client
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("daemon session should be created");
    let attached = client
        .attach_session(&session_id)
        .expect("daemon session should attach");

    client
        .send_command(&session_id, TerminalCommand::SendCommand("exit".into()))
        .expect("exit command should send");
    let runtime = wait_for_lifecycle(&attached.updates, |lifecycle| {
        matches!(lifecycle, TerminalLifecycle::Exited { .. })
    });
    assert!(matches!(
        runtime.lifecycle,
        TerminalLifecycle::Exited { .. }
    ));

    let sessions = client.list_sessions().expect("daemon sessions should list");
    let session = sessions
        .iter()
        .find(|session| session.session_id == session_id)
        .expect("exited session should remain listed");
    assert!(matches!(
        session.runtime.lifecycle,
        TerminalLifecycle::Exited { .. }
    ));

    client
        .kill_session(&session_id)
        .expect("explicit kill should remove exited session");
    let sessions = client
        .list_sessions()
        .expect("daemon sessions should relist");
    assert!(!sessions
        .iter()
        .any(|session| session.session_id == session_id));
}

#[test]
fn daemon_session_listing_preserves_creation_order_not_lexical_order() {
    let (_server, socket_path) = start_test_daemon("neozeus-daemon-list-order");
    let client =
        SocketTerminalDaemonClient::connect(&socket_path).expect("daemon client should connect");

    let mut created = Vec::new();
    for _ in 0..12 {
        created.push(
            client
                .create_session(PERSISTENT_SESSION_PREFIX)
                .expect("daemon session should be created"),
        );
    }

    let listed = client
        .list_sessions()
        .expect("daemon sessions should list")
        .into_iter()
        .map(|session| session.session_id)
        .collect::<Vec<_>>();
    assert_eq!(listed, created);
}

#[test]
fn runtime_spawner_bootstraps_persistent_sessions_with_plain_pi_only() {
    let client = Arc::new(FakeDaemonClient::default());
    let spawner = fake_runtime_spawner(client.clone());

    let persistent = spawner
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("persistent session should be created");
    let _verifier = spawner
        .create_session(VERIFIER_SESSION_PREFIX)
        .expect("verifier session should be created");

    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0, persistent);
    assert!(matches!(
        &commands[0].1,
        TerminalCommand::SendCommand(command) if command == "pi"
    ));
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
