use bevy_egui::egui;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub(crate) const DEFAULT_COLS: u16 = 120;
pub(crate) const DEFAULT_ROWS: u16 = 38;
pub(crate) const DEFAULT_BG: egui::Color32 = egui::Color32::from_rgb(10, 10, 10);
pub(crate) const DEFAULT_CELL_HEIGHT_PX: u32 = 16;
pub(crate) const DEFAULT_CELL_WIDTH_PX: u32 = 9;
pub(crate) const GPU_NOT_FOUND_PANIC_FRAGMENT: &str = "Unable to find a GPU!";

pub(crate) const DEBUG_LOG_PATH: &str = "/tmp/neozeus-debug.log";
pub(crate) const DEBUG_TEXTURE_DUMP_PATH: &str = "/tmp/neozeus-texture.ppm";
const NEOZEUS_CONFIG_FILENAME: &str = "config.toml";
const NEOZEUS_CWD_CONFIG_FILENAME: &str = "neozeus.toml";

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NeoZeusConfig {
    pub(crate) terminal: NeoZeusTerminalConfig,
    pub(crate) window: NeoZeusWindowConfig,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NeoZeusTerminalConfig {
    pub(crate) font_path: Option<PathBuf>,
    pub(crate) font_size_px: Option<f32>,
    pub(crate) baseline_offset_px: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NeoZeusWindowConfig {
    pub(crate) title: Option<String>,
    pub(crate) app_id: Option<String>,
}

// Loads NeoZeus config.
pub(crate) fn load_neozeus_config() -> Result<NeoZeusConfig, String> {
    let Some(path) = resolve_neozeus_config_path() else {
        return Ok(NeoZeusConfig::default());
    };
    load_neozeus_config_from(&path)
}

// Loads NeoZeus config from.
pub(crate) fn load_neozeus_config_from(path: &Path) -> Result<NeoZeusConfig, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read NeoZeus config {}: {error}", path.display()))?;
    parse_neozeus_config(&text)
}

// Resolves NeoZeus config path.
pub(crate) fn resolve_neozeus_config_path() -> Option<PathBuf> {
    let current_dir = env::current_dir().ok();
    resolve_neozeus_config_path_with(
        env::var_os("NEOZEUS_CONFIG_PATH").as_deref(),
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
        current_dir.as_deref(),
    )
}

// Resolves NeoZeus config path with.
pub(crate) fn resolve_neozeus_config_path_with(
    explicit_path: Option<&std::ffi::OsStr>,
    xdg_config_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    current_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(explicit_path) = explicit_path {
        let path = PathBuf::from(explicit_path);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_home) = xdg_config_home {
        let path = PathBuf::from(xdg_config_home)
            .join("neozeus")
            .join(NEOZEUS_CONFIG_FILENAME);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(home) = home {
        let path = PathBuf::from(home)
            .join(".config/neozeus")
            .join(NEOZEUS_CONFIG_FILENAME);
        if path.is_file() {
            return Some(path);
        }
    }

    current_dir
        .map(|dir| dir.join(NEOZEUS_CWD_CONFIG_FILENAME))
        .filter(|path| path.is_file())
}

// Resolves window title.
pub(crate) fn resolve_window_title(config: &NeoZeusConfig) -> String {
    env::var("NEOZEUS_WINDOW_TITLE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| config.window.title.clone())
        .unwrap_or_else(|| "neozeus".to_owned())
}

// Resolves app id.
pub(crate) fn resolve_app_id(config: &NeoZeusConfig) -> String {
    env::var("NEOZEUS_APP_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| config.window.app_id.clone())
        .unwrap_or_else(|| "neozeus".to_owned())
}

// Resolves terminal font path.
pub(crate) fn resolve_terminal_font_path(config: &NeoZeusConfig) -> Option<PathBuf> {
    env::var_os("NEOZEUS_TERMINAL_FONT_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| config.terminal.font_path.clone())
}

// Resolves terminal font size px.
pub(crate) fn resolve_terminal_font_size_px(config: &NeoZeusConfig, default: f32) -> f32 {
    env::var("NEOZEUS_TERMINAL_FONT_SIZE_PX")
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .or(config.terminal.font_size_px)
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(default)
}

// Resolves terminal baseline offset px.
pub(crate) fn resolve_terminal_baseline_offset_px(config: &NeoZeusConfig, default: f32) -> f32 {
    env::var("NEOZEUS_TERMINAL_BASELINE_OFFSET_PX")
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .or(config.terminal.baseline_offset_px)
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

// Parses NeoZeus config.
pub(crate) fn parse_neozeus_config(text: &str) -> Result<NeoZeusConfig, String> {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Section {
        Root,
        Terminal,
        Window,
        Other,
    }

    let mut config = NeoZeusConfig::default();
    let mut section = Section::Root;

    for (line_idx, raw_line) in text.lines().enumerate() {
        let line = strip_toml_comment(raw_line)?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(format!(
                    "invalid NeoZeus config section header on line {}: `{line}`",
                    line_idx + 1
                ));
            }
            section = match &line[1..line.len() - 1] {
                "terminal" => Section::Terminal,
                "window" => Section::Window,
                _ => Section::Other,
            };
            continue;
        }

        let Some((key, raw_value)) = line.split_once('=') else {
            return Err(format!(
                "invalid NeoZeus config entry on line {}: `{line}`",
                line_idx + 1
            ));
        };
        let key = key.trim();
        let raw_value = raw_value.trim();

        match (section, key) {
            (Section::Terminal, "font_path") => {
                config.terminal.font_path =
                    Some(PathBuf::from(parse_toml_basic_string(raw_value)?));
            }
            (Section::Terminal, "font_size_px") => {
                config.terminal.font_size_px = Some(parse_toml_f32(raw_value)?);
            }
            (Section::Terminal, "baseline_offset_px") => {
                config.terminal.baseline_offset_px = Some(parse_toml_f32(raw_value)?);
            }
            (Section::Window, "title") => {
                config.window.title = Some(parse_toml_basic_string(raw_value)?);
            }
            (Section::Window, "app_id") => {
                config.window.app_id = Some(parse_toml_basic_string(raw_value)?);
            }
            _ => {}
        }
    }

    Ok(config)
}

// Strips TOML comment.
fn strip_toml_comment(line: &str) -> Result<String, String> {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut escape = false;

    for ch in line.chars() {
        if in_string {
            out.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '#' => break,
            '"' => {
                in_string = true;
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }

    if in_string {
        return Err("unterminated NeoZeus config string".to_owned());
    }

    Ok(out)
}

// Parses TOML f32.
fn parse_toml_f32(raw: &str) -> Result<f32, String> {
    raw.parse::<f32>()
        .map_err(|error| format!("NeoZeus config expected float value, got `{raw}`: {error}"))
}

// Parses TOML basic string.
fn parse_toml_basic_string(raw: &str) -> Result<String, String> {
    let Some(raw) = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return Err(format!(
            "NeoZeus config currently expects quoted string values, got `{raw}`"
        ));
    };

    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        let Some(escaped) = chars.next() else {
            return Err("NeoZeus config string ends with trailing escape".to_owned());
        };
        match escaped {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            other => {
                return Err(format!(
                    "unsupported NeoZeus config string escape `\\{other}`"
                ))
            }
        }
    }

    Ok(out)
}
