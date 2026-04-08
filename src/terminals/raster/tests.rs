use super::*;
use crate::{
    app_config::{
        load_neozeus_config, resolve_terminal_baseline_offset_px, resolve_terminal_font_path,
        resolve_terminal_font_size_px, DEFAULT_BG, DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX,
    },
    hud::{HudState, HudWidgetKey},
    terminals::{active_terminal_cell_size, active_terminal_dimensions, active_terminal_layout},
};
use bevy::{ecs::system::RunSystemOnce, prelude::*, window::PrimaryWindow};
use bevy_egui::egui;
use cosmic_text::{Style as CtStyle, Weight as CtWeight};
use std::{
    fs,
    path::Path,
    sync::{mpsc, Arc, Mutex},
};

use super::super::{
    bridge::TerminalBridge,
    debug::TerminalDebugStats,
    fonts::{
        initialize_terminal_text_renderer_with_locale, measure_monospace_cell,
        resolve_terminal_font_report_for_family, resolve_terminal_font_report_for_path,
        TerminalFontRasterConfig,
    },
    mailbox::TerminalUpdateMailbox,
    presentation_state::{
        PresentedTerminal, TerminalDisplayMode, TerminalPresentationStore, TerminalTextureState,
        TerminalViewState,
    },
    registry::TerminalManager,
    types::{
        TerminalCell, TerminalCellContent, TerminalCellStyle, TerminalDamage, TerminalDimensions,
        TerminalFontReport, TerminalSurface, TerminalUnderlineStyle,
    },
};

/// Resolves the host's effective monospace terminal font stack for raster tests.
fn test_terminal_font_report() -> TerminalFontReport {
    resolve_terminal_font_report_for_family("monospace")
        .expect("failed to resolve terminal fonts for test family")
}

/// Resolves the configured terminal font report when one is explicitly configured, otherwise falls
/// back to the host default.
fn configured_terminal_font_report() -> TerminalFontReport {
    let config = load_neozeus_config().unwrap_or_default();
    if let Some(path) = resolve_terminal_font_path(&config) {
        resolve_terminal_font_report_for_path(&path)
            .expect("failed to resolve configured terminal font report")
    } else {
        test_terminal_font_report()
    }
}

/// Initializes a terminal text renderer for tests with a fixed locale.
fn initialize_test_terminal_text_renderer(
    report: &TerminalFontReport,
    renderer: &mut TerminalTextRenderer,
) {
    initialize_terminal_text_renderer_with_locale(report, renderer, "en-US")
        .expect("failed to initialize terminal text renderer");
}

/// Computes the raster font sizing config that raster tests should use.
fn configured_test_font_raster() -> TerminalFontRasterConfig {
    let config = load_neozeus_config().unwrap_or_default();
    let defaults = TerminalFontRasterConfig::default();
    TerminalFontRasterConfig {
        font_size_px: resolve_terminal_font_size_px(&config, defaults.font_size_px),
        baseline_offset_px: resolve_terminal_baseline_offset_px(
            &config,
            defaults.baseline_offset_px,
        ),
    }
}

/// Builds a fully initialized font state with measured cell metrics for raster tests.
fn configured_test_font_state(
    report: TerminalFontReport,
    renderer: &mut TerminalTextRenderer,
) -> TerminalFontState {
    let raster = configured_test_font_raster();
    let cell_metrics = renderer
        .font_system
        .as_mut()
        .and_then(|fs| measure_monospace_cell(fs, raster.font_size_px))
        .unwrap_or_default();
    TerminalFontState {
        report: Some(Ok(report)),
        raster,
        cell_metrics,
    }
}

/// Creates a bare test terminal bridge suitable for raster-only tests.
fn test_bridge() -> TerminalBridge {
    let (input_tx, _input_rx) = mpsc::channel();
    TerminalBridge::new(
        input_tx,
        Arc::new(TerminalUpdateMailbox::default()),
        Arc::new(Mutex::new(TerminalDebugStats::default())),
    )
}

/// Inserts the terminal manager together with the mirrored focus-state test resource.
fn insert_terminal_manager_resources(world: &mut World, terminal_manager: TerminalManager) {
    world.insert_resource(terminal_manager.clone_focus_state());
    world.insert_resource(terminal_manager);
}

