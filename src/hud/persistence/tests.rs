use super::*;
use crate::hud::{HudModuleId, HudRect};
use crate::tests::{insert_test_hud_state, temp_dir};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::{Time, World},
};
use std::{fs, path::PathBuf, time::Duration};

// Verifies that HUD layout path prefers XDG then home.
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

// Verifies that HUD layout parse and serialize roundtrip.
#[test]
fn hud_layout_parse_and_serialize_roundtrip() {
    let mut persisted = PersistedHudState::default();
    persisted.modules.insert(
        HudModuleId::AgentList,
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

// Verifies that HUD layout v1 parser remains backward compatible.
#[test]
fn hud_layout_v1_parser_remains_backward_compatible() {
    let persisted =
        parse_persisted_hud_state("version 1\nAgentList enabled=1 x=24 y=96 w=300 h=420\n");
    let module = persisted.modules.get(&HudModuleId::AgentList).unwrap();
    assert!(module.enabled);
    assert_eq!(module.rect.w, 300.0);
}

// Verifies that apply persisted layout overrides defaults.
#[test]
fn apply_persisted_layout_overrides_defaults() {
    let mut persisted = PersistedHudState::default();
    persisted.modules.insert(
        HudModuleId::AgentList,
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
    let hud_state =
        apply_persisted_layout(crate::hud::HUD_MODULE_DEFINITIONS.as_slice(), &persisted);
    let module = hud_state.get(HudModuleId::AgentList).unwrap();
    assert!(!module.shell.enabled);
    assert_eq!(module.shell.target_rect.x, 11.0);
    assert_eq!(module.shell.target_rect.w, 333.0);
}

// Verifies that saving HUD layout persists target rect.
#[test]
fn saving_hud_layout_persists_target_rect() {
    let dir = temp_dir("neozeus-hud-layout-save");
    let path = dir.join("hud-layout.v1");
    let mut world = World::default();
    let mut hud_state = crate::hud::HudState::default();
    let mut module =
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]);
    module.shell.target_rect = HudRect {
        x: 321.0,
        y: 222.0,
        w: 333.0,
        h: 444.0,
    };
    hud_state.insert(HudModuleId::AgentList, module);
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
    let restored =
        apply_persisted_layout(crate::hud::HUD_MODULE_DEFINITIONS.as_slice(), &persisted);
    let restored_module = restored.get(HudModuleId::AgentList).unwrap();
    assert_eq!(restored_module.shell.target_rect.x, 321.0);
    assert_eq!(restored_module.shell.target_rect.h, 444.0);
}
