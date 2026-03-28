use bevy_egui::egui;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub(crate) const DEFAULT_COLS: u16 = 120;
pub(crate) const DEFAULT_ROWS: u16 = 38;
pub(crate) const DEFAULT_BG: egui::Color32 = egui::Color32::from_rgb(24, 32, 30);
pub(crate) const DEFAULT_CELL_HEIGHT_PX: u32 = 16;
pub(crate) const DEFAULT_CELL_WIDTH_PX: u32 = 9;
pub(crate) const GPU_NOT_FOUND_PANIC_FRAGMENT: &str = "Unable to find a GPU!";

pub(crate) const DEBUG_LOG_PATH: &str = "/tmp/neozeus-debug.log";
pub(crate) const DEBUG_TEXTURE_DUMP_PATH: &str = "/tmp/neozeus-texture.ppm";
const NEOZEUS_CONFIG_FILENAME: &str = "config.toml";
const NEOZEUS_CWD_CONFIG_FILENAME: &str = "neozeus.toml";

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NeoZeusConfig {
    terminal: NeoZeusTerminalConfig,
    window: NeoZeusWindowConfig,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct NeoZeusTerminalConfig {
    pub(crate) font_path: Option<PathBuf>,
    pub(crate) font_size_px: Option<f32>,
    pub(crate) baseline_offset_px: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct NeoZeusWindowConfig {
    pub(crate) title: Option<String>,
    pub(crate) app_id: Option<String>,
}

impl NeoZeusConfig {
    /// Returns the configured terminal font path, if any.
    fn terminal_font_path(&self) -> Option<&Path> {
        self.terminal.font_path.as_deref()
    }

    /// Returns the configured terminal font size, if any.
    fn terminal_font_size_px(&self) -> Option<f32> {
        self.terminal.font_size_px
    }

    /// Returns the configured terminal baseline offset, if any.
    fn terminal_baseline_offset_px(&self) -> Option<f32> {
        self.terminal.baseline_offset_px
    }

    /// Returns the configured window title, if any.
    fn window_title(&self) -> Option<&str> {
        self.window.title.as_deref()
    }

    /// Returns the configured app id, if any.
    fn window_app_id(&self) -> Option<&str> {
        self.window.app_id.as_deref()
    }
}

/// Loads the first NeoZeus config file that exists, or returns the all-default config when none is
/// present.
///
/// This is the top-level convenience entry point used by bootstrap. Path resolution is delegated to
/// [`resolve_neozeus_config_path`], and only an actually found file is parsed; absence is not treated
/// as an error.
pub(crate) fn load_neozeus_config() -> Result<NeoZeusConfig, String> {
    let Some(path) = resolve_neozeus_config_path() else {
        return Ok(NeoZeusConfig::default());
    };
    load_neozeus_config_from(&path)
}

/// Reads and parses a specific NeoZeus config file from disk.
///
/// The function keeps I/O and parsing errors separate in the final message by first attaching the
/// filesystem path to any read failure and then delegating the actual syntax handling to
/// [`parse_neozeus_config`].
fn load_neozeus_config_from(path: &Path) -> Result<NeoZeusConfig, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read NeoZeus config {}: {error}", path.display()))?;
    parse_neozeus_config(&text)
}

/// Resolves the effective config path using the process environment and current working directory.
///
/// This wrapper gathers the live process state and forwards it to
/// [`resolve_neozeus_config_path_with`], which contains the actual precedence rules and is easier to
/// test deterministically.
fn resolve_neozeus_config_path() -> Option<PathBuf> {
    let current_dir = env::current_dir().ok();
    resolve_neozeus_config_path_with(
        env::var_os("NEOZEUS_CONFIG_PATH").as_deref(),
        env::var_os("XDG_CONFIG_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
        current_dir.as_deref(),
    )
}

/// Implements the config-file search order used by NeoZeus.
///
/// Precedence is strict and stops at the first existing file: explicit `NEOZEUS_CONFIG_PATH`, then
/// `$XDG_CONFIG_HOME/neozeus/config.toml`, then `~/.config/neozeus/config.toml`, and finally a local
/// `neozeus.toml` in the current directory. Non-existent candidates are skipped silently.
fn resolve_neozeus_config_path_with(
    explicit_path: Option<&std::ffi::OsStr>,
    xdg_config_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    current_dir: Option<&Path>,
) -> Option<PathBuf> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
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

/// Resolves the human-visible window title with environment overrides taking precedence.
///
/// Empty override values are ignored on purpose so shell wrappers can export the variable without
/// forcing an empty title. If neither the environment nor the config file specifies a title, the
/// application falls back to `neozeus`.
pub(crate) fn resolve_window_title(config: &NeoZeusConfig) -> String {
    env::var("NEOZEUS_WINDOW_TITLE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| config.window_title().map(str::to_owned))
        .unwrap_or_else(|| "neozeus".to_owned())
}

/// Resolves the native app/window id with the same precedence model as the visible title.
///
/// This value is what window managers see, so environment overrides win, config-file values come
/// next, and the hard-coded `neozeus` fallback keeps the identity stable when nothing is specified.
pub(crate) fn resolve_app_id(config: &NeoZeusConfig) -> String {
    env::var("NEOZEUS_APP_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| config.window_app_id().map(str::to_owned))
        .unwrap_or_else(|| "neozeus".to_owned())
}

/// Resolves the explicit terminal font path, if one has been configured.
///
/// The environment variable wins over the config file, and an empty environment value is treated as
/// absent rather than as an instruction to clear the path. If neither source specifies a font file,
/// the renderer is left to its normal font-discovery path.
pub(crate) fn resolve_terminal_font_path(config: &NeoZeusConfig) -> Option<PathBuf> {
    env::var_os("NEOZEUS_TERMINAL_FONT_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| config.terminal_font_path().map(PathBuf::from))
}

/// Resolves the terminal font size in pixels with validation and fallback.
///
/// The function prefers the environment override, falls back to the parsed config value, and rejects
/// anything non-finite or non-positive. If neither source yields a valid size, the caller-provided
/// default is returned unchanged.
pub(crate) fn resolve_terminal_font_size_px(config: &NeoZeusConfig, default: f32) -> f32 {
    env::var("NEOZEUS_TERMINAL_FONT_SIZE_PX")
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .or(config.terminal_font_size_px())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(default)
}

/// Resolves the terminal baseline offset override.
///
/// Unlike font size, zero and negative finite values are allowed because a baseline offset is a
/// signed adjustment. Only non-finite inputs are discarded before falling back through environment,
/// config, and finally the supplied default.
pub(crate) fn resolve_terminal_baseline_offset_px(config: &NeoZeusConfig, default: f32) -> f32 {
    env::var("NEOZEUS_TERMINAL_BASELINE_OFFSET_PX")
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .or(config.terminal_baseline_offset_px())
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

/// Parses the small NeoZeus TOML subset supported by the project.
///
/// This is intentionally not a full TOML parser. It walks the file line by line, strips comments
/// while respecting quoted strings, tracks only the sections NeoZeus understands, and ignores all
/// unknown keys/sections. Syntax errors that would make the supported subset ambiguous still return
/// explicit errors with line numbers.
fn parse_neozeus_config(text: &str) -> Result<NeoZeusConfig, String> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
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

/// Removes a trailing `# ...` comment from one config line while respecting quoted strings.
///
/// The implementation is a tiny state machine that tracks whether it is inside a basic string and
/// whether the previous character was an escape. That is the important part: `#` only starts a
/// comment outside strings, and unterminated strings are reported as an error.
fn strip_toml_comment(line: &str) -> Result<String, String> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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

/// Parses a raw scalar as `f32` and decorates any failure with config-specific context.
///
/// This helper exists mainly so the higher-level parser can keep its error messages uniform instead
/// of repeating the same formatting boilerplate for every numeric field.
fn parse_toml_f32(raw: &str) -> Result<f32, String> {
    raw.parse::<f32>()
        .map_err(|error| format!("NeoZeus config expected float value, got `{raw}`: {error}"))
}

/// Parses the restricted double-quoted string syntax supported by the hand-written config parser.
///
/// The function accepts only TOML basic strings with a small escape set (`\"`, `\\`, `\n`, `\r`,
/// `\t`). That limited scope is intentional: NeoZeus only needs a few string fields and does not
/// want a full TOML dependency just for them.
fn parse_toml_basic_string(raw: &str) -> Result<String, String> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
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

#[cfg(test)]
mod tests;
