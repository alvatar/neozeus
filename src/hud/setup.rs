use crate::app::AppSessionState;

use super::{
    compositor::{setup_hud_offscreen_compositor, HudOffscreenCompositor},
    persistence::{load_persisted_hud_modules_from, resolve_hud_layout_path, HudPersistenceState},
    render::{
        HudModalCameraMarker, HudModalVectorSceneMarker, HudOverlayCameraMarker,
        HudOverlayVectorSceneMarker, HudVectorSceneMarker, HUD_MODAL_CAMERA_ORDER,
        HUD_MODAL_RENDER_LAYER, HUD_OVERLAY_CAMERA_ORDER, HUD_OVERLAY_RENDER_LAYER,
    },
    render_layer::{HudLayerCameraMarker, HudLayerId, HudLayerRegistry, HudLayerSceneMarker},
    state::{
        default_hud_module_instance, docked_agent_list_rect_with_top_inset, docked_info_bar_rect,
        AgentListUiState, ConversationListUiState, HudInputCaptureState, HudLayoutState,
        InfoBarUiState, ThreadPaneUiState,
    },
    widgets::{HudWidgetKey, HUD_WIDGET_DEFINITIONS},
};
use bevy::{
    camera::{visibility::NoFrustumCulling, ClearColorConfig},
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use bevy_vello::prelude::{VelloScene2d, VelloView};

/// Prefixes a message as HUD-related and forwards it to the shared terminal debug log.
///
/// HUD systems use this instead of talking to the terminal logger directly so HUD-originated messages
/// remain easy to spot.
pub(crate) fn append_hud_log(message: impl AsRef<str>) {
    crate::terminals::append_debug_log(format!("hud: {}", message.as_ref()));
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD setup initializes retained state, persistence, compositor, and scene resources together"
)]
/// Initializes retained HUD resources, restores persisted layout, and spawns the HUD scene entities.
///
/// Startup resets any leftover modal/input state, applies persisted module rect/enablement overrides,
/// creates the main and modal Vello scenes plus the modal camera, and then boots the offscreen
/// compositor.
pub(crate) fn setup_hud(
    mut commands: Commands,
    mut layout_state: ResMut<HudLayoutState>,
    mut agent_list_state: ResMut<AgentListUiState>,
    mut conversation_list_state: ResMut<ConversationListUiState>,
    mut _info_bar_state: ResMut<InfoBarUiState>,
    mut _thread_pane_state: ResMut<ThreadPaneUiState>,
    mut app_session: ResMut<AppSessionState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut persistence_state: ResMut<HudPersistenceState>,
    mut compositor: ResMut<HudOffscreenCompositor>,
    mut layers: ResMut<HudLayerRegistry>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut composite_materials: ResMut<Assets<bevy_vello::render::VelloCanvasMaterial>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    persistence_state.path = resolve_hud_layout_path();
    let persisted = persistence_state
        .path
        .as_ref()
        .map(|path| load_persisted_hud_modules_from(path))
        .unwrap_or_default();
    layout_state.modules.clear();
    layout_state.z_order.clear();
    layout_state.drag = None;
    layout_state.dirty_layout = false;
    app_session.composer = crate::composer::ComposerState::default();
    *agent_list_state = AgentListUiState::default();
    *conversation_list_state = ConversationListUiState::default();
    *_info_bar_state = InfoBarUiState;
    *_thread_pane_state = ThreadPaneUiState;
    input_capture.direct_input_terminal = None;
    for definition in HUD_WIDGET_DEFINITIONS.iter() {
        let mut module = default_hud_module_instance(definition);
        if let Some((enabled, rect)) = persisted.get(&definition.key) {
            module.shell.enabled = *enabled;
            module.shell.set_canonical_rect(*rect, true);
            module
                .shell
                .set_canonical_alpha(if *enabled { 1.0 } else { 0.0 }, true);
        }
        layout_state.insert(definition.key, module);
    }

    let main_scene = commands
        .spawn((
            VelloScene2d::default(),
            Transform::from_xyz(0.0, 0.0, 50.0),
            NoFrustumCulling,
            HudVectorSceneMarker,
            HudLayerSceneMarker {
                id: HudLayerId::Main,
            },
        ))
        .id();
    layers.set_scene_entity(HudLayerId::Main, main_scene);

    let overlay_scene = commands
        .spawn((
            VelloScene2d::default(),
            Transform::from_xyz(0.0, 0.0, 55.0),
            NoFrustumCulling,
            bevy::camera::visibility::RenderLayers::layer(HUD_OVERLAY_RENDER_LAYER),
            HudOverlayVectorSceneMarker,
            HudLayerSceneMarker {
                id: HudLayerId::Overlay,
            },
        ))
        .id();
    layers.set_scene_entity(HudLayerId::Overlay, overlay_scene);

    let overlay_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: HUD_OVERLAY_CAMERA_ORDER,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            VelloView,
            bevy::camera::visibility::RenderLayers::layer(HUD_OVERLAY_RENDER_LAYER),
            HudOverlayCameraMarker,
            HudLayerCameraMarker {
                id: HudLayerId::Overlay,
            },
        ))
        .id();
    layers.set_camera_entity(HudLayerId::Overlay, overlay_camera);

    let modal_scene = commands
        .spawn((
            VelloScene2d::default(),
            Transform::from_xyz(0.0, 0.0, 60.0),
            NoFrustumCulling,
            bevy::camera::visibility::RenderLayers::layer(HUD_MODAL_RENDER_LAYER),
            HudModalVectorSceneMarker,
            HudLayerSceneMarker {
                id: HudLayerId::Modal,
            },
        ))
        .id();
    layers.set_scene_entity(HudLayerId::Modal, modal_scene);

    let modal_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: HUD_MODAL_CAMERA_ORDER,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            VelloView,
            bevy::camera::visibility::RenderLayers::layer(HUD_MODAL_RENDER_LAYER),
            HudModalCameraMarker,
            HudLayerCameraMarker {
                id: HudLayerId::Modal,
            },
        ))
        .id();
    layers.set_camera_entity(HudLayerId::Modal, modal_camera);

    setup_hud_offscreen_compositor(
        &mut commands,
        &mut compositor,
        &mut meshes,
        &mut composite_materials,
    );
    redraws.write(RequestRedraw);
}

