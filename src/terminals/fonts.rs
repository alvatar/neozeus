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

#[derive(Default)]
pub(crate) struct KittyFontConfig {
    pub(crate) font_family: Option<String>,
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
    let mut config = KittyFontConfig::default();
    parse_kitty_config_file(&config_path, &mut visited, &mut config)?;
    Ok(config.font_family)
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
    config: &mut KittyFontConfig,
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
                    parse_kitty_config_file(&include, visited, config)?;
                }
            }
            "font_family" => {
                config.font_family = Some(value);
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
mod tests;
