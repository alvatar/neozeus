use crate::app_config::{
    load_neozeus_config, resolve_terminal_baseline_offset_px, resolve_terminal_font_path,
    resolve_terminal_font_size_px,
};

use super::types::{TerminalFontFace, TerminalFontReport};
use bevy::prelude::{ResMut, Resource};
use cosmic_text::{
    fontdb, Attrs as CtAttrs, Buffer as CtBuffer, Family as CtFamily, FontSystem as CtFontSystem,
    Shaping as CtShaping, SwashCache as CtSwashCache,
};
use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TerminalFontRasterConfig {
    pub(crate) font_size_px: f32,
    pub(crate) baseline_offset_px: f32,
}

impl Default for TerminalFontRasterConfig {
    /// Returns the built-in rasterization defaults used before config/font discovery runs.
    fn default() -> Self {
        Self {
            font_size_px: 21.6,
            baseline_offset_px: -0.5,
        }
    }
}

/// Deterministic monospace cell dimensions measured from the actual loaded font.
///
/// These are the ground-truth pixel dimensions for one terminal cell, derived by shaping a
/// reference glyph at the configured font size.  Every other metric (grid cols/rows,
/// texture size, display size) is computed from these values — nothing is hardcoded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TerminalCellMetrics {
    /// Advance width of a single monospace glyph in pixels (ceiled to integer).
    pub(crate) cell_width: u32,
    /// Line height (ascent + descent) in pixels (ceiled to integer).
    pub(crate) cell_height: u32,
}

