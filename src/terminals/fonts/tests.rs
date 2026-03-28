use super::*;
use crate::{
    app_config::{
        load_neozeus_config, resolve_terminal_baseline_offset_px, resolve_terminal_font_path,
        resolve_terminal_font_size_px,
    },
    hud::{HudState, HudWidgetKey},
};
use bevy::{ecs::system::RunSystemOnce, prelude::*, window::PrimaryWindow};
use bevy_egui::egui;
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use super::super::{
    bridge::TerminalBridge,
    debug::TerminalDebugStats,
    mailbox::TerminalUpdateMailbox,
    presentation::target_active_terminal_dimensions,
    presentation_state::{
        PresentedTerminal, TerminalDisplayMode, TerminalPresentationStore, TerminalViewState,
    },
    raster::{
        create_terminal_image, rasterize_terminal_glyph, sync_terminal_texture,
        CachedTerminalGlyph, TerminalFontRole, TerminalGlyphCacheKey,
    },
    registry::TerminalManager,
    types::{TerminalCell, TerminalCellContent, TerminalDamage, TerminalSurface},
};

/// Resolves the font stack for an explicit family name.
pub(crate) fn resolve_terminal_font_report_for_family(
    requested_family: &str,
) -> Result<TerminalFontReport, String> {
    resolve_terminal_font_stack_for_family(requested_family)
}

/// Resolves the font stack for an explicit font file path.
pub(crate) fn resolve_terminal_font_report_for_path(
    path: &Path,
) -> Result<TerminalFontReport, String> {
    resolve_terminal_font_stack_for_path(path)
}

/// Creates a unique temporary directory for one fonts test case.
fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

/// Verifies that one rasterized glyph contains at least one visible pixel.
fn assert_glyph_has_visible_pixels(glyph: &CachedTerminalGlyph) {
    assert!(
        glyph
            .pixels
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0 && (pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0)),
        "glyph should contain visible pixels"
    );
}

/// Resolves the host's effective monospace terminal font stack for font-focused tests.
fn test_terminal_font_report() -> TerminalFontReport {
    resolve_terminal_font_report_for_family("monospace")
        .expect("failed to resolve terminal fonts for test family")
}

/// Resolves the explicitly configured terminal font report when present, otherwise the host default.
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

/// Computes the raster config used by fonts tests after applying optional NeoZeus config overrides.
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

/// Builds a fully initialized font state with measured cell metrics for test use.
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

/// Builds a measured font state for an explicit font size.
fn measured_font_state_for_size(font_size_px: f32) -> TerminalFontState {
    let report = configured_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let cell_metrics = renderer
        .font_system
        .as_mut()
        .and_then(|fs| measure_monospace_cell(fs, font_size_px))
        .expect("font metrics should be measurable");
    TerminalFontState {
        report: Some(Ok(report)),
        raster: TerminalFontRasterConfig {
            font_size_px,
            baseline_offset_px: configured_test_font_raster().baseline_offset_px,
        },
        cell_metrics,
    }
}

/// Verifies that measured cell metrics grow with font size.
#[test]
fn measured_cell_metrics_grow_with_font_size() {
    let smaller = measured_font_state_for_size(16.0);
    let larger = measured_font_state_for_size(21.6);

    assert!(larger.cell_metrics.cell_width > smaller.cell_metrics.cell_width);
    assert!(larger.cell_metrics.cell_height > smaller.cell_metrics.cell_height);
}

/// Verifies that larger measured cells reduce terminal grid capacity in the same viewport.
#[test]
fn larger_measured_cells_reduce_terminal_grid_in_same_viewport() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let rect = crate::hud::docked_agent_list_rect(&window);
    let agent_list = hud_state.get_mut(HudWidgetKey::AgentList).unwrap();
    agent_list.shell.enabled = true;
    agent_list.shell.current_rect = rect;
    agent_list.shell.target_rect = rect;

    let smaller = measured_font_state_for_size(16.0);
    let larger = measured_font_state_for_size(21.6);

    let smaller_grid =
        target_active_terminal_dimensions(&window, &hud_state.layout_state(), &smaller);
    let larger_grid =
        target_active_terminal_dimensions(&window, &hud_state.layout_state(), &larger);

    assert!(larger_grid.cols < smaller_grid.cols);
    assert!(larger_grid.rows < smaller_grid.rows);
}

/// Writes colored single-width text into a terminal surface row for rasterization-based font tests.
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
                bg: crate::app_config::DEFAULT_BG,
                width: 1,
            },
        );
    }
}

