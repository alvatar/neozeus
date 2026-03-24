mod animation;
mod bloom;
mod capture;
mod compositor;
mod dispatcher;
mod input;
mod message_box;
mod messages;
mod modules;
mod persistence;
mod render;
mod state;

pub(crate) use animation::animate_hud_modules;
pub(crate) use bloom::{
    setup_hud_widget_bloom, sync_hud_widget_bloom, AgentListBloomBlurMaterial, HudBloomSettings,
    HudWidgetBloom,
};
#[cfg(test)]
pub(crate) use bloom::{
    AgentListBloomBlurUniform, AgentListBloomCameraMarker, AgentListBloomCompositeMarker,
    AgentListBloomSourceKind, AgentListBloomSourceSprite,
};
pub(crate) use capture::{
    finalize_window_capture, request_hud_composite_capture, request_hud_texture_capture,
    request_window_capture, HudCompositeCaptureConfig, HudTextureCaptureConfig,
    WindowCaptureConfig,
};
pub(crate) use compositor::{
    setup_hud_offscreen_compositor, sync_hud_offscreen_compositor, HudOffscreenCompositor,
};
#[cfg(test)]
pub(crate) use compositor::{
    HudCompositeCameraMarker, HudCompositeLayerId, HudCompositeLayerMarker,
    HUD_COMPOSITE_FOREGROUND_Z, HUD_COMPOSITE_RENDER_LAYER,
};
#[cfg(test)]
pub(crate) use dispatcher::kill_active_terminal;
pub(crate) use dispatcher::{
    apply_hud_module_requests, apply_terminal_focus_requests, apply_terminal_lifecycle_requests,
    apply_terminal_send_requests, apply_terminal_task_requests, apply_terminal_view_requests,
    apply_visibility_requests, dispatch_hud_intents,
};
pub(crate) use input::{handle_hud_module_shortcuts, handle_hud_pointer_input};
pub(crate) use message_box::{
    message_box_action_at, message_box_action_buttons, message_box_rect, task_dialog_action_at,
    task_dialog_action_buttons, task_dialog_rect, HudMessageBoxAction, HudMessageBoxState,
    HudTaskDialogAction, HudTaskDialogState,
};
pub(crate) use messages::{
    HudIntent, HudModuleRequest, TerminalFocusRequest, TerminalLifecycleRequest,
    TerminalSendRequest, TerminalTaskRequest, TerminalViewRequest, TerminalVisibilityRequest,
};
#[cfg(test)]
pub(crate) use persistence::{
    apply_persisted_layout, parse_persisted_hud_state, resolve_hud_layout_path_with,
    serialize_persisted_hud_state, PersistedHudModuleState, PersistedHudState,
};
pub(crate) use persistence::{save_hud_layout_if_dirty, HudPersistenceState};
pub(crate) use render::{render_hud_scene, HudVectorSceneMarker};
pub(crate) use state::{
    default_hud_module_instance, docked_agent_list_rect, AgentDirectory, HudDragState, HudModuleId,
    HudModuleModel, HudRect, HudState, TerminalVisibilityPolicy, TerminalVisibilityState,
    HUD_BUTTON_GAP, HUD_BUTTON_HEIGHT, HUD_BUTTON_MIN_WIDTH, HUD_MODULE_DEFINITIONS,
    HUD_MODULE_PADDING, HUD_ROW_HEIGHT, HUD_TITLEBAR_HEIGHT,
};

use bevy::{
    camera::visibility::NoFrustumCulling,
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use bevy_vello::prelude::VelloScene2d;

pub(crate) fn append_hud_log(message: impl AsRef<str>) {
    crate::terminals::append_debug_log(format!("hud: {}", message.as_ref()));
}

pub(crate) fn setup_hud(
    mut commands: Commands,
    mut hud_state: ResMut<HudState>,
    mut persistence_state: ResMut<HudPersistenceState>,
    mut compositor: ResMut<HudOffscreenCompositor>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut composite_materials: ResMut<Assets<bevy_vello::render::VelloCanvasMaterial>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    persistence_state.path = persistence::resolve_hud_layout_path();
    let persisted = persistence_state
        .path
        .as_ref()
        .map(persistence::load_persisted_hud_state_from)
        .unwrap_or_default();
    hud_state.modules.clear();
    hud_state.z_order.clear();
    hud_state.drag = None;
    hud_state.dirty_layout = false;
    hud_state.message_box = HudMessageBoxState::default();
    hud_state.task_dialog = HudTaskDialogState::default();
    hud_state.direct_input_terminal = None;
    for definition in HUD_MODULE_DEFINITIONS.iter() {
        let mut module = default_hud_module_instance(definition);
        if let Some(saved) = persisted.modules.get(&definition.id) {
            module.shell.enabled = saved.enabled;
            module.shell.target_rect = saved.rect;
            module.shell.current_rect = saved.rect;
            module.shell.target_alpha = if saved.enabled { 1.0 } else { 0.0 };
            module.shell.current_alpha = module.shell.target_alpha;
        }
        hud_state.insert(definition.id, module);
    }

    commands.spawn((
        VelloScene2d::default(),
        Transform::from_xyz(0.0, 0.0, 50.0),
        NoFrustumCulling,
        HudVectorSceneMarker,
    ));
    setup_hud_offscreen_compositor(
        &mut commands,
        &mut compositor,
        &mut meshes,
        &mut composite_materials,
    );
    redraws.write(RequestRedraw);
}

pub(crate) fn sync_structural_hud_layout(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut hud_state: ResMut<HudState>,
) {
    let rect = docked_agent_list_rect(&primary_window);
    let Some(agent_list) = hud_state.get_mut(HudModuleId::AgentList) else {
        return;
    };
    agent_list.shell.target_rect = rect;
    agent_list.shell.current_rect = rect;
}

pub(crate) fn hud_needs_redraw(hud_state: &HudState) -> bool {
    hud_state.drag.is_some() || hud_state.is_animating()
}

#[cfg(test)]
pub(crate) use bloom::{
    agent_list_bloom_layer, agent_list_bloom_z, resolve_agent_list_bloom_debug_previews,
    resolve_agent_list_bloom_intensity,
};
#[cfg(test)]
pub(crate) use modules::{
    agent_row_rect, agent_rows, debug_toolbar_buttons,
    handle_pointer_click as dispatch_hud_pointer_click, handle_scroll as dispatch_hud_scroll,
    resolve_agent_label, AgentListRowSection, AGENT_LIST_BLOOM_RED_B, AGENT_LIST_BLOOM_RED_G,
    AGENT_LIST_BLOOM_RED_R, AGENT_LIST_BORDER_ORANGE_B, AGENT_LIST_BORDER_ORANGE_G,
    AGENT_LIST_BORDER_ORANGE_R,
};
