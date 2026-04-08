use super::{
    setup::append_hud_log,
    state::{HudLayoutState, HudRect},
    widgets::{HudWidgetKey, HUD_WIDGET_DEFINITIONS},
};
use bevy::prelude::*;
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const HUD_LAYOUT_FILENAME: &str = "hud-layout.v1";
const HUD_LAYOUT_VERSION_V1: &str = "version 1";
const HUD_LAYOUT_VERSION_V2: &str = "version 2";
const HUD_LAYOUT_SAVE_DEBOUNCE_SECS: f32 = 0.3;

#[derive(Clone, Debug, PartialEq)]
struct PersistedHudModuleState {
    pub(crate) enabled: bool,
    pub(crate) rect: HudRect,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct PersistedHudState {
    pub(crate) modules: BTreeMap<HudWidgetKey, PersistedHudModuleState>,
}

#[derive(Resource, Default)]
pub(crate) struct HudPersistenceState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
}

/// Resolves the on-disk HUD layout path from explicit XDG/HOME inputs.
///
/// XDG config home wins when present; otherwise the fallback is `$HOME/.config/neozeus/...`.
fn resolve_hud_layout_path_with(
    xdg_config_home: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(xdg) = xdg_config_home.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(xdg).join("neozeus").join(HUD_LAYOUT_FILENAME));
    }

    home.filter(|value| !value.is_empty()).map(|value| {
        PathBuf::from(value)
            .join(".config/neozeus")
            .join(HUD_LAYOUT_FILENAME)
    })
}

/// Resolves the HUD layout path from the real process environment.
///
/// This thin wrapper exists so the actual path policy can be tested without mutating environment
/// variables in every test.
pub(crate) fn resolve_hud_layout_path() -> Option<PathBuf> {
    resolve_hud_layout_path_with(
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
    )
}

/// Parses the legacy line-oriented v1 HUD layout format.
///
/// Unknown modules or malformed numeric fields are skipped instead of aborting the whole load.
fn parse_v1_hud_state(text: &str) -> PersistedHudState {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let mut persisted = PersistedHudState::default();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(module_name) = parts.next() else {
            continue;
        };
        let Some(module_id) = parse_hud_module_id(module_name) else {
            continue;
        };
        let mut enabled = None;
        let mut x = None;
        let mut y = None;
        let mut w = None;
        let mut h = None;
        for part in parts {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            match key {
                "enabled" => enabled = value.parse::<u8>().ok().map(|flag| flag != 0),
                "x" => x = value.parse::<f32>().ok(),
                "y" => y = value.parse::<f32>().ok(),
                "w" => w = value.parse::<f32>().ok(),
                "h" => h = value.parse::<f32>().ok(),
                _ => {}
            }
        }
        let (Some(enabled), Some(x), Some(y), Some(w), Some(h)) = (enabled, x, y, w, h) else {
            continue;
        };
        persisted.modules.insert(
            module_id,
            PersistedHudModuleState {
                enabled,
                rect: HudRect { x, y, w, h },
            },
        );
    }
    persisted
}

/// Parses the current block-oriented v2 HUD layout format.
///
/// Each `[module] ... [/module]` block is accumulated independently so malformed blocks are dropped
/// without poisoning the rest of the file.
fn parse_v2_hud_state(text: &str) -> PersistedHudState {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let mut persisted = PersistedHudState::default();
    let mut module_id = None;
    let mut enabled = None;
    let mut x = None;
    let mut y = None;
    let mut w = None;
    let mut h = None;
    let mut in_module = false;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 {
            continue;
        }

        match line {
            "[module]" => {
                in_module = true;
                module_id = None;
                enabled = None;
                x = None;
                y = None;
                w = None;
                h = None;
            }
            "[/module]" => {
                if in_module {
                    if let (Some(module_id), Some(enabled), Some(x), Some(y), Some(w), Some(h)) =
                        (module_id, enabled, x, y, w, h)
                    {
                        persisted.modules.insert(
                            module_id,
                            PersistedHudModuleState {
                                enabled,
                                rect: HudRect { x, y, w, h },
                            },
                        );
                    }
                }
                in_module = false;
            }
            _ if !in_module => {}
            _ => {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key {
                    "id" => module_id = parse_hud_module_id(value.trim_matches('"')),
                    "enabled" => enabled = value.parse::<u8>().ok().map(|flag| flag != 0),
                    "x" => x = value.parse::<f32>().ok(),
                    "y" => y = value.parse::<f32>().ok(),
                    "w" => w = value.parse::<f32>().ok(),
                    "h" => h = value.parse::<f32>().ok(),
                    _ => {}
                }
            }
        }
    }

    persisted
}

/// Dispatches persisted HUD layout parsing based on the first non-empty version line.
///
/// Unknown versions are logged and treated as empty state rather than hard errors.
fn parse_persisted_hud_state(text: &str) -> PersistedHudState {
    let version_line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    match version_line {
        HUD_LAYOUT_VERSION_V1 => parse_v1_hud_state(text),
        HUD_LAYOUT_VERSION_V2 => parse_v2_hud_state(text),
        line => {
            append_hud_log(format!("hud layout: unexpected version line `{line}`"));
            PersistedHudState::default()
        }
    }
}