/// Writes colored single-width text into a terminal surface row for rasterization tests.
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
            TerminalCell {
                content: TerminalCellContent::Single(ch),
                fg,
                bg: DEFAULT_BG,
                style: Default::default(),
                width: 1,
                selected: false,
            },
        );
    }
}

/// Runs the normal terminal-texture sync pipeline on a supplied surface and returns the rendered
/// image plus the texture state it ended up using.
fn render_surface_to_terminal_image(surface: TerminalSurface) -> (Image, TerminalTextureState) {
    let report = configured_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = configured_test_font_state(report, &mut renderer);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();

    let bridge = test_bridge();
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
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    world.insert_resource(hud_state.layout_state());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(TerminalGlyphCache::default());
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

/// Renders a surface to a terminal image while starting from an explicit texture-state contract.
fn render_surface_to_terminal_image_with_presentation_state(
    surface: TerminalSurface,
    presentation_state: TerminalTextureState,
) -> (Image, TerminalTextureState) {
    let report = configured_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let mut best_size = configured_test_font_raster().font_size_px;
    let mut best_metrics = renderer
        .font_system
        .as_mut()
        .and_then(|fs| measure_monospace_cell(fs, best_size))
        .unwrap_or_default();
    let mut best_score = u32::MAX;
    for step in 32..=160 {
        let size = step as f32 * 0.25;
        let Some(metrics) = renderer
            .font_system
            .as_mut()
            .and_then(|fs| measure_monospace_cell(fs, size))
        else {
            continue;
        };
        let score = metrics.cell_width.abs_diff(presentation_state.cell_size.x)
            + metrics.cell_height.abs_diff(presentation_state.cell_size.y);
        if score < best_score {
            best_score = score;
            best_size = size;
            best_metrics = metrics;
            if score == 0 {
                break;
            }
        }
    }
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
        raster: TerminalFontRasterConfig {
            font_size_px: best_size,
            baseline_offset_px: configured_test_font_raster().baseline_offset_px,
        },
        cell_metrics: best_metrics,
    };

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();

    let bridge = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);
    let terminal = manager.get_mut(id).expect("terminal should exist");
    terminal.snapshot.surface = Some(surface);
    terminal.surface_revision = 1;
    terminal.pending_damage = Some(TerminalDamage::Full);

    let mut images = Assets::<Image>::default();
    let image = images.add(create_terminal_image(presentation_state.texture_size));
    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: image.clone(),
            texture_state: presentation_state.clone(),
            desired_texture_state: presentation_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    world.insert_resource(hud_state.layout_state());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(TerminalGlyphCache::default());
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

/// Counts non-background pixels in a contiguous horizontal band.
fn count_non_background_pixels_in_band(image: &Image, y_start: u32, y_end: u32) -> usize {
    let size = image.texture_descriptor.size;
    let data = image.data.as_ref().expect("image data should exist");
    let mut count = 0;
    for y in y_start..y_end.min(size.height) {
        for x in 0..size.width {
            let idx = ((y * size.width + x) * 4) as usize;
            let pixel = &data[idx..idx + 4];
            if pixel
                != [
                    DEFAULT_BG.r(),
                    DEFAULT_BG.g(),
                    DEFAULT_BG.b(),
                    DEFAULT_BG.a(),
                ]
            {
                count += 1;
            }
        }
    }
    count
}

/// Verifies combined bold+italic styling keeps both font attributes instead of silently dropping italics.
#[test]
fn terminal_text_attrs_preserve_bold_and_italic_together() {
    let font_state = TerminalFontState::default();
    let attrs = terminal_text_attrs(TerminalFontRole::Primary, true, true, &font_state);

    assert_eq!(attrs.weight, CtWeight::BOLD);
    assert_eq!(attrs.style, CtStyle::Italic);
}

/// Counts non-background pixels inside a rectangular cell-aligned crop.
fn count_non_background_pixels_in_rect(
    image: &Image,
    x_start: u32,
    y_start: u32,
    width: u32,
    height: u32,
) -> usize {
    let size = image.texture_descriptor.size;
    let data = image.data.as_ref().expect("image data should exist");
    let mut count = 0;
    for y in y_start..(y_start + height).min(size.height) {
        for x in x_start..(x_start + width).min(size.width) {
            let idx = ((y * size.width + x) * 4) as usize;
            let pixel = &data[idx..idx + 4];
            if pixel
                != [
                    DEFAULT_BG.r(),
                    DEFAULT_BG.g(),
                    DEFAULT_BG.b(),
                    DEFAULT_BG.a(),
                ]
            {
                count += 1;
            }
        }
    }
    count
}