impl Default for TerminalCellMetrics {
    /// Returns the default value for this type.
    fn default() -> Self {
        Self {
            cell_width: crate::app_config::DEFAULT_CELL_WIDTH_PX,
            cell_height: crate::app_config::DEFAULT_CELL_HEIGHT_PX,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct TerminalFontState {
    pub(crate) report: Option<Result<TerminalFontReport, String>>,
    pub(crate) raster: TerminalFontRasterConfig,
    /// Cell dimensions measured from the font.  Populated after font init; falls back to
    /// `DEFAULT_CELL_*_PX` before that.
    pub(crate) cell_metrics: TerminalCellMetrics,
}

#[derive(Resource)]
pub(crate) struct TerminalTextRenderer {
    pub(crate) font_system: Option<CtFontSystem>,
    pub(crate) swash_cache: CtSwashCache,
}

impl Default for TerminalTextRenderer {
    /// Creates an empty text renderer with no loaded font system and a fresh swash cache.
    fn default() -> Self {
        Self {
            font_system: None,
            swash_cache: CtSwashCache::new(),
        }
    }
}

impl TerminalFontState {
    /// Returns whether font discovery found a fallback face for private-use glyphs.
    pub(crate) fn has_private_use_font(&self) -> bool {
        self.fallback_family_name("private-use").is_some()
    }

    /// Returns whether font discovery found a fallback face for emoji-like glyphs.
    pub(crate) fn has_emoji_font(&self) -> bool {
        self.fallback_family_name("emoji").is_some()
    }

    /// Returns cosmic-text metrics for rasterizing glyphs at the configured font size.
    ///
    /// Glyph size stays fixed; only line height follows the measured cell height.
    pub(crate) fn glyph_metrics(&self) -> cosmic_text::Metrics {
        cosmic_text::Metrics::new(
            self.raster.font_size_px,
            self.cell_metrics.cell_height as f32,
        )
    }

    /// Returns the configured baseline offset in raster pixels.
    pub(crate) fn baseline_offset(&self) -> f32 {
        self.raster.baseline_offset_px
    }

    /// Implements fallback family name.
    pub(crate) fn fallback_family_name<'a>(&'a self, needle: &str) -> Option<&'a str> {
        let report = self.report.as_ref()?.as_ref().ok()?;
        report
            .fallbacks
            .iter()
            .find(|face| face.source.contains(needle))
            .map(|face| face.family.as_str())
    }
}

/// Measures the monospace cell dimensions by shaping a reference glyph at the given font size.
///
/// This is the single source of truth for terminal cell sizing.  The advance width of "M"
/// gives the cell width; `max_ascent + max_descent` from the layout gives the cell height.
/// Both are ceiled to whole pixels so grid arithmetic stays integer-exact.
pub(crate) fn measure_monospace_cell(
    font_system: &mut CtFontSystem,
    font_size: f32,
) -> Option<TerminalCellMetrics> {
    // Use a generous buffer so the glyph is never clipped during measurement.
    let metrics = cosmic_text::Metrics::new(font_size, font_size * 2.0);
    let mut buffer = CtBuffer::new_empty(metrics);
    {
        let mut borrowed = buffer.borrow_with(font_system);
        borrowed.set_size(Some(font_size * 10.0), Some(font_size * 10.0));
        let attrs = CtAttrs::new().family(CtFamily::Monospace);
        borrowed.set_text("M", &attrs, CtShaping::Advanced, None);
        borrowed.shape_until_scroll(false);
    }

    // Read advance width from the shaped glyph.
    let advance_width = buffer
        .layout_runs()
        .find_map(|run| run.glyphs.iter().next().map(|glyph| glyph.w));

    // Read ascent + descent from the LayoutLine for the actual glyph height.
    let (max_ascent, max_descent) = buffer
        .lines
        .first()
        .and_then(|line| line.layout_opt())
        .and_then(|layouts| layouts.first())
        .map(|layout_line| (layout_line.max_ascent, layout_line.max_descent))?;

    let cell_width = advance_width?.ceil() as u32;
    let cell_height = (max_ascent + max_descent).ceil() as u32 + 1;
    if cell_width == 0 || cell_height == 0 {
        return None;
    }
    Some(TerminalCellMetrics {
        cell_width,
        cell_height,
    })
}

/// Resolves terminal font configuration/report data once and initializes the shared text renderer.
///
/// After the font system is loaded the monospace cell dimensions are measured deterministically
/// from the actual font at the configured size.  The function is idempotent after the first
/// successful or failed discovery because `font_state.report` becomes the one-shot guard.
pub(crate) fn configure_terminal_fonts(
    mut font_state: ResMut<TerminalFontState>,
    mut text_renderer: ResMut<TerminalTextRenderer>,
) {
    if font_state.report.is_some() {
        return;
    }

    let config = match load_neozeus_config() {
        Ok(config) => config,
        Err(error) => {
            font_state.report = Some(Err(error));
            return;
        }
    };
    let defaults = TerminalFontRasterConfig::default();
    font_state.raster = TerminalFontRasterConfig {
        font_size_px: resolve_terminal_font_size_px(&config, defaults.font_size_px),
        baseline_offset_px: resolve_terminal_baseline_offset_px(
            &config,
            defaults.baseline_offset_px,
        ),
    };

    match resolve_terminal_font_report() {
        Ok(report) => match initialize_terminal_text_renderer(&report, &mut text_renderer) {
            Ok(()) => {
                // Measure the actual monospace cell dimensions from the loaded font.
                if let Some(font_system) = text_renderer.font_system.as_mut() {
                    if let Some(metrics) =
                        measure_monospace_cell(font_system, font_state.raster.font_size_px)
                    {
                        font_state.cell_metrics = metrics;
                    }
                }
                font_state.report = Some(Ok(report));
            }
            Err(error) => {
                font_state.report = Some(Err(error));
            }
        },
        Err(error) => {
            font_state.report = Some(Err(error));
        }
    }
}

/// Initializes the terminal text renderer using the current `LANG` locale.
fn initialize_terminal_text_renderer(
    report: &TerminalFontReport,
    text_renderer: &mut TerminalTextRenderer,
) -> Result<(), String> {
    let locale = env::var("LANG").unwrap_or_else(|_| "en-US".to_owned());
    initialize_terminal_text_renderer_with_locale(report, text_renderer, &locale)
}

/// Builds a cosmic-text font database from the resolved primary/fallback faces and installs it into
/// the shared text renderer.
///
/// The swash cache is reset alongside the font system so stale glyph caches never survive font
/// changes.
pub(crate) fn initialize_terminal_text_renderer_with_locale(
    report: &TerminalFontReport,
    text_renderer: &mut TerminalTextRenderer,
    locale: &str,
) -> Result<(), String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    db.set_monospace_family(report.primary.family.clone());
    db.load_font_file(&report.primary.path).map_err(|error| {
        format!(
            "failed to load primary terminal font {} into text renderer: {error}",
            report.primary.path.display()
        )
    })?;

