use crate::hud::HudLayoutState;
use bevy::prelude::*;

pub(crate) fn animate_hud_modules(time: Res<Time>, mut layout_state: ResMut<HudLayoutState>) {
    let blend = 1.0 - (-time.delta_secs() * 14.0).exp();
    for module in layout_state.modules.values_mut() {
        module.shell.current_rect.x +=
            (module.shell.target_rect.x - module.shell.current_rect.x) * blend;
        module.shell.current_rect.y +=
            (module.shell.target_rect.y - module.shell.current_rect.y) * blend;
        module.shell.current_rect.w +=
            (module.shell.target_rect.w - module.shell.current_rect.w) * blend;
        module.shell.current_rect.h +=
            (module.shell.target_rect.h - module.shell.current_rect.h) * blend;
        module.shell.current_alpha +=
            (module.shell.target_alpha - module.shell.current_alpha) * blend;
    }
}