/// Creates a bare test terminal bridge suitable for rasterization-only tests.
fn test_bridge() -> TerminalBridge {
    let (input_tx, _input_rx) = mpsc::channel();
    TerminalBridge::new(
        input_tx,
        Arc::new(TerminalUpdateMailbox::default()),
        Arc::new(Mutex::new(TerminalDebugStats::default())),
    )
}

/// Verifies that Kitty config parsing follows `include` directives when resolving `font_family`.
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

/// Verifies Kitty config discovery precedence prefers an explicit config directory over XDG and HOME fallbacks.
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

/// Verifies that resolving a configured terminal font path preserves the exact primary face path and source metadata.
#[test]
fn configured_terminal_font_path_resolves_exact_primary_face() {
    let report = resolve_terminal_font_report_for_path(Path::new(
        "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf",
    ))
    .expect("configured font path should resolve");

    assert_eq!(report.primary.family, "Adwaita Mono");
    assert_eq!(
        report.primary.path,
        PathBuf::from("/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf")
    );
    assert_eq!(report.primary.source, "neozeus config terminal.font_path");
    assert!(!report.fallbacks.is_empty());
}

/// Verifies the host font-resolution path yields a usable primary face plus at least one fallback.
#[test]
fn resolves_effective_terminal_font_stack_on_host() {
    let report = test_terminal_font_report();
    assert_eq!(report.requested_family, "monospace");
    assert!(report.primary.path.is_file());
    assert!(!report.primary.family.is_empty());
    assert!(!report.fallbacks.is_empty());
    assert!(report.fallbacks.iter().all(|face| face.path.is_file()));
}

/// Verifies the Unicode range heuristics used for private-use and emoji fallback selection.
#[test]
fn detects_special_font_ranges() {
    assert!(is_private_use_like('\u{e0b0}'));
    assert!(is_emoji_like('🚀'));
    assert!(!is_private_use_like('a'));
}

/// Verifies that the standalone text renderer can rasterize a simple ASCII glyph into visible pixels.
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

/// Verifies that glyph rasterization snaps fractional baseline to the same pixels.
#[test]
fn glyph_rasterization_snaps_fractional_baseline_to_same_pixels() {
    let report = configured_terminal_font_report();
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let base = measured_font_state_for_size(14.0);
    let cache_key = TerminalGlyphCacheKey {
        content: TerminalCellContent::Single('A'),
        font_role: TerminalFontRole::Primary,
        width_cells: 1,
        cell_width: base.cell_metrics.cell_width,
        cell_height: base.cell_metrics.cell_height,
    };

    let integer_baseline = TerminalFontState {
        report: base.report.clone(),
        raster: TerminalFontRasterConfig {
            font_size_px: base.raster.font_size_px,
            baseline_offset_px: 0.0,
        },
        cell_metrics: base.cell_metrics,
    };
    let fractional_baseline = TerminalFontState {
        report: base.report.clone(),
        raster: TerminalFontRasterConfig {
            font_size_px: base.raster.font_size_px,
            baseline_offset_px: -0.49,
        },
        cell_metrics: base.cell_metrics,
    };

    let integer = rasterize_terminal_glyph(
        &cache_key,
        TerminalFontRole::Primary,
        false,
        &mut renderer,
        &integer_baseline,
    );
    let fractional = rasterize_terminal_glyph(
        &cache_key,
        TerminalFontRole::Primary,
        false,
        &mut renderer,
        &fractional_baseline,
    );

    assert_eq!(fractional.pixels, integer.pixels);
}

/// Manual verifier that dumps a rendered terminal font reference sample to a PPM file for visual inspection.
#[test]
#[ignore = "manual offscreen font-reference verifier"]
fn dump_terminal_font_reference_sample() {
    let report = resolve_terminal_font_report_for_path(Path::new(
        "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf",
    ))
    .expect("configured font path should resolve");
    let mut renderer = TerminalTextRenderer::default();
    initialize_test_terminal_text_renderer(&report, &mut renderer);
    let font_state = configured_test_font_state(report, &mut renderer);

    let window = Window {
        resolution: bevy::window::WindowResolution::new(1908, 243).with_scale_factor_override(1.45),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let active_layout = super::super::presentation::active_terminal_layout_for_dimensions(
        &window,
        &hud_state.layout_state(),
        &view_state,
        target_active_terminal_dimensions(&window, &hud_state.layout_state(), &font_state),
        &font_state,
    );

    let bridge = test_bridge();
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
    world.insert_resource(manager);
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(presentation_store);
    world.insert_resource(font_state);
    world.insert_resource(view_state);
    world.insert_resource(hud_state.layout_state());
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
