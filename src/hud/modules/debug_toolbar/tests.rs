use super::*;
use crate::hud::{DebugToolbarView, HudRect, HudState, HudWidgetKey};
use bevy::prelude::*;

/// Verifies that the info bar ignores pointer clicks while it is intentionally empty.
#[test]
fn info_bar_click_does_not_emit_commands() {
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );

    let mut emitted_commands = Vec::new();
    handle_pointer_click(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 1280.0,
            h: 40.0,
        },
        Vec2::new(120.0, 20.0),
        &DebugToolbarView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert!(emitted_commands.is_empty());
}