/// Sums visible ink intensity inside a rectangular crop while ignoring untouched background pixels.
fn summed_non_background_rgb(
    image: &Image,
    x_start: u32,
    y_start: u32,
    width: u32,
    height: u32,
) -> u64 {
    let size = image.texture_descriptor.size;
    let data = image.data.as_ref().expect("image data should exist");
    let mut total = 0u64;
    for y in y_start..(y_start + height).min(size.height) {
        for x in x_start..(x_start + width).min(size.width) {
            let idx = ((y * size.width + x) * 4) as usize;
            let pixel = &data[idx..idx + 4];
            if pixel
                == [
                    DEFAULT_BG.r(),
                    DEFAULT_BG.g(),
                    DEFAULT_BG.b(),
                    DEFAULT_BG.a(),
                ]
            {
                continue;
            }
            total += u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]);
        }
    }
    total
}

/// Reads a binary `P6` PPM image from disk.
fn read_binary_ppm(path: &Path) -> (u32, u32, Vec<u8>) {
    let bytes = fs::read(path).expect("ppm should read");
    let mut idx = 0;
    let mut tokens = Vec::new();
    while tokens.len() < 4 {
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx < bytes.len() && bytes[idx] == b'#' {
            while idx < bytes.len() && bytes[idx] != b'\n' {
                idx += 1;
            }
            continue;
        }
        let start = idx;
        while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        tokens.push(String::from_utf8(bytes[start..idx].to_vec()).expect("ppm token utf8"));
    }
    assert_eq!(tokens[0], "P6");
    let width = tokens[1].parse::<u32>().expect("ppm width");
    let height = tokens[2].parse::<u32>().expect("ppm height");
    assert_eq!(tokens[3].parse::<u32>().expect("ppm max value"), 255);
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    (width, height, bytes[idx..].to_vec())
}

/// Crops a rectangular RGB region from packed row-major RGB bytes.
fn crop_rgb_rows(data: &[u8], width: u32, x: u32, y: u32, crop_w: u32, crop_h: u32) -> Vec<u8> {
    let stride = width as usize * 3;
    let start_x = x as usize * 3;
    let row_bytes = crop_w as usize * 3;
    let mut out = Vec::with_capacity(crop_h as usize * row_bytes);
    for row in 0..crop_h as usize {
        let row_start = (y as usize + row) * stride + start_x;
        out.extend_from_slice(&data[row_start..row_start + row_bytes]);
    }
    out
}

/// Crops a rectangular RGB region from a Bevy RGBA image, discarding alpha.
fn crop_image_rgb(image: &Image, x: u32, y: u32, crop_w: u32, crop_h: u32) -> Vec<u8> {
    let size = image.texture_descriptor.size;
    let data = image.data.as_ref().expect("image data should exist");
    let mut out = Vec::with_capacity((crop_w * crop_h * 3) as usize);
    for row in 0..crop_h {
        for col in 0..crop_w {
            let px = x + col;
            let py = y + row;
            assert!(
                px < size.width && py < size.height,
                "crop exceeds image bounds"
            );
            let idx = ((py * size.width + px) * 4) as usize;
            out.extend_from_slice(&data[idx..idx + 3]);
        }
    }
    out
}

/// Loads the reference ANSI screen used by the deterministic PI screenshot raster test.
fn surface_from_pi_screen_reference_ansi() -> TerminalSurface {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets/pi-screen-reference-20260328.ansi");
    let bytes = fs::read(path).expect("pi screen ansi should exist");
    let dimensions = TerminalDimensions {
        cols: 106,
        rows: 38,
    };
    let config = alacritty_terminal::term::Config {
        scrolling_history: 5000,
        ..alacritty_terminal::term::Config::default()
    };
    let mut terminal =
        alacritty_terminal::term::Term::<alacritty_terminal::event::VoidListener>::new(
            config,
            &dimensions,
            alacritty_terminal::event::VoidListener,
        );
    let mut parser = alacritty_terminal::vte::ansi::Processor::<
        alacritty_terminal::vte::ansi::StdSyncHandler,
    >::new();
    parser.advance(&mut terminal, &bytes);
    crate::terminals::build_surface(&terminal)
}

