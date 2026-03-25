use crate::hud::{HudLayoutState, HudModuleId, HudModuleRequest};
use crate::terminals::append_debug_log;
use bevy::prelude::*;

fn toggle_module(layout_state: &mut HudLayoutState, id: HudModuleId) {
    let enabled = layout_state
        .get(id)
        .is_some_and(|module| !module.shell.enabled);
    layout_state.set_module_enabled(id, enabled);
}

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