    for fallback in &report.fallbacks {
        db.load_font_file(&fallback.path).map_err(|error| {
            format!(
                "failed to load fallback terminal font {} into text renderer: {error}",
                fallback.path.display()
            )
        })?;
    }

    text_renderer.font_system = Some(CtFontSystem::new_with_locale_and_db(locale.to_owned(), db));
    text_renderer.swash_cache = CtSwashCache::new();
    Ok(())
}

/// Resolves the terminal font stack from NeoZeus config or, absent that, Kitty/fontconfig data.
///
/// An explicit configured font path wins; otherwise the code discovers the requested family and
/// matching fallback faces.
fn resolve_terminal_font_report() -> Result<TerminalFontReport, String> {
    let config = load_neozeus_config()?;
    if let Some(font_path) = resolve_terminal_font_path(&config) {
        return resolve_terminal_font_stack_for_path(&font_path);
    }

    let requested_family = load_kitty_font_family()?.unwrap_or_else(|| "monospace".to_owned());
    resolve_terminal_font_stack_for_family(&requested_family)
}

/// Builds the primary/fallback font stack starting from an explicit font file path.
///
/// The primary face comes from `fc-query` on that exact file; fallback probes then ask fontconfig for
/// private-use and emoji coverage within the same family.
#[cfg_attr(test, allow(dead_code))]
fn resolve_terminal_font_stack_for_path(path: &Path) -> Result<TerminalFontReport, String> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    if !path.is_file() {
        return Err(format!(
            "configured terminal font path does not exist: {}",
            path.display()
        ));
    }

    let requested_family = fc_query_family_for_path(path)?;
    let primary = TerminalFontFace {
        family: requested_family.clone(),
        path: path.to_path_buf(),
        source: "neozeus config terminal.font_path".to_owned(),
    };
    let mut fallbacks = Vec::new();
    let mut seen_paths = BTreeSet::from([primary.path.clone()]);

    for (query, source) in [
        (
            format!("{requested_family}:charset=F013"),
            "kitty fallback for private-use symbols",
        ),
        (
            format!("{requested_family}:charset=1F680"),
            "kitty fallback for emoji",
        ),
    ] {
        let candidate = fc_match_face(&query, source)?;
        if seen_paths.insert(candidate.path.clone()) {
            fallbacks.push(candidate);
        }
    }

    Ok(TerminalFontReport {
        requested_family,
        primary,
        fallbacks,
    })
}

/// Builds the primary/fallback font stack starting from a requested family name.
///
/// Fontconfig chooses the concrete primary face, then two additional fontconfig queries look for
/// private-use and emoji-capable fallbacks.
fn resolve_terminal_font_stack_for_family(
    requested_family: &str,
) -> Result<TerminalFontReport, String> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let primary = fc_match_face(requested_family, "kitty primary font")?;
    let mut fallbacks = Vec::new();
    let mut seen_paths = BTreeSet::from([primary.path.clone()]);

    for (query, source) in [
        (
            format!("{requested_family}:charset=F013"),
            "kitty fallback for private-use symbols",
        ),
        (
            format!("{requested_family}:charset=1F680"),
            "kitty fallback for emoji",
        ),
    ] {
        let candidate = fc_match_face(&query, source)?;
        if seen_paths.insert(candidate.path.clone()) {
            fallbacks.push(candidate);
        }
    }

    Ok(TerminalFontReport {
        requested_family: requested_family.to_owned(),
        primary,
        fallbacks,
    })
}

