use crate::app::AppSessionState;

use super::{
    compositor::{setup_hud_offscreen_compositor, HudOffscreenCompositor},
    persistence::{load_persisted_hud_state_from, resolve_hud_layout_path, HudPersistenceState},
    render::{
        HudModalCameraMarker, HudModalVectorSceneMarker, HudVectorSceneMarker,
        HUD_MODAL_CAMERA_ORDER, HUD_MODAL_RENDER_LAYER,
    },
    state::{
        default_hud_module_instance, docked_agent_list_rect, AgentListUiState,
        ConversationListUiState, DebugToolbarUiState, HudInputCaptureState, HudLayoutState,
        ThreadPaneUiState,
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
    mut _debug_toolbar_state: ResMut<DebugToolbarUiState>,
    mut _thread_pane_state: ResMut<ThreadPaneUiState>,
    mut app_session: ResMut<AppSessionState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut persistence_state: ResMut<HudPersistenceState>,
    mut compositor: ResMut<HudOffscreenCompositor>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut composite_materials: ResMut<Assets<bevy_vello::render::VelloCanvasMaterial>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    persistence_state.path = resolve_hud_layout_path();
    let persisted = persistence_state
        .path
        .as_ref()
        .map(load_persisted_hud_state_from)
        .unwrap_or_default();
    layout_state.modules.clear();
    layout_state.z_order.clear();
    layout_state.drag = None;
    layout_state.dirty_layout = false;
    app_session.composer = crate::ui::ComposerState::default();
    *agent_list_state = AgentListUiState::default();
    *conversation_list_state = ConversationListUiState::default();
    *_debug_toolbar_state = DebugToolbarUiState;
    *_thread_pane_state = ThreadPaneUiState;
    input_capture.direct_input_terminal = None;
    for definition in HUD_WIDGET_DEFINITIONS.iter() {
        let mut module = default_hud_module_instance(definition);
        if let Some(saved) = persisted.modules.get(&definition.key) {
            module.shell.enabled = saved.enabled;
            module.shell.target_rect = saved.rect;
            module.shell.current_rect = saved.rect;
            module.shell.target_alpha = if saved.enabled { 1.0 } else { 0.0 };
            module.shell.current_alpha = module.shell.target_alpha;
        }
        layout_state.insert(definition.key, module);
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

/// Reapplies structural layout constraints that the user is not allowed to move.
///
/// Today this means forcing the agent list to stay docked to the window edge regardless of persisted
/// or animated state.
pub(crate) fn sync_structural_hud_layout(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut layout_state: ResMut<HudLayoutState>,
) {
    let rect = docked_agent_list_rect(&primary_window);
    let Some(agent_list) = layout_state.get_mut(HudWidgetKey::AgentList) else {
        return;
    };
    agent_list.shell.target_rect = rect;
    agent_list.shell.current_rect = rect;
}

/// Returns whether HUD animation/dragging requires another redraw.
///
/// Static retained HUD scenes do not need continuous redraw; dragging and animation are the two
/// conditions that keep frames flowing.
pub(crate) fn hud_needs_redraw(layout_state: &HudLayoutState) -> bool {
    layout_state.drag.is_some() || layout_state.is_animating()
}
