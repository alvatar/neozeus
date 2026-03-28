use super::animate_hud_modules;
use crate::{
    hud::{HudState, HudWidgetKey},
    tests::{insert_test_hud_state, snapshot_test_hud_state},
};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use std::time::Duration;

/// Verifies that one animation tick moves both HUD rect position and alpha toward their targets.
#[test]
fn animate_hud_modules_moves_current_rect_and_alpha_toward_target() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    let mut module =
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]);
    module.shell.current_rect.x = 24.0;
    module.shell.target_rect.x = 124.0;
    module.shell.current_alpha = 0.2;
    module.shell.target_alpha = 1.0;
    hud_state.insert(HudWidgetKey::AgentList, module);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_test_hud_state(&mut world, hud_state);

    world.run_system_once(animate_hud_modules).unwrap();

    let hud_state = snapshot_test_hud_state(&world);
    let module = hud_state
        .get(HudWidgetKey::AgentList)
        .expect("agent list should exist");
    assert!(module.shell.current_rect.x > 24.0);
    assert!(module.shell.current_rect.x < 124.0);
    assert!(module.shell.current_alpha > 0.2);
    assert!(module.shell.current_alpha < 1.0);
}
