use super::{
    setup::append_hud_log,
    state::{HudLayoutState, HudRect},
    widgets::{HudWidgetKey, HUD_WIDGET_DEFINITIONS},
};
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use crate::shared::persistence::{
    first_non_empty_trimmed_line, load_text_file_or_default, mark_dirty_since,
    non_empty_trimmed_lines_after_header, resolve_config_path_with, save_debounce_elapsed,
    write_file_atomically,
};

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
    resolve_config_path_with(xdg_config_home, home, "neozeus", HUD_LAYOUT_FILENAME)
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
    for line in non_empty_trimmed_lines_after_header(text) {
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

    for line in non_empty_trimmed_lines_after_header(text) {
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
    let version_line = first_non_empty_trimmed_line(text);
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
fn load_persisted_hud_state_from(path: &Path) -> PersistedHudState {
    load_text_file_or_default(path, parse_persisted_hud_state, |path, error| {
        append_hud_log(format!(
            "hud layout load failed {}: {error}",
            path.display()
        ));
    })
}

/// Loads persisted HUD module enablement/rect overrides from disk.
///
/// Missing files or unreadable data degrade to an empty override map so startup can keep the built-in
/// defaults without having to know about the on-disk representation.
pub(crate) fn load_persisted_hud_modules_from(
    path: &Path,
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
    let started_dirty_window =
        layout_state.dirty_layout && persistence_state.dirty_since_secs.is_none();
    if layout_state.dirty_layout {
        mark_dirty_since(&mut persistence_state.dirty_since_secs, Some(&time));
    }
    if layout_state.drag.is_some() || started_dirty_window {
        return;
    }

    if !save_debounce_elapsed(
        persistence_state.dirty_since_secs,
        time.elapsed_secs(),
        HUD_LAYOUT_SAVE_DEBOUNCE_SECS,
    ) {
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
    if let Err(error) = write_file_atomically(path, &serialized) {
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
mod tests {
    use super::*;
    use super::super::{
        state::{default_hud_module_instance, HudLayoutState, HudRect, HudState},
        widgets::{HudWidgetDefinition, HudWidgetKey, HUD_WIDGET_DEFINITIONS},
    };
    use crate::tests::{insert_test_hud_state, temp_dir};
    use bevy::{
        ecs::system::RunSystemOnce,
        prelude::{Time, World},
    };
    use std::{fs, path::PathBuf, time::Duration};

    /// Applies persisted module enablement/rect overrides onto a set of module definitions.
    fn apply_persisted_layout(
        definitions: &[HudWidgetDefinition],
        persisted: &PersistedHudState,
    ) -> HudLayoutState {
        let mut hud_state = HudLayoutState::default();
        for definition in definitions {
            let mut module = default_hud_module_instance(definition);
            if let Some(saved) = persisted.modules.get(&definition.key) {
                module.shell.enabled = saved.enabled;
                module.shell.target_rect = saved.rect;
                module.shell.current_rect = saved.rect;
                module.shell.target_alpha = if saved.enabled { 1.0 } else { 0.0 };
                module.shell.current_alpha = module.shell.target_alpha;
            }
            hud_state.insert(definition.key, module);
        }
        hud_state
    }

    /// Verifies the config-path search order for persisted HUD layouts.
    ///
    /// XDG config takes precedence when present, otherwise the code falls back to `$HOME/.config`, and
    /// with neither variable available persistence is disabled.
    #[test]
    fn hud_layout_path_prefers_xdg_then_home() {
        assert_eq!(
            resolve_hud_layout_path_with(Some("/tmp/xdg"), Some("/tmp/home")),
            Some(PathBuf::from("/tmp/xdg/neozeus/hud-layout.v1"))
        );
        assert_eq!(
            resolve_hud_layout_path_with(None, Some("/tmp/home")),
            Some(PathBuf::from("/tmp/home/.config/neozeus/hud-layout.v1"))
        );
        assert_eq!(resolve_hud_layout_path_with(None, None), None);
    }

    /// Verifies that persisted HUD layout serialization round-trips through the current v2 format.
    ///
    /// This locks down both field coverage and the parser/serializer pairing for modern layout files.
    #[test]
    fn hud_layout_parse_and_serialize_roundtrip() {
        let mut persisted = PersistedHudState::default();
        persisted.modules.insert(
            HudWidgetKey::AgentList,
            PersistedHudModuleState {
                enabled: true,
                rect: HudRect {
                    x: 24.0,
                    y: 96.0,
                    w: 300.0,
                    h: 420.0,
                },
            },
        );
        let text = serialize_persisted_hud_state(&persisted);
        assert_eq!(parse_persisted_hud_state(&text), persisted);
    }

    /// Verifies backward compatibility with the legacy v1 HUD layout format.
    ///
    /// Old persisted layouts should still load even though new saves use the v2 block-based format.
    #[test]
    fn hud_layout_v1_parser_remains_backward_compatible() {
        let persisted =
            parse_persisted_hud_state("version 1\nAgentList enabled=1 x=24 y=96 w=300 h=420\n");
        let module = persisted.modules.get(&HudWidgetKey::AgentList).unwrap();
        assert!(module.enabled);
        assert_eq!(module.rect.w, 300.0);
    }

    /// Verifies that loading persisted layout data overrides the built-in module defaults.
    ///
    /// The test checks both enablement and rect replacement so defaults are not silently kept when saved
    /// data exists.
    #[test]
    fn apply_persisted_layout_overrides_defaults() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let mut persisted = PersistedHudState::default();
        persisted.modules.insert(
            HudWidgetKey::AgentList,
            PersistedHudModuleState {
                enabled: false,
                rect: HudRect {
                    x: 11.0,
                    y: 22.0,
                    w: 333.0,
                    h: 444.0,
                },
            },
        );
        let hud_state = apply_persisted_layout(HUD_WIDGET_DEFINITIONS.as_slice(), &persisted);
        let module = hud_state.get(HudWidgetKey::AgentList).unwrap();
        assert!(!module.shell.enabled);
        assert_eq!(module.shell.target_rect.x, 11.0);
        assert_eq!(module.shell.target_rect.w, 333.0);
    }

    /// Verifies that saving HUD layout writes the target rect, not the transient animated rect.
    ///
    /// Persistence should capture the intended stable layout, which lives in `target_rect` while animation
    /// may still be catching up.
    #[test]
    fn saving_hud_layout_persists_target_rect() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let dir = temp_dir("neozeus-hud-layout-save");
        let path = dir.join("hud-layout.v1");
        let mut world = World::default();
        let mut hud_state = HudState::default();
        let mut module = default_hud_module_instance(&HUD_WIDGET_DEFINITIONS[1]);
        module.shell.target_rect = HudRect {
            x: 321.0,
            y: 222.0,
            w: 333.0,
            h: 444.0,
        };
        hud_state.insert(HudWidgetKey::AgentList, module);
        hud_state.dirty_layout = true;
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        world.insert_resource(time);
        insert_test_hud_state(&mut world, hud_state);
        world.insert_resource(HudPersistenceState {
            path: Some(path.clone()),
            dirty_since_secs: None,
        });

        world.run_system_once(save_hud_layout_if_dirty).unwrap();
        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        world.run_system_once(save_hud_layout_if_dirty).unwrap();

        let serialized = fs::read_to_string(&path).expect("hud layout file missing");
        assert!(serialized.contains("version 2"));
        assert!(serialized.contains("[module]"));
        assert!(serialized.contains("id=\"AgentList\""));
        assert!(serialized.contains("enabled=1"));
        assert!(serialized.contains("x=321"));
        assert!(serialized.contains("y=222"));
        assert!(serialized.contains("w=333"));
        assert!(serialized.contains("h=444"));

        let persisted = parse_persisted_hud_state(&serialized);
        let restored = apply_persisted_layout(HUD_WIDGET_DEFINITIONS.as_slice(), &persisted);
        let restored_module = restored.get(HudWidgetKey::AgentList).unwrap();
        assert_eq!(restored_module.shell.target_rect.x, 321.0);
        assert_eq!(restored_module.shell.target_rect.h, 444.0);
    }
}
