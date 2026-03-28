use super::{hud_needs_redraw, setup_hud, sync_structural_hud_layout};
use crate::{
    hud::{
        HudDragState, HudOffscreenCompositor, HudPersistenceState, HudState, HudWidgetKey,
        HUD_MODAL_CAMERA_ORDER, HUD_MODAL_RENDER_LAYER,
    },
    tests::{insert_default_hud_resources, insert_test_hud_state, snapshot_test_hud_state},
};
use bevy::{
    camera::visibility::RenderLayers,
    ecs::system::RunSystemOnce,
    prelude::*,
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
        1
    );
    assert_eq!(
        world
            .query::<&crate::hud::HudModalVectorSceneMarker>()
            .iter(&world)
            .count(),
        1
    );
    let mut camera_query = world
        .query_filtered::<(&Camera, &RenderLayers), With<crate::hud::HudCompositeCameraMarker>>();
    let (camera, layers) = camera_query
        .single(&world)
        .expect("composite camera should exist");
    assert_eq!(camera.order, 50);
    assert!(layers.intersects(&RenderLayers::layer(crate::hud::HUD_COMPOSITE_RENDER_LAYER)));

    let mut modal_camera_query =
        world.query_filtered::<(&Camera, &RenderLayers), With<crate::hud::HudModalCameraMarker>>();
    let (modal_camera, modal_layers) = modal_camera_query
        .single(&world)
        .expect("modal camera should exist");
    assert_eq!(modal_camera.order, HUD_MODAL_CAMERA_ORDER);
    assert!(modal_layers.intersects(&RenderLayers::layer(HUD_MODAL_RENDER_LAYER)));
}

/// Verifies that structural HUD sync forcibly docks the agent list to the left edge at full window
/// height.
#[test]
fn sync_structural_hud_layout_docks_agent_list_to_full_height_left_column() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
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

    let expected_rect = {
        let mut query = world.query_filtered::<&Window, With<PrimaryWindow>>();
        crate::hud::docked_agent_list_rect(query.single(&world).expect("window should exist"))
    };
    let hud_state = snapshot_test_hud_state(&world);
    let module = hud_state
        .get(HudWidgetKey::AgentList)
        .expect("agent list should exist");
    assert_eq!(module.shell.current_rect, expected_rect);
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
