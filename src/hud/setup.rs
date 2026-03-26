use crate::hud::{
    default_hud_module_instance, docked_agent_list_rect, setup_hud_offscreen_compositor,
    HudInputCaptureState, HudLayoutState, HudMessageBoxState, HudModalCameraMarker, HudModalState,
    HudModalVectorSceneMarker, HudModuleId, HudOffscreenCompositor, HudPersistenceState,
    HudTaskDialogState, HudVectorSceneMarker, HUD_MODAL_CAMERA_ORDER, HUD_MODAL_RENDER_LAYER,
    HUD_MODULE_DEFINITIONS,
};
use bevy::{
    camera::{visibility::NoFrustumCulling, ClearColorConfig},
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use bevy_vello::prelude::{VelloScene2d, VelloView};

pub(crate) fn append_hud_log(message: impl AsRef<str>) {
    crate::terminals::append_debug_log(format!("hud: {}", message.as_ref()));
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD setup initializes retained state, persistence, compositor, and scene resources together"
)]
pub(crate) fn setup_hud(
    mut commands: Commands,
    mut layout_state: ResMut<HudLayoutState>,
    mut modal_state: ResMut<HudModalState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut persistence_state: ResMut<HudPersistenceState>,
    mut compositor: ResMut<HudOffscreenCompositor>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut composite_materials: ResMut<Assets<bevy_vello::render::VelloCanvasMaterial>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    persistence_state.path = crate::hud::persistence::resolve_hud_layout_path();
    let persisted = persistence_state
        .path
        .as_ref()
        .map(crate::hud::persistence::load_persisted_hud_state_from)
        .unwrap_or_default();
    layout_state.modules.clear();
    layout_state.z_order.clear();
    layout_state.drag = None;
    layout_state.dirty_layout = false;
    modal_state.message_box = HudMessageBoxState::default();
    modal_state.task_dialog = HudTaskDialogState::default();
    input_capture.direct_input_terminal = None;
    for definition in HUD_MODULE_DEFINITIONS.iter() {
        let mut module = default_hud_module_instance(definition);
        if let Some(saved) = persisted.modules.get(&definition.id) {
            module.shell.enabled = saved.enabled;
            module.shell.target_rect = saved.rect;
            module.shell.current_rect = saved.rect;
            module.shell.target_alpha = if saved.enabled { 1.0 } else { 0.0 };
            module.shell.current_alpha = module.shell.target_alpha;
        }
        layout_state.insert(definition.id, module);
    }

    commands.spawn((
        VelloScene2d::default(),
        Transform::from_xyz(0.0, 0.0, 50.0),
        NoFrustumCulling,
        HudVectorSceneMarker,
    ));
    commands.spawn((
        VelloScene2d::default(),
        Transform::from_xyz(0.0, 0.0, 60.0),
        NoFrustumCulling,
        bevy::camera::visibility::RenderLayers::layer(HUD_MODAL_RENDER_LAYER),
        HudModalVectorSceneMarker,
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: HUD_MODAL_CAMERA_ORDER,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        VelloView,
        bevy::camera::visibility::RenderLayers::layer(HUD_MODAL_RENDER_LAYER),
        HudModalCameraMarker,
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
    mut layout_state: ResMut<HudLayoutState>,
) {
    let rect = docked_agent_list_rect(&primary_window);
    let Some(agent_list) = layout_state.get_mut(HudModuleId::AgentList) else {
        return;
    };
    agent_list.shell.target_rect = rect;
    agent_list.shell.current_rect = rect;
}

pub(crate) fn hud_needs_redraw(layout_state: &HudLayoutState) -> bool {
    layout_state.drag.is_some() || layout_state.is_animating()
}
