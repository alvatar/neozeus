use bevy_egui::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind,
    Vec2,
};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

pub const TERMINAL_FONT_FAMILY_NAME: &str = "neozeus-terminal";
const FONT_METRIC_SAMPLE_SIZE: f32 = 16.0;
const DEFAULT_BG: Color32 = Color32::from_rgb(10, 10, 10);

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCell {
    pub text: String,
    pub fg: Color32,
    pub bg: Color32,
    pub width: u8,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            text: String::new(),
            fg: Color32::from_rgb(220, 220, 220),
            bg: DEFAULT_BG,
            width: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCursorShape {
    Block,
    Underline,
    Beam,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCursor {
    pub x: usize,
    pub y: usize,
    pub shape: TerminalCursorShape,
    pub visible: bool,
    pub color: Color32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalSurface {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<TerminalCell>,
    pub cursor: Option<TerminalCursor>,
    pub title: Option<String>,
}

impl TerminalSurface {
    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![TerminalCell::default(); cols.saturating_mul(rows)],
            cursor: None,
            title: None,
        }
    }

    pub fn set_cell(&mut self, x: usize, y: usize, cell: TerminalCell) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        let index = y * self.cols + x;
        self.cells[index] = cell;
    }

    #[must_use]
    pub fn cell(&self, x: usize, y: usize) -> &TerminalCell {
        &self.cells[y * self.cols + x]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalFontFace {
    pub family: String,
    pub path: PathBuf,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalFontReport {
    pub kitty_config: Option<PathBuf>,
    pub requested_family: String,
    pub primary: TerminalFontFace,
    pub fallbacks: Vec<TerminalFontFace>,
}

pub fn install_terminal_fonts(ctx: &egui::Context) -> Result<TerminalFontReport, String> {
    let report = resolve_terminal_font_report()?;
    let mut definitions = FontDefinitions::default();
    let mut family_chain = Vec::new();

    insert_font_face(&mut definitions, &report.primary, &mut family_chain)?;
    for fallback in &report.fallbacks {
        insert_font_face(&mut definitions, fallback, &mut family_chain)?;
    }

    let family = FontFamily::Name(Arc::from(TERMINAL_FONT_FAMILY_NAME));
    definitions
        .families
        .insert(family.clone(), family_chain.clone());

    let monospace = definitions
        .families
        .entry(FontFamily::Monospace)
        .or_default();
    for name in family_chain.iter().rev() {
        monospace.retain(|existing| existing != name);
        monospace.insert(0, name.clone());
    }

    ctx.set_fonts(definitions);
    Ok(report)
}

fn insert_font_face(
    definitions: &mut FontDefinitions,
    face: &TerminalFontFace,
    family_chain: &mut Vec<String>,
) -> Result<(), String> {
    let key = format!("{}#{}", face.family, face.path.display());
    if definitions.font_data.contains_key(&key) {
        family_chain.push(key);
        return Ok(());
    }

    let bytes = fs::read(&face.path)
        .map_err(|error| format!("failed to read font {}: {error}", face.path.display()))?;
    definitions
        .font_data
        .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
    family_chain.push(key);
    Ok(())
}

pub fn paint_terminal(ui: &mut egui::Ui, surface: &TerminalSurface, use_custom_font: bool) {
    let available = ui.available_size();
    let desired = Vec2::new(available.x.max(64.0), available.y.max(64.0));
    let (response, painter) = ui.allocate_painter(desired, egui::Sense::click());
    let outer_rect = response.rect;

    painter.rect_filled(outer_rect, 0.0, DEFAULT_BG);

    if surface.cols == 0 || surface.rows == 0 {
        return;
    }

    let font_family = if use_custom_font {
        FontFamily::Name(Arc::from(TERMINAL_FONT_FAMILY_NAME))
    } else {
        FontFamily::Monospace
    };

    let sample_font = FontId::new(FONT_METRIC_SAMPLE_SIZE, font_family.clone());
    let sample_galley = painter.layout_no_wrap("M".to_owned(), sample_font, Color32::WHITE);
    let sample_size = sample_galley.size();
    let glyph_w = sample_size.x.max(1.0);
    let glyph_h = sample_size.y.max(1.0);
    let cell_aspect = (glyph_w / glyph_h).clamp(0.3, 1.0);

    let cell_h = (outer_rect.height() / surface.rows as f32)
        .min(outer_rect.width() / (surface.cols as f32 * cell_aspect))
        .max(1.0);
    let cell_w = (cell_h * cell_aspect).max(1.0);
    let grid_size = Vec2::new(cell_w * surface.cols as f32, cell_h * surface.rows as f32);
    let grid_min = Pos2::new(
        outer_rect.left() + (outer_rect.width() - grid_size.x) * 0.5,
        outer_rect.top() + (outer_rect.height() - grid_size.y) * 0.5,
    );
    let grid_rect = Rect::from_min_size(grid_min, grid_size);

    let font_scale = (cell_w / glyph_w).min(cell_h / glyph_h) * 0.98;
    let font = FontId::new((FONT_METRIC_SAMPLE_SIZE * font_scale).max(6.0), font_family);

    for y in 0..surface.rows {
        for x in 0..surface.cols {
            let cell = surface.cell(x, y);
            let min = Pos2::new(
                grid_rect.left() + x as f32 * cell_w,
                grid_rect.top() + y as f32 * cell_h,
            );
            let width = if cell.width <= 1 {
                cell_w
            } else {
                cell_w * f32::from(cell.width)
            };
            let cell_rect = Rect::from_min_size(min, Vec2::new(width, cell_h));
            painter.rect_filled(cell_rect, 0.0, cell.bg);

            if cell.width == 0 || cell.text.is_empty() {
                continue;
            }

            let galley = painter.layout_no_wrap(cell.text.clone(), font.clone(), cell.fg);
            let text_pos = Pos2::new(
                cell_rect.min.x,
                cell_rect.center().y - galley.size().y * 0.5,
            );
            painter
                .with_clip_rect(cell_rect)
                .galley(text_pos, galley, cell.fg);
        }
    }

    if let Some(cursor) = &surface.cursor {
        if cursor.visible && cursor.x < surface.cols && cursor.y < surface.rows {
            let min = Pos2::new(
                grid_rect.left() + cursor.x as f32 * cell_w,
                grid_rect.top() + cursor.y as f32 * cell_h,
            );
            let cursor_rect = Rect::from_min_size(min, Vec2::new(cell_w.max(1.0), cell_h.max(1.0)));
            match cursor.shape {
                TerminalCursorShape::Block => {
                    painter.rect_stroke(
                        cursor_rect.shrink(1.0),
                        0.0,
                        Stroke::new(1.5, cursor.color),
                        StrokeKind::Outside,
                    );
                }
                TerminalCursorShape::Underline => {
                    painter.line_segment(
                        [cursor_rect.left_bottom(), cursor_rect.right_bottom()],
                        Stroke::new(2.0, cursor.color),
                    );
                }
                TerminalCursorShape::Beam => {
                    painter.line_segment(
                        [cursor_rect.left_top(), cursor_rect.left_bottom()],
                        Stroke::new(2.0, cursor.color),
                    );
                }
            }
        }
    }
}

pub fn resolve_terminal_font_report() -> Result<TerminalFontReport, String> {
    let (kitty_config, config) = load_kitty_font_config()?;
    let requested_family = config.font_family.unwrap_or_else(|| "monospace".to_owned());
    let primary = fc_match_face(&requested_family, "kitty primary font")?;
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
        kitty_config,
        requested_family,
        primary,
        fallbacks,
    })
}

#[derive(Default)]
struct KittyFontConfig {
    font_family: Option<String>,
}

fn load_kitty_font_config() -> Result<(Option<PathBuf>, KittyFontConfig), String> {
    let Some(config_path) = find_kitty_config_path() else {
        return Ok((None, KittyFontConfig::default()));
    };

    let mut visited = BTreeSet::new();
    let mut config = KittyFontConfig::default();
    parse_kitty_config_file(&config_path, &mut visited, &mut config)?;
    Ok((Some(config_path), config))
}

fn find_kitty_config_path() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("KITTY_CONFIG_DIRECTORY") {
        let path = PathBuf::from(dir).join("kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg_config_home).join("kitty/kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let path = PathBuf::from(home).join(".config/kitty/kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_dirs) = env::var_os("XDG_CONFIG_DIRS") {
        for base in env::split_paths(&xdg_config_dirs) {
            let path = base.join("kitty/kitty.conf");
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let system_path = PathBuf::from("/etc/xdg/kitty/kitty.conf");
    if system_path.is_file() {
        Some(system_path)
    } else {
        None
    }
}

fn parse_kitty_config_file(
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

    let content = fs::read_to_string(&canonical).map_err(|error| {
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

#[cfg(test)]
mod tests {
    use super::{
        find_kitty_config_path, parse_kitty_config_file, resolve_terminal_font_report,
        KittyFontConfig,
    };
    use std::{
        collections::BTreeSet,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
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
}