/// Loads Kitty's configured `font_family`, following nested `include` directives when present.
///
/// If no Kitty config file exists, the function reports `Ok(None)` rather than treating that as an
/// error.
fn load_kitty_font_family() -> Result<Option<String>, String> {
    let Some(config_path) = find_kitty_config_path() else {
        return Ok(None);
    };

    let mut visited = BTreeSet::new();
    let mut font_family = None;
    parse_kitty_config_file(&config_path, &mut visited, &mut font_family)?;
    Ok(font_family)
}

/// Finds the first Kitty config file visible through the real process environment.
///
/// This is a wrapper around the testable `_with` helper.
fn find_kitty_config_path() -> Option<PathBuf> {
    find_kitty_config_path_with(
        env::var_os("KITTY_CONFIG_DIRECTORY").as_deref(),
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
        env::var_os("XDG_CONFIG_DIRS").as_deref(),
        Some(Path::new("/etc/xdg/kitty/kitty.conf")),
    )
}

/// Finds the first existing Kitty config file using the same precedence Kitty itself expects.
///
/// The search checks explicit config dir, XDG config home, HOME fallback, XDG config dirs, then an
/// optional system path.
fn find_kitty_config_path_with(
    kitty_config_directory: Option<&std::ffi::OsStr>,
    xdg_config_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    xdg_config_dirs: Option<&std::ffi::OsStr>,
    system_path: Option<&Path>,
) -> Option<PathBuf> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if let Some(dir) = kitty_config_directory {
        let path = PathBuf::from(dir).join("kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_home) = xdg_config_home {
        let path = PathBuf::from(xdg_config_home).join("kitty/kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(home) = home {
        let path = PathBuf::from(home).join(".config/kitty/kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_dirs) = xdg_config_dirs {
        for base in env::split_paths(xdg_config_dirs) {
            let path = base.join("kitty/kitty.conf");
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let system_path = system_path?.to_path_buf();
    system_path.is_file().then_some(system_path)
}

/// Parses one Kitty config file, following recursive `include` directives and recording the last seen
/// `font_family`.
///
/// The `visited` set breaks include cycles using canonical paths.
fn parse_kitty_config_file(
    path: &Path,
    visited: &mut BTreeSet<PathBuf>,
    font_family: &mut Option<String>,
) -> Result<(), String> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize {}: {error}", path.display()))?;
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let content = std::fs::read_to_string(&canonical).map_err(|error| {
        format!(
            "failed to read kitty config {}: {error}",
            canonical.display()
        )
    })?;

    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let value = parts.collect::<Vec<_>>().join(" ");
        if value.is_empty() {
            continue;
        }

        match key {
            "include" => {
                let include = canonical
                    .parent()
                    .map(|parent| parent.join(&value))
                    .unwrap_or_else(|| PathBuf::from(&value));
                if include.is_file() {
                    parse_kitty_config_file(&include, visited, font_family)?;
                }
            }
            "font_family" => {
                *font_family = Some(value);
            }
            _ => {}
        }
    }

    Ok(())
}

/// Uses `fc-query` to confirm the family name associated with an explicit font file path.
///
/// The function also verifies that fontconfig resolved the same file path back, rather than some other
/// substitution.
fn fc_query_family_for_path(path: &Path) -> Result<String, String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let output = Command::new("fc-query")
        .arg("-f")
        .arg("%{family}\n%{file}\n")
        .arg(path)
        .output()
        .map_err(|error| {
            format!(
                "failed to execute fc-query for terminal font {}: {error}",
                path.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "fc-query failed for terminal font {} with status {}: {}",
            path.display(),
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let family = lines
        .next()
        .ok_or_else(|| format!("fc-query returned no family for {}", path.display()))?
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("fc-query returned invalid family for {}", path.display()))?
        .to_owned();
    let resolved_path = PathBuf::from(
        lines
            .next()
            .ok_or_else(|| format!("fc-query returned no path for {}", path.display()))?,
    );

    if resolved_path != path {
        return Err(format!(
            "fc-query resolved terminal font path {} to unexpected file {}",
            path.display(),
            resolved_path.display()
        ));
    }

    Ok(family)
}

/// Uses `fc-match` to resolve one fontconfig query into a concrete font face record.
///
/// The returned record keeps the human-readable `source` string so later font reports can explain why
/// a face was chosen.
fn fc_match_face(query: &str, source: &str) -> Result<TerminalFontFace, String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let output = Command::new("/usr/bin/fc-match")
        .arg("-f")
        .arg("%{family}\n%{file}\n")
        .arg(query)
        .output()
        .map_err(|error| format!("failed to execute fc-match for `{query}`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "fc-match failed for `{query}` with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let family = lines
        .next()
        .ok_or_else(|| format!("fc-match returned no family for `{query}`"))?
        .to_owned();
    let path = PathBuf::from(
        lines
            .next()
            .ok_or_else(|| format!("fc-match returned no path for `{query}`"))?,
    );

    if !path.is_file() {
        return Err(format!(
            "fc-match resolved `{query}` to missing file {}",
            path.display()
        ));
    }

    Ok(TerminalFontFace {
        family,
        path,
        source: source.to_owned(),
    })
}

/// Returns whether a character falls into the private-use ranges relevant to terminal icon glyphs.
pub(crate) fn is_private_use_like(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

/// Returns whether a character falls into the emoji-heavy ranges that usually need fallback fonts.
pub(crate) fn is_emoji_like(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0xFE0F | 0x200D
    )
}

#[cfg(test)]
pub(crate) use tests::{
    resolve_terminal_font_report_for_family, resolve_terminal_font_report_for_path,
};


#[cfg(test)]
mod tests {
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
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let rect = crate::hud::docked_agent_list_rect(&window);
        hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

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
                    style: Default::default(),
                    width: 1,
                    selected: false,
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
        let mut font_family = None;
        parse_kitty_config_file(&main, &mut visited, &mut font_family)
            .expect("failed to parse kitty config");

        assert_eq!(font_family.as_deref(), Some("JetBrains Mono Nerd Font"));
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
                bold: false,
                italic: false,
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

    /// Verifies bold/italic shaping paths do not collapse onto the plain glyph cache entry.
    #[test]
    fn styled_ascii_glyphs_rasterize_differently_from_plain_text() {
        let report = test_terminal_font_report();
        let mut renderer = TerminalTextRenderer::default();
        initialize_test_terminal_text_renderer(&report, &mut renderer);
        let font_state = TerminalFontState {
            report: Some(Ok(report)),
            ..Default::default()
        };
        let plain_key = TerminalGlyphCacheKey {
            content: TerminalCellContent::Single('A'),
            font_role: TerminalFontRole::Primary,
            bold: false,
            italic: false,
            width_cells: 1,
            cell_width: 14,
            cell_height: 24,
        };
        let bold_key = TerminalGlyphCacheKey {
            bold: true,
            ..plain_key.clone()
        };
        let italic_key = TerminalGlyphCacheKey {
            italic: true,
            ..plain_key.clone()
        };

        let plain = rasterize_terminal_glyph(
            &plain_key,
            TerminalFontRole::Primary,
            false,
            &mut renderer,
            &font_state,
        );
        let bold = rasterize_terminal_glyph(
            &bold_key,
            TerminalFontRole::Primary,
            false,
            &mut renderer,
            &font_state,
        );
        let italic = rasterize_terminal_glyph(
            &italic_key,
            TerminalFontRole::Primary,
            false,
            &mut renderer,
            &font_state,
        );

        assert_ne!(
            bold.pixels, plain.pixels,
            "bold glyph should differ from plain glyph"
        );
        assert_ne!(
            italic.pixels, plain.pixels,
            "italic glyph should differ from plain glyph"
        );
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
            bold: false,
            italic: false,
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
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
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
}