/// Verifies the alpha blender leaves fully transparent glyph pixels untouched and accumulates alpha
/// for partially transparent pixels.
#[test]
fn alpha_blend_preserves_transparent_glyph_background() {
    let mut pixel = [0, 0, 0, 0];
    blend_rgba_in_place(&mut pixel, [255, 255, 255, 0]);
    assert_eq!(pixel, [0, 0, 0, 0]);

    blend_rgba_in_place(&mut pixel, [255, 255, 255, 128]);
    assert_eq!(pixel[3], 128);
}

/// Verifies the raster path preserves the cached active texture for a switched-to terminal until a
/// surface matching the new active layout arrives.
#[test]
fn sync_terminal_texture_keeps_cached_switch_frame_until_resized_surface_arrives() {
    let report = test_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
        ..Default::default()
    };

    let bridge_one = test_bridge();
    let bridge_two = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let rect = crate::hud::docked_agent_list_rect(&window);
    hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

    let view_state = TerminalViewState::default();
    let active_dimensions =
        active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state, &font_state);
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
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
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
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    world.insert_resource(hud_state.layout_state());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(TerminalGlyphCache::default());
    world.insert_resource(renderer);
    world.insert_resource(Assets::<Image>::default());
    world.spawn((window, PrimaryWindow));

    world.run_system_once(sync_terminal_texture).unwrap();

    let store = world.resource::<TerminalPresentationStore>();
    let inactive = store.get(id_one).expect("missing inactive terminal");
    assert_eq!(
        inactive.texture_state,
        TerminalTextureState {
            texture_size: UVec2::new(
                active_dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
                active_dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
            ),
            cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
        }
    );
    assert_eq!(inactive.desired_texture_state, inactive.texture_state);

    let active = store.get(id_two).expect("missing active terminal");
    assert_eq!(active.texture_state, cached_background_state);
    assert_eq!(active.desired_texture_state, cached_background_state);
}

/// Verifies that once the resized active-layout surface finally arrives, texture sync promotes the
/// active terminal to the new texture contract and revision.
#[test]
fn sync_terminal_texture_promotes_active_terminal_once_resized_surface_arrives() {
    let report = test_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = TerminalFontState {
        report: Some(Ok(report)),
        ..Default::default()
    };

    let bridge = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let rect = crate::hud::docked_agent_list_rect(&window);
    hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

    let view_state = TerminalViewState::default();
    let active_dimensions =
        active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state, &font_state);
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
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    world.insert_resource(hud_state.layout_state());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(TerminalGlyphCache::default());
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

/// Verifies underline decoration paints visible pixels even when the cell carries only styling.
#[test]
fn sync_terminal_texture_draws_underline_for_styled_blank_cell() {
    let mut surface = TerminalSurface::new(2, 1);
    surface.set_cell(
        0,
        0,
        TerminalCell {
            content: TerminalCellContent::Empty,
            fg: egui::Color32::from_rgb(170, 220, 200),
            bg: DEFAULT_BG,
            style: TerminalCellStyle {
                underline: TerminalUnderlineStyle::Single,
                underline_color: Some(egui::Color32::from_rgb(32, 180, 140)),
                ..Default::default()
            },
            width: 1,
            selected: false,
        },
    );

    let (image, texture_state) = render_surface_to_terminal_image(surface);
    let underline_band_y = texture_state
        .cell_size
        .y
        .saturating_sub((texture_state.cell_size.y / 4).max(1));
    let underline_pixels = count_non_background_pixels_in_rect(
        &image,
        0,
        underline_band_y,
        texture_state.cell_size.x,
        (texture_state.cell_size.y / 4).max(1),
    );
    assert!(
        underline_pixels > 0,
        "styled blank cell should paint underline pixels"
    );
}

/// Verifies strikeout decoration paints visible pixels even when the cell carries only styling.
#[test]
fn sync_terminal_texture_draws_strikeout_for_styled_blank_cell() {
    let mut surface = TerminalSurface::new(2, 1);
    surface.set_cell(
        0,
        0,
        TerminalCell {
            content: TerminalCellContent::Empty,
            fg: egui::Color32::from_rgb(210, 210, 210),
            bg: DEFAULT_BG,
            style: TerminalCellStyle {
                strikeout: true,
                ..Default::default()
            },
            width: 1,
            selected: false,
        },
    );

    let (image, texture_state) = render_surface_to_terminal_image(surface);
    let strike_y = texture_state.cell_size.y / 2;
    let strike_pixels = count_non_background_pixels_in_rect(
        &image,
        0,
        strike_y.saturating_sub(1),
        texture_state.cell_size.x,
        3,
    );
    assert!(
        strike_pixels > 0,
        "styled blank cell should paint strikeout pixels"
    );
}

