use crate::app::AppSessionState;

use super::{
    compositor::{setup_hud_offscreen_compositor, HudOffscreenCompositor},
    persistence::{load_persisted_hud_modules_from, resolve_hud_layout_path, HudPersistenceState},
    render::{HudModalVectorSceneMarker, HudOverlayVectorSceneMarker, HudVectorSceneMarker},
    render_layer::{HudLayerId, HudLayerRegistry, HudLayerSceneMarker},
    state::{
        default_hud_module_instance, docked_agent_list_rect_with_top_inset, docked_info_bar_rect,
        AgentListUiState, ConversationListUiState, HudInputCaptureState, HudLayoutState,
        InfoBarUiState, ThreadPaneUiState,
    },
    widgets::{HudWidgetKey, HUD_WIDGET_DEFINITIONS},
};
use bevy::{
    camera::visibility::NoFrustumCulling,
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use bevy_vello::prelude::VelloScene2d;

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
            HudOverlayVectorSceneMarker,
            HudLayerSceneMarker {
                id: HudLayerId::Overlay,
            },
        ))
        .id();
    layers.set_scene_entity(HudLayerId::Overlay, overlay_scene);

    let modal_scene = commands
        .spawn((
            VelloScene2d::default(),
            Transform::from_xyz(0.0, 0.0, 60.0),
            NoFrustumCulling,
            HudModalVectorSceneMarker,
            HudLayerSceneMarker {
                id: HudLayerId::Modal,
            },
        ))
        .id();
    layers.set_scene_entity(HudLayerId::Modal, modal_scene);

    setup_hud_offscreen_compositor(
        &mut commands,
        &mut layers,
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
mod tests {
    use super::*;
    use super::{hud_needs_redraw, setup_hud, sync_structural_hud_layout};
    use crate::{
        hud::{
            HudCompositeLayerId, HudDragState, HudLayerId, HudLayerRegistry, HudOffscreenCompositor,
            HudPersistenceState, HudState, HudWidgetKey, HUD_COMPOSITE_RENDER_LAYER,
        },
        tests::{insert_default_hud_resources, insert_test_hud_state, snapshot_test_hud_state},
    };
    use bevy::{
        camera::visibility::RenderLayers,
        ecs::system::RunSystemOnce,
        window::{PrimaryWindow, RequestRedraw},
    };
    use bevy_vello::render::VelloCanvasMaterial;

    /// Verifies that HUD setup spawns the expected scene/compositor entities and immediately requests a
    /// redraw.
    #[test]
    fn setup_hud_requests_initial_redraw() {
        let mut world = World::default();
        insert_default_hud_resources(&mut world);
        world.insert_resource(HudPersistenceState::default());
        world.insert_resource(HudOffscreenCompositor::default());
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<VelloCanvasMaterial>::default());
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(setup_hud).unwrap();

        let redraws = world.resource::<Messages<RequestRedraw>>();
        assert_eq!(redraws.len(), 1);
        assert_eq!(
            world
                .query::<&crate::hud::HudVectorSceneMarker>()
                .iter(&world)
                .count(),
            1
        );
        assert_eq!(
            world
                .query::<&crate::hud::HudCompositeLayerMarker>()
                .iter(&world)
                .count(),
            3
        );
        assert_eq!(
            world
                .query::<&crate::hud::HudOverlayVectorSceneMarker>()
                .iter(&world)
                .count(),
            1
        );
        assert_eq!(
            world
                .query::<&crate::hud::HudModalVectorSceneMarker>()
                .iter(&world)
                .count(),
            1
        );
        let composite_cameras = world
            .query::<(
                &Camera,
                &RenderLayers,
                &crate::hud::HudCompositeCameraMarker,
            )>()
            .iter(&world)
            .map(|(camera, layers, marker)| (marker.id, camera.order, layers.clone()))
            .collect::<Vec<_>>();
        assert_eq!(composite_cameras.len(), 3);
        assert!(composite_cameras.iter().any(|(id, order, layers)| {
            *id == HudCompositeLayerId::Main
                && *order == 50
                && layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER))
        }));
        assert!(composite_cameras.iter().any(|(id, order, layers)| {
            *id == HudCompositeLayerId::Overlay
                && *order == HudLayerId::Overlay.order()
                && layers.intersects(&RenderLayers::layer(HudLayerId::Overlay.composite_render_layer()))
        }));
        assert!(composite_cameras.iter().any(|(id, order, layers)| {
            *id == HudCompositeLayerId::Modal
                && *order == HudLayerId::Modal.order()
                && layers.intersects(&RenderLayers::layer(HudLayerId::Modal.composite_render_layer()))
        }));
    }

    #[test]
    fn setup_hud_initializes_explicit_layer_registry_with_overlay() {
        let mut world = World::default();
        insert_default_hud_resources(&mut world);
        world.insert_resource(HudPersistenceState::default());
        world.insert_resource(HudOffscreenCompositor::default());
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<VelloCanvasMaterial>::default());
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(setup_hud).unwrap();

        let registry = world.resource::<HudLayerRegistry>();
        assert_eq!(
            registry.ordered_ids(),
            &[HudLayerId::Main, HudLayerId::Overlay, HudLayerId::Modal]
        );
        assert!(registry
            .layer(HudLayerId::Overlay)
            .and_then(|layer| layer.scene_entity)
            .is_some());
        assert!(registry
            .layer(HudLayerId::Overlay)
            .and_then(|layer| layer.camera_entity)
            .is_some());
    }

    #[test]
    fn setup_hud_marks_layer_entities_with_explicit_ids() {
        let mut world = World::default();
        insert_default_hud_resources(&mut world);
        world.insert_resource(HudPersistenceState::default());
        world.insert_resource(HudOffscreenCompositor::default());
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<VelloCanvasMaterial>::default());
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(setup_hud).unwrap();

        let registry = world.resource::<HudLayerRegistry>();
        for id in registry.ordered_ids() {
            let scene_entity = registry
                .layer(*id)
                .and_then(|layer| layer.scene_entity)
                .expect("layer scene should exist");
            let marker = world
                .get::<crate::hud::HudLayerSceneMarker>(scene_entity)
                .expect("scene should have explicit layer marker");
            assert_eq!(marker.id, *id);
        }
    }

    #[test]
    fn setup_hud_orders_overlay_between_compositor_and_modal_layers() {
        let mut world = World::default();
        insert_default_hud_resources(&mut world);
        world.insert_resource(HudPersistenceState::default());
        world.insert_resource(HudOffscreenCompositor::default());
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<VelloCanvasMaterial>::default());
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(setup_hud).unwrap();

        let composite_order = world
            .query::<(&Camera, &crate::hud::HudCompositeCameraMarker)>()
            .iter(&world)
            .find(|(_, marker)| marker.id == HudCompositeLayerId::Main)
            .expect("main composite camera exists")
            .0
            .order;
        let overlay_order = world
            .query::<(&Camera, &crate::hud::HudCompositeCameraMarker)>()
            .iter(&world)
            .find(|(_, marker)| marker.id == HudCompositeLayerId::Overlay)
            .expect("overlay composite camera exists")
            .0
            .order;
        let modal_order = world
            .query::<(&Camera, &crate::hud::HudCompositeCameraMarker)>()
            .iter(&world)
            .find(|(_, marker)| marker.id == HudCompositeLayerId::Modal)
            .expect("modal composite camera exists")
            .0
            .order;

        assert!(composite_order < overlay_order);
        assert!(overlay_order < modal_order);
    }

    /// Verifies that structural HUD sync pins the info bar to the top edge and docks the agent list
    /// directly below it.
    #[test]
    fn sync_structural_hud_layout_pins_info_bar_and_docks_agent_list_below() {
        let mut world = World::default();
        let mut hud_state = HudState::default();
        hud_state.insert(
            HudWidgetKey::InfoBar,
            crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
        );
        hud_state.insert(
            HudWidgetKey::AgentList,
            crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
        );
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..default()
            },
            PrimaryWindow,
        ));

        world.run_system_once(sync_structural_hud_layout).unwrap();

        let (expected_info_bar_rect, expected_agent_rect) = {
            let mut query = world.query_filtered::<&Window, With<PrimaryWindow>>();
            let window = query.single(&world).expect("window should exist");
            (
                crate::hud::docked_info_bar_rect(window),
                crate::hud::docked_agent_list_rect_with_top_inset(
                    window,
                    crate::hud::docked_info_bar_rect(window).h,
                ),
            )
        };
        let hud_state = snapshot_test_hud_state(&world);
        let info_bar = hud_state
            .get(HudWidgetKey::InfoBar)
            .expect("info bar should exist");
        let agent_list = hud_state
            .get(HudWidgetKey::AgentList)
            .expect("agent list should exist");
        assert_eq!(info_bar.shell.current_rect, expected_info_bar_rect);
        assert_eq!(agent_list.shell.current_rect, expected_agent_rect);
    }

    /// Verifies that the HUD redraw predicate turns on for either drag state or in-flight shell
    /// animation.
    #[test]
    fn hud_needs_redraw_when_drag_or_animation_is_active() {
        let mut state = HudState::default();
        state.insert(
            HudWidgetKey::AgentList,
            crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
        );
        assert!(!hud_needs_redraw(&state.layout_state()));
        state.drag = Some(HudDragState {
            module_id: HudWidgetKey::AgentList,
            grab_offset: Vec2::ZERO,
        });
        assert!(hud_needs_redraw(&state.layout_state()));
        state.drag = None;
        let module = state
            .get_mut(HudWidgetKey::AgentList)
            .expect("agent list should exist");
        module.shell.current_rect.x = 0.0;
        module.shell.target_rect.x = 10.0;
        assert!(hud_needs_redraw(&state.layout_state()));
    }

    /// Verifies that disabling a HUD module does not suppress redraw while its fade-out animation is
    /// still active.
    #[test]
    fn disabled_hud_module_still_requests_redraw_while_fading_out() {
        let mut state = HudState::default();
        state.insert(
            HudWidgetKey::AgentList,
            crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
        );

        state.set_module_enabled(HudWidgetKey::AgentList, false);

        let module = state
            .get(HudWidgetKey::AgentList)
            .expect("agent list should exist");
        assert!(!module.shell.enabled);
        assert!(module.shell.is_animating());
        assert!(hud_needs_redraw(&state.layout_state()));
    }

    /// Verifies that disabling the info bar removes the top structural inset from the docked agent
    /// list.
    #[test]
    fn sync_structural_hud_layout_uses_canonical_info_bar_rect_for_agent_list_inset() {
        let mut world = World::default();
        let mut hud_state = HudState::default();
        hud_state.insert(
            HudWidgetKey::InfoBar,
            crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
        );
        hud_state.insert(
            HudWidgetKey::AgentList,
            crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
        );
        let info_bar = hud_state
            .get_mut(HudWidgetKey::InfoBar)
            .expect("info bar should exist");
        info_bar.shell.target_rect.h = 60.0;
        info_bar.shell.current_rect.h = 12.0;
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..default()
            },
            PrimaryWindow,
        ));

        world.run_system_once(sync_structural_hud_layout).unwrap();

        let hud_state = snapshot_test_hud_state(&world);
        let agent_list = hud_state
            .get(HudWidgetKey::AgentList)
            .expect("agent list should exist");
        assert_eq!(agent_list.shell.target_rect.y, 60.0);
        assert_eq!(agent_list.shell.current_rect.y, 60.0);
    }

    #[test]
    fn sync_structural_hud_layout_removes_header_inset_when_info_bar_is_disabled() {
        let mut world = World::default();
        let mut hud_state = HudState::default();
        hud_state.insert(
            HudWidgetKey::InfoBar,
            crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
        );
        hud_state.insert(
            HudWidgetKey::AgentList,
            crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
        );
        hud_state.set_module_enabled(HudWidgetKey::InfoBar, false);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..default()
            },
            PrimaryWindow,
        ));

        world.run_system_once(sync_structural_hud_layout).unwrap();

        let hud_state = snapshot_test_hud_state(&world);
        let agent_list = hud_state
            .get(HudWidgetKey::AgentList)
            .expect("agent list should exist");
        assert_eq!(agent_list.shell.current_rect.y, 0.0);
        assert_eq!(agent_list.shell.current_rect.h, 900.0);
    }
}
