use crate::hud::{HudLayoutState, HudModuleId, HudModuleRequest};
use crate::terminals::append_debug_log;
use bevy::prelude::*;

/// Flips a module between enabled and disabled state.
///
/// The function reads the current shell flag and then writes the opposite through
/// `set_module_enabled`, which keeps any side effects of the layout state's enable/disable path in
/// one place instead of poking the shell directly.
fn toggle_module(layout_state: &mut HudLayoutState, id: HudModuleId) {
    let enabled = layout_state
        .get(id)
        .is_some_and(|module| !module.shell.enabled);
    layout_state.set_module_enabled(id, enabled);
}

/// Applies enable/disable and reset requests for HUD modules.
///
/// Toggle requests are routed through [`toggle_module`] so the enable logic stays centralized. Reset
/// requests delegate to the layout state's reset path and emit a debug log entry, because module
/// resets are significant enough to be useful in the session trace.
pub(crate) fn apply_hud_module_requests(
    mut requests: MessageReader<HudModuleRequest>,
    mut layout_state: ResMut<HudLayoutState>,
) {
    for request in requests.read() {
        match request {
            HudModuleRequest::Toggle(id) => toggle_module(&mut layout_state, *id),
            HudModuleRequest::Reset(id) => {
                layout_state.reset_module(*id);
                append_debug_log(format!("hud module reset {}", id.number()));
            }
        }
    }
}
