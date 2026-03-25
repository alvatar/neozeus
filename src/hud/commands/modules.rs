use crate::hud::{HudModuleId, HudModuleRequest, HudState};
use crate::terminals::append_debug_log;
use bevy::prelude::*;

fn toggle_module(hud_state: &mut HudState, id: HudModuleId) {
    let enabled = hud_state
        .get(id)
        .is_some_and(|module| !module.shell.enabled);
    hud_state.set_module_enabled(id, enabled);
}

pub(crate) fn apply_hud_module_requests(
    mut requests: MessageReader<HudModuleRequest>,
    mut hud_state: ResMut<HudState>,
) {
    for request in requests.read() {
        match request {
            HudModuleRequest::Toggle(id) => toggle_module(&mut hud_state, *id),
            HudModuleRequest::Reset(id) => {
                hud_state.reset_module(*id);
                append_debug_log(format!("hud module reset {}", id.number()));
            }
        }
    }
}