/// Verifies selected-foreground luminance computation does not overflow on bright cells.
#[test]
fn selected_foreground_color_handles_bright_selection_without_overflow() {
    let cell = TerminalCell {
        content: TerminalCellContent::Single('A'),
        fg: egui::Color32::from_rgb(255, 255, 255),
        bg: egui::Color32::from_rgb(255, 255, 255),
        style: TerminalCellStyle::default(),
        width: 1,
        selected: false,
    };

    assert_eq!(selected_foreground_color(&cell, true), egui::Color32::BLACK);
}

/// Verifies dim styling darkens the visible glyph output compared with the same un-dimmed glyph.
#[test]
fn sync_terminal_texture_dims_foreground_ink() {
    let mut surface = TerminalSurface::new(2, 1);
    let fg = egui::Color32::from_rgb(220, 220, 220);
    surface.set_cell(
        0,
        0,
        TerminalCell {
            content: TerminalCellContent::Single('A'),
            fg,
            bg: DEFAULT_BG,
            style: TerminalCellStyle::default(),
            width: 1,
            selected: false,
        },
    );
    surface.set_cell(
        1,
        0,
        TerminalCell {
            content: TerminalCellContent::Single('A'),
            fg,
            bg: DEFAULT_BG,
            style: TerminalCellStyle {
                dim: true,
                ..Default::default()
            },
            width: 1,
            selected: false,
        },
    );

    let (image, texture_state) = render_surface_to_terminal_image(surface);
    let normal_sum = summed_non_background_rgb(
        &image,
        0,
        0,
        texture_state.cell_size.x,
        texture_state.cell_size.y,
    );
    let dim_sum = summed_non_background_rgb(
        &image,
        texture_state.cell_size.x,
        0,
        texture_state.cell_size.x,
        texture_state.cell_size.y,
    );
    assert!(
        dim_sum < normal_sum,
        "dim glyph should emit less visible ink than regular glyph"
    );
}

/// Verifies every non-empty character cell in the provided `pi` screenshot crop exactly.
#[test]
fn rendered_pi_screen_matches_reference_per_character_pixels() {
    let reference_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets/pi-screen-reference-20260328.ppm");
    let (width, height, reference) = read_binary_ppm(&reference_path);
    assert_eq!((width, height), (1378, 1064));

    let cell_size = UVec2::new(13, 28);
    let surface = surface_from_pi_screen_reference_ansi();
    let (image, texture_state) = render_surface_to_terminal_image_with_presentation_state(
        surface,
        TerminalTextureState {
            texture_size: UVec2::new(width, height),
            cell_size,
        },
    );
    assert_eq!(texture_state.cell_size, cell_size);
    let actual = crop_image_rgb(&image, 0, 0, width, height);
    let reference_bg = [reference[0], reference[1], reference[2]];

    let mut ppm = Vec::with_capacity(actual.len() + 64);
    ppm.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
    ppm.extend_from_slice(&actual);
    fs::write("/tmp/neozeus-pi-screen-actual.ppm", ppm).expect("actual ppm should write");

    for row in 0..(height / cell_size.y) {
        for col in 0..(width / cell_size.x) {
            let x = col * cell_size.x;
            let y = row * cell_size.y;
            let expected = crop_rgb_rows(&reference, width, x, y, cell_size.x, cell_size.y);
            if expected
                .chunks_exact(3)
                .all(|pixel| [pixel[0], pixel[1], pixel[2]] == reference_bg)
            {
                continue;
            }
            let actual_cell = crop_rgb_rows(&actual, width, x, y, cell_size.x, cell_size.y);
            assert_eq!(
                actual_cell, expected,
                "pixel mismatch for screenshot cell row={row} col={col}"
            );
        }
    }
}

