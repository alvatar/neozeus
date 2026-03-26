use crate::{
    app_config::{
        load_neozeus_config, resolve_terminal_baseline_offset_px, resolve_terminal_font_path,
        resolve_terminal_font_size_px,
    },
    terminals::{TerminalFontFace, TerminalFontReport},
};
use bevy::prelude::{ResMut, Resource};
use cosmic_text::{fontdb, FontSystem as CtFontSystem, SwashCache as CtSwashCache};
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
    // Returns the default value for this type.
    fn default() -> Self {
        Self {
            font_size_px: 16.0,
            baseline_offset_px: -0.5,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct TerminalFontState {
    pub(crate) report: Option<Result<TerminalFontReport, String>>,
    pub(crate) raster: TerminalFontRasterConfig,
}

#[derive(Resource)]
pub(crate) struct TerminalTextRenderer {
    pub(crate) font_system: Option<CtFontSystem>,
    pub(crate) swash_cache: CtSwashCache,
}

impl Default for TerminalTextRenderer {
    // Returns the default value for this type.
    fn default() -> Self {
        Self {
            font_system: None,
            swash_cache: CtSwashCache::new(),
        }
    }
}

impl TerminalFontState {
    // Returns whether private use font.
    pub(crate) fn has_private_use_font(&self) -> bool {
        self.fallback_family_name("private-use").is_some()
    }

    // Returns whether emoji font.
    pub(crate) fn has_emoji_font(&self) -> bool {
        self.fallback_family_name("emoji").is_some()
    }

    // Implements glyph metrics for cell height.
    pub(crate) fn glyph_metrics_for_cell_height(&self, cell_height: u32) -> cosmic_text::Metrics {
        let scale = cell_height.max(1) as f32 / 16.0;
        cosmic_text::Metrics::new(self.raster.font_size_px * scale, cell_height.max(1) as f32)
    }

    // Implements baseline offset for cell height.
    pub(crate) fn baseline_offset_for_cell_height(&self, cell_height: u32) -> f32 {
        self.raster.baseline_offset_px * (cell_height.max(1) as f32 / 16.0)
    }

    // Implements fallback family name.
    pub(crate) fn fallback_family_name<'a>(&'a self, needle: &str) -> Option<&'a str> {
        let report = self.report.as_ref()?.as_ref().ok()?;
        report
            .fallbacks
            .iter()
            .find(|face| face.source.contains(needle))
            .map(|face| face.family.as_str())
    }
}

// Configures terminal fonts.
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

// Initializes terminal text renderer.
pub(crate) fn initialize_terminal_text_renderer(
    report: &TerminalFontReport,
    text_renderer: &mut TerminalTextRenderer,
) -> Result<(), String> {
    let locale = env::var("LANG").unwrap_or_else(|_| "en-US".to_owned());
    initialize_terminal_text_renderer_with_locale(report, text_renderer, &locale)
}

// Initializes terminal text renderer with locale.
pub(crate) fn initialize_terminal_text_renderer_with_locale(
    report: &TerminalFontReport,
    text_renderer: &mut TerminalTextRenderer,
    locale: &str,
) -> Result<(), String> {
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

// Resolves terminal font report.
pub(crate) fn resolve_terminal_font_report() -> Result<TerminalFontReport, String> {
    let config = load_neozeus_config()?;
    if let Some(font_path) = resolve_terminal_font_path(&config) {
        return resolve_terminal_font_stack_for_path(&font_path);
    }

    let requested_family = load_kitty_font_family()?.unwrap_or_else(|| "monospace".to_owned());
    resolve_terminal_font_stack_for_family(&requested_family)
}

// Resolves terminal font report for family.
#[cfg(test)]
pub(crate) fn resolve_terminal_font_report_for_family(
    requested_family: &str,
) -> Result<TerminalFontReport, String> {
    resolve_terminal_font_stack_for_family(requested_family)
}

// Resolves terminal font report for path.
#[cfg(test)]
pub(crate) fn resolve_terminal_font_report_for_path(
    path: &Path,
) -> Result<TerminalFontReport, String> {
    resolve_terminal_font_stack_for_path(path)
}

// Resolves terminal font stack for path.
#[cfg_attr(test, allow(dead_code))]
fn resolve_terminal_font_stack_for_path(path: &Path) -> Result<TerminalFontReport, String> {
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

// Resolves terminal font stack for family.
fn resolve_terminal_font_stack_for_family(
    requested_family: &str,
) -> Result<TerminalFontReport, String> {
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

// Loads kitty font family.
fn load_kitty_font_family() -> Result<Option<String>, String> {
    let Some(config_path) = find_kitty_config_path() else {
        return Ok(None);
    };

    let mut visited = BTreeSet::new();
    let mut config = KittyFontConfig::default();
    parse_kitty_config_file(&config_path, &mut visited, &mut config)?;
    Ok(config.font_family)
}

// Finds kitty config path.
pub(crate) fn find_kitty_config_path() -> Option<PathBuf> {
    find_kitty_config_path_with(
        env::var_os("KITTY_CONFIG_DIRECTORY").as_deref(),
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
        env::var_os("XDG_CONFIG_DIRS").as_deref(),
        Some(Path::new("/etc/xdg/kitty/kitty.conf")),
    )
}

// Finds kitty config path with.
pub(crate) fn find_kitty_config_path_with(
    kitty_config_directory: Option<&std::ffi::OsStr>,
    xdg_config_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    xdg_config_dirs: Option<&std::ffi::OsStr>,
    system_path: Option<&Path>,
) -> Option<PathBuf> {
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

// Parses kitty config file.
pub(crate) fn parse_kitty_config_file(
    path: &Path,
    visited: &mut BTreeSet<PathBuf>,
    config: &mut KittyFontConfig,
) -> Result<(), String> {
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

// Implements fontconfig query family for path.
fn fc_query_family_for_path(path: &Path) -> Result<String, String> {
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

// Implements fontconfig match face.
fn fc_match_face(query: &str, source: &str) -> Result<TerminalFontFace, String> {
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

// Returns whether private use like.
pub(crate) fn is_private_use_like(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

// Returns whether emoji like.
pub(crate) fn is_emoji_like(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0xFE0F | 0x200D
    )
}