/// Serializes persisted HUD layout state into the current v2 text format.
///
/// Modules are emitted in the canonical definition order so files stay stable across saves.
fn serialize_persisted_hud_state(state: &PersistedHudState) -> String {
    let mut output = String::from(HUD_LAYOUT_VERSION_V2);
    output.push('\n');
    for definition in HUD_WIDGET_DEFINITIONS {
        let Some(module) = state.modules.get(&definition.key) else {
            continue;
        };
        output.push_str("[module]\n");
        output.push_str(&format!("id=\"{}\"\n", definition.key.title_key()));
        output.push_str(&format!("enabled={}\n", u8::from(module.enabled)));
        output.push_str(&format!("x={}\n", module.rect.x));
        output.push_str(&format!("y={}\n", module.rect.y));
        output.push_str(&format!("w={}\n", module.rect.w));
        output.push_str(&format!("h={}\n", module.rect.h));
        output.push_str("[/module]\n");
    }
    output
}

/// Maps a persisted module name back to its enum id.
///
/// The accepted names are the stable title keys, not the human-facing titles.
fn parse_hud_module_id(name: &str) -> Option<HudWidgetKey> {
    match name {
        "DebugToolbar" | "InfoBar" => Some(HudWidgetKey::InfoBar),
        "AgentList" => Some(HudWidgetKey::AgentList),
        "ConversationList" => Some(HudWidgetKey::ConversationList),
        "ThreadPane" => Some(HudWidgetKey::ThreadPane),
        _ => None,
    }
}

/// Loads persisted HUD layout state from disk.
///
/// Missing files are treated as "no saved layout"; other I/O failures are logged and also fall back
/// to defaults.
fn load_persisted_hud_state_from(path: &PathBuf) -> PersistedHudState {
    match fs::read_to_string(path) {
        Ok(text) => parse_persisted_hud_state(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => PersistedHudState::default(),
        Err(error) => {
            append_hud_log(format!(
                "hud layout load failed {}: {error}",
                path.display()
            ));
            PersistedHudState::default()
        }
    }
}

/// Loads persisted HUD module enablement/rect overrides from disk.
///
/// Missing files or unreadable data degrade to an empty override map so startup can keep the built-in
/// defaults without having to know about the on-disk representation.
pub(crate) fn load_persisted_hud_modules_from(
    path: &PathBuf,
) -> BTreeMap<HudWidgetKey, (bool, HudRect)> {
    load_persisted_hud_state_from(path)
        .modules
        .into_iter()
        .map(|(key, state)| (key, (state.enabled, state.rect)))
        .collect()
}

/// Debounces and writes HUD layout changes to disk once the layout has settled.
///
/// Active drags defer saving, dirty timestamps start only once, and the persisted snapshot is built
/// from module `target_rect`s so in-flight animations do not leak into the saved layout.
pub(crate) fn save_hud_layout_if_dirty(
    time: Res<Time>,
    mut layout_state: ResMut<HudLayoutState>,
    mut persistence_state: ResMut<HudPersistenceState>,
) {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    if layout_state.drag.is_some() {
        if layout_state.dirty_layout && persistence_state.dirty_since_secs.is_none() {
            persistence_state.dirty_since_secs = Some(time.elapsed_secs());
        }
        return;
    }

    if layout_state.dirty_layout && persistence_state.dirty_since_secs.is_none() {
        persistence_state.dirty_since_secs = Some(time.elapsed_secs());
        return;
    }

    let Some(dirty_since) = persistence_state.dirty_since_secs else {
        return;
    };
    if time.elapsed_secs() - dirty_since < HUD_LAYOUT_SAVE_DEBOUNCE_SECS {
        return;
    }
    let Some(path) = persistence_state.path.as_ref() else {
        layout_state.dirty_layout = false;
        persistence_state.dirty_since_secs = None;
        return;
    };

    let mut persisted = PersistedHudState::default();
    for definition in HUD_WIDGET_DEFINITIONS {
        let Some(layout) = layout_state.module_layout(definition.key) else {
            continue;
        };
        persisted.modules.insert(
            definition.key,
            PersistedHudModuleState {
                enabled: layout.enabled,
                rect: layout.rect,
            },
        );
    }

    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            append_hud_log(format!(
                "hud layout mkdir failed {}: {error}",
                parent.display()
            ));
            layout_state.dirty_layout = false;
            persistence_state.dirty_since_secs = None;
            return;
        }
    }

    let serialized = serialize_persisted_hud_state(&persisted);
    if let Err(error) = fs::write(path, serialized) {
        append_hud_log(format!(
            "hud layout save failed {}: {error}",
            path.display()
        ));
    } else {
        append_hud_log(format!("hud layout saved {}", path.display()));
    }
    layout_state.dirty_layout = false;
    persistence_state.dirty_since_secs = None;
}

#[cfg(test)]
mod tests;
