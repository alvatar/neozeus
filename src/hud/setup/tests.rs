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

    let (bloom_camera_order, bloom_layers_ok) = {
        let mut bloom_camera_query = world.query_filtered::<
            (&Camera, &RenderLayers),
            With<crate::hud::HudCompositeBloomCameraMarker>,
        >();
        let (bloom_camera, bloom_layers) = bloom_camera_query
            .single(&world)
            .expect("composite bloom camera should exist");
        (
            bloom_camera.order,
            bloom_layers.intersects(&RenderLayers::layer(
                crate::hud::HUD_COMPOSITE_BLOOM_RENDER_LAYER,
            )),
        )
    };
    assert_eq!(
        bloom_camera_order,
        crate::hud::HUD_COMPOSITE_BLOOM_CAMERA_ORDER
    );
    assert!(bloom_layers_ok);

    let (modal_camera_order, modal_layers_ok) = {
        let mut modal_camera_query = world
            .query_filtered::<(&Camera, &RenderLayers), With<crate::hud::HudModalCameraMarker>>();
        let (modal_camera, modal_layers) = modal_camera_query
            .single(&world)
            .expect("modal camera should exist");
        (
            modal_camera.order,
            modal_layers.intersects(&RenderLayers::layer(HUD_MODAL_RENDER_LAYER)),
        )
    };
    assert_eq!(modal_camera_order, HUD_MODAL_CAMERA_ORDER);
    assert!(modal_layers_ok);
    assert!(bloom_camera_order < modal_camera_order);
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