/// Verifies that texture sync paints visible glyph pixels on the last terminal row, which is a
/// common active-input case.
#[test]
fn sync_terminal_texture_renders_visible_text_on_last_row() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let active_layout =
        active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

    let mut surface = TerminalSurface::new(active_layout.0.cols, active_layout.0.rows);
    set_colored_text(
        &mut surface,
        active_layout.0.rows - 1,
        0,
        "typed text",
        egui::Color32::from_rgb(220, 220, 220),
    );

    let (image, texture_state) = render_surface_to_terminal_image(surface);
    let y_start = (active_layout.0.rows as u32 - 1) * texture_state.cell_size.y;
    let visible_pixels =
        count_non_background_pixels_in_band(&image, y_start, y_start + texture_state.cell_size.y);
    assert!(
        visible_pixels > 0,
        "last terminal row rendered no visible text pixels"
    );
}

/// Verifies that incremental rasterization of a scrolled viewport matches a fresh full render.
#[test]
fn incremental_scroll_render_matches_fresh_render() {
    let mut before = TerminalSurface::new(4, 3);
    set_colored_text(
        &mut before,
        0,
        0,
        "4",
        egui::Color32::from_rgb(220, 220, 220),
    );
    set_colored_text(
        &mut before,
        1,
        0,
        "5",
        egui::Color32::from_rgb(220, 220, 220),
    );
    set_colored_text(
        &mut before,
        2,
        0,
        "6",
        egui::Color32::from_rgb(220, 220, 220),
    );

    let mut after = TerminalSurface::new(4, 3);
    set_colored_text(
        &mut after,
        0,
        0,
        "3",
        egui::Color32::from_rgb(220, 220, 220),
    );
    set_colored_text(
        &mut after,
        1,
        0,
        "4",
        egui::Color32::from_rgb(220, 220, 220),
    );
    set_colored_text(
        &mut after,
        2,
        0,
        "5",
        egui::Color32::from_rgb(220, 220, 220),
    );
    after.display_offset = 1;

    let report = configured_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = configured_test_font_state(report, &mut renderer);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();

    let bridge = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);
    let terminal = manager.get_mut(id).expect("terminal should exist");
    terminal.snapshot.surface = Some(before.clone());
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
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    world.insert_resource(hud_state.layout_state());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.insert_resource(TerminalGlyphCache::default());
    world.insert_resource(renderer);
    world.insert_resource(images);
    world.spawn((window, PrimaryWindow));

    world.run_system_once(sync_terminal_texture).unwrap();
    let first_texture_state = world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .expect("presented terminal should exist")
        .texture_state
        .clone();

    {
        let mut manager = world.resource_mut::<TerminalManager>();
        let terminal = manager.get_mut(id).expect("terminal should exist");
        terminal.snapshot.surface = Some(after.clone());
        terminal.surface_revision = 2;
        terminal.pending_damage = Some(TerminalDamage::Full);
    }
    world.run_system_once(sync_terminal_texture).unwrap();

    let incremental_image = {
        let store = world.resource::<TerminalPresentationStore>();
        let presented = store.get(id).expect("presented terminal should exist");
        world
            .resource::<Assets<Image>>()
            .get(&presented.image)
            .expect("rendered image should exist")
            .clone()
    };
    let (fresh_image, _) =
        render_surface_to_terminal_image_with_presentation_state(after, first_texture_state);

    let incremental = incremental_image
        .data
        .as_ref()
        .expect("incremental image data should exist");
    let fresh = fresh_image
        .data
        .as_ref()
        .expect("fresh image data should exist");
    assert_eq!(
        incremental, fresh,
        "incremental scroll render diverged from fresh render"
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
    let font_state = TerminalFontState::default();
    let active_layout =
        active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

    let mut before = TerminalSurface::new(active_layout.0.cols, active_layout.0.rows);
    set_colored_text(
        &mut before,
        active_layout.0.rows - 1,
        0,
        "$ ",
        egui::Color32::from_rgb(220, 220, 220),
    );
    let (before_image, texture_state) = render_surface_to_terminal_image(before);

    let mut after = TerminalSurface::new(active_layout.0.cols, active_layout.0.rows);
    set_colored_text(
        &mut after,
        active_layout.0.rows - 1,
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
        "last-row text change should alter texture pixels"
    );

    let y_start = (active_layout.0.rows as u32 - 1) * texture_state.cell_size.y;
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
        "longer prompt line should draw more pixels"
    );
}