/// Reapplies structural layout constraints that the user is not allowed to move.
///
/// The info bar is pinned to the top edge, the agent list is pinned below it, and floating modules
/// are clamped so they cannot overlap the reserved header band.
pub(crate) fn sync_structural_hud_layout(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut layout_state: ResMut<HudLayoutState>,
) {
    let info_bar_rect = docked_info_bar_rect(&primary_window);
    if let Some(info_bar) = layout_state.get_mut(HudWidgetKey::InfoBar) {
        info_bar.shell.set_canonical_rect(info_bar_rect, true);
    }

    let reserved_top = layout_state
        .module_layout(HudWidgetKey::InfoBar)
        .filter(|layout| layout.enabled)
        .map(|layout| layout.rect.h)
        .unwrap_or(0.0);
    let agent_list_rect = docked_agent_list_rect_with_top_inset(&primary_window, reserved_top);
    if let Some(agent_list) = layout_state.get_mut(HudWidgetKey::AgentList) {
        agent_list.shell.set_canonical_rect(agent_list_rect, true);
    }

    let module_ids = layout_state.iter_z_order().collect::<Vec<_>>();
    for module_id in module_ids {
        if matches!(module_id, HudWidgetKey::InfoBar | HudWidgetKey::AgentList) {
            continue;
        }
        let Some(module) = layout_state.get_mut(module_id) else {
            continue;
        };
        let mut canonical_rect = module.shell.canonical_layout().rect;
        if canonical_rect.y < reserved_top {
            canonical_rect.y = reserved_top;
            module.shell.set_canonical_rect(canonical_rect, false);
        }
        if module.shell.presentation_layout().rect.y < reserved_top {
            let mut presented_rect = module.shell.presentation_layout().rect;
            presented_rect.y = reserved_top;
            module.shell.current_rect = presented_rect;
        }
    }
}

/// Returns whether HUD animation/dragging requires another redraw.
///
/// Static retained HUD scenes do not need continuous redraw; dragging and animation are the two
/// conditions that keep frames flowing.
pub(crate) fn hud_needs_redraw(layout_state: &HudLayoutState) -> bool {
    layout_state.drag.is_some() || layout_state.is_animating()
}

#[cfg(test)]
mod tests;
