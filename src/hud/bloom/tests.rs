use super::*;
use crate::{
    hud::AgentListView,
    terminals::TerminalManager,
    tests::{
        insert_terminal_manager_resources, insert_test_hud_state, snapshot_test_hud_state,
        test_bridge,
    },
};
use bevy::{
    camera::{visibility::RenderLayers, RenderTarget},
    ecs::system::RunSystemOnce,
    prelude::{
        default, Assets, Image, Mesh, Sprite, Transform, Vec2, Vec4, Visibility, Window, With,
        World,
    },
    render::render_resource::TextureFormat,
    sprite_render::{AlphaMode2d, Material2d},
    window::{PrimaryWindow, WindowResolution},
};

/// Locks down the exact reference RGB constants used by the agent-list border and bloom styling.
///
/// These values matter because the visual verification scripts compare rendered output against known
/// color targets; accidental changes here would silently change the whole visual signature.
#[test]
fn agent_list_reference_colors_match_requested_values() {
    assert_eq!(
        (
            crate::hud::modules::AGENT_LIST_BORDER_ORANGE_R,
            crate::hud::modules::AGENT_LIST_BORDER_ORANGE_G,
            crate::hud::modules::AGENT_LIST_BORDER_ORANGE_B,
        ),
        (225, 129, 10)
    );
    assert_eq!(
        (
            AGENT_LIST_BLOOM_RED_R,
            AGENT_LIST_BLOOM_RED_G,
            AGENT_LIST_BLOOM_RED_B,
        ),
        (143, 37, 15)
    );
}

/// Verifies the permissive parser for the bloom-intensity override.
///
/// Valid non-negative finite numbers should be accepted, while empty, negative, or malformed values
/// should fall back to the module default intensity.
#[test]
fn parses_agent_bloom_intensity_override() {
    assert_eq!(resolve_agent_list_bloom_intensity(None), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("")), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("2.0")), 2.0);
    assert_eq!(resolve_agent_list_bloom_intensity(Some(" 0.0 ")), 0.0);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("-1")), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("abc")), 0.10);
}

/// Verifies the boolean parser for bloom debug previews.
///
/// The test covers the accepted truthy spellings and confirms that empty or explicit falsy values do
/// not enable the preview overlays.
#[test]
fn parses_agent_bloom_debug_previews_override() {
    assert!(!resolve_agent_list_bloom_debug_previews(None));
    assert!(!resolve_agent_list_bloom_debug_previews(Some("")));
    assert!(resolve_agent_list_bloom_debug_previews(Some("1")));
    assert!(resolve_agent_list_bloom_debug_previews(Some(" true ")));
    assert!(resolve_agent_list_bloom_debug_previews(Some("on")));
    assert!(!resolve_agent_list_bloom_debug_previews(Some("0")));
    assert!(!resolve_agent_list_bloom_debug_previews(Some("false")));
}

/// Verifies that the bloom blur material renders its offscreen passes opaquely.
///
/// The blur passes already operate on isolated float targets, so alpha blending would only pollute
/// the intermediate images. This test locks the material policy down.
#[test]
fn bloom_blur_material_writes_offscreen_passes_opaquely() {
    let material = AgentListBloomBlurMaterial {
        image: default(),
        uniform: AgentListBloomBlurUniform {
            texel_step_gain: Vec4::ZERO,
        },
    };
    assert_eq!(Material2d::alpha_mode(&material), AlphaMode2d::Opaque);
}

/// Verifies the initial bloom setup graph created at startup.
///
/// The setup system should create the offscreen source camera, its float render target, and the
/// hidden composite sprite that will later bring the bloom result back into the main HUD composition.
#[test]
fn setup_hud_widget_bloom_spawns_camera_and_composite_sprite() {
    let mut world = World::default();
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();

    assert_eq!(
        world
            .query::<&AgentListBloomCameraMarker>()
            .iter(&world)
            .count(),
        1
    );

    let layer = agent_list_bloom_layer();
    let mut camera_query =
        world.query_filtered::<(&RenderLayers, &RenderTarget), With<AgentListBloomCameraMarker>>();
    let (layers, target) = camera_query.single(&world).unwrap();
    assert!(layers.intersects(&RenderLayers::layer(layer)));

    let RenderTarget::Image(handle) = target else {
        panic!("bloom camera must render to image target");
    };
    let bloom_image_handle = handle.handle.clone();
    let image_format = {
        let images = world.resource::<Assets<Image>>();
        images
            .get(bloom_image_handle.id())
            .expect("bloom image exists")
            .texture_descriptor
            .format
    };
    assert_eq!(image_format, TextureFormat::Rgba16Float);

    let mut composite_query = world.query::<(
        &Transform,
        &Visibility,
        &Sprite,
        &AgentListBloomCompositeMarker,
    )>();
    let (transform, visibility, sprite, _) = composite_query.single(&world).unwrap();
    assert_eq!(transform.translation.z, agent_list_bloom_z());
    assert_eq!(visibility, &Visibility::Hidden);
    let composite_image_format = {
        let images = world.resource::<Assets<Image>>();
        images
            .get(sprite.image.id())
            .expect("bloom composite image exists")
            .texture_descriptor
            .format
    };
    assert_eq!(composite_image_format, TextureFormat::Rgba16Float);
}

/// Verifies that bloom targets are sized from logical window dimensions, not physical pixels.
///
/// This matters when a scale-factor override is active: the bloom pipeline is intentionally tied to
/// logical HUD layout, so its render targets should track the logical size.
#[test]
fn setup_hud_widget_bloom_uses_logical_window_size_for_targets() {
    let mut world = World::default();
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: WindowResolution::new(1400, 900).with_scale_factor_override(2.0),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();

    let target_handles = {
        let mut target_query = world.query::<&RenderTarget>();
        target_query
            .iter(&world)
            .filter_map(|target| match target {
                RenderTarget::Image(handle) => Some(handle.handle.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let images = world.resource::<Assets<Image>>();
    let target_images = target_handles
        .iter()
        .filter_map(|handle| images.get(handle.id()))
        .collect::<Vec<_>>();
    let target_sizes = target_images
        .iter()
        .map(|image| image.texture_descriptor.size)
        .collect::<Vec<_>>();
    assert!(target_sizes.iter().all(|size| size.width == 700));
    assert!(target_sizes.iter().all(|size| size.height == 450));
    assert!(target_images
        .iter()
        .all(|image| image.texture_descriptor.format == TextureFormat::Rgba16Float));
}

/// Verifies that bloom sync creates the expected set of source border sprites for the active agent
/// row.
///
/// The test checks both the count and the per-segment breakdown so the bloom source generation keeps
/// producing the four border strips for both the main cell and the marker cell.
#[test]
fn sync_hud_widget_bloom_spawns_agent_list_source_sprites() {
    let mut world = World::default();
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();
    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();
    world.run_system_once(sync_hud_widget_bloom).unwrap();

    let source_sprites = world
        .query::<(&AgentListBloomSourceSprite, &Sprite)>()
        .iter(&world)
        .map(|(marker, sprite)| (*marker, sprite.clone()))
        .collect::<Vec<_>>();
    assert_eq!(source_sprites.len(), 8);
    assert_eq!(
        source_sprites
            .iter()
            .filter(|(sprite, _)| sprite.kind == AgentListBloomSourceKind::Main)
            .count(),
        4
    );
    assert_eq!(
        source_sprites
            .iter()
            .filter(|(sprite, _)| sprite.kind == AgentListBloomSourceKind::Marker)
            .count(),
        4
    );
    for segment in [
        AgentListBloomSourceSegment::Top,
        AgentListBloomSourceSegment::Right,
        AgentListBloomSourceSegment::Bottom,
        AgentListBloomSourceSegment::Left,
    ] {
        assert!(source_sprites.iter().any(|(sprite, _)| {
            sprite.kind == AgentListBloomSourceKind::Main && sprite.segment == segment
        }));
        assert!(source_sprites.iter().any(|(sprite, _)| {
            sprite.kind == AgentListBloomSourceKind::Marker && sprite.segment == segment
        }));
    }

    let expected_sizes = {
        let hud_state = snapshot_test_hud_state(&world);
        let agent_list_view = world.resource::<AgentListView>();
        let module = hud_state.get(HudWidgetKey::AgentList).unwrap();
        let crate::hud::HudModuleModel::AgentList(state) = &module.model else {
            panic!("agent list module model missing")
        };
        let row = crate::hud::agent_rows(
            module.shell.current_rect,
            state.scroll_offset,
            state.hovered_terminal,
            agent_list_view,
        )
        .into_iter()
        .next()
        .expect("agent row exists");
        let main = crate::hud::agent_row_rect(row.rect, crate::hud::AgentListRowSection::Main);
        let marker = crate::hud::agent_row_rect(row.rect, crate::hud::AgentListRowSection::Marker);
        let target_size = {
            let mut camera_query =
                world.query_filtered::<&RenderTarget, With<AgentListBloomCameraMarker>>();
            let RenderTarget::Image(handle) = camera_query.single(&world).unwrap() else {
                panic!("bloom target missing")
            };
            let images = world.resource::<Assets<Image>>();
            images
                .get(handle.handle.id())
                .expect("bloom target image exists")
                .texture_descriptor
                .size
        };
        let scale_x = target_size.width as f32 / 1400.0;
        let scale_y = target_size.height as f32 / 900.0;
        [
            Vec2::new(main.w * scale_x, 3.0 * scale_y),
            Vec2::new(3.0 * scale_x, main.h * scale_y),
            Vec2::new(marker.w * scale_x, 2.5 * scale_y),
            Vec2::new(2.5 * scale_x, marker.h * scale_y),
        ]
    };
    let actual_sizes = source_sprites
        .iter()
        .map(|(_, sprite)| sprite.custom_size.expect("source size exists"))
        .collect::<Vec<_>>();
    assert!(actual_sizes
        .iter()
        .all(|size| expected_sizes.contains(size)));

    let mut composite_query = world.query::<(
        &Visibility,
        &Transform,
        &Sprite,
        &AgentListBloomCompositeMarker,
    )>();
    let (visibility, transform, sprite, _) = composite_query.single(&world).unwrap();
    assert_eq!(visibility, &Visibility::Visible);
    assert_eq!(transform.translation.z, agent_list_bloom_z());
    assert_eq!(sprite.custom_size, Some(Vec2::new(1400.0, 900.0)));
}

/// Verifies that bloom is suppressed while a modal HUD surface is visible.
///
/// The bloom effect should not leak behind message/task dialogs, so the sync system must remove any
/// source sprites and hide the composite sprite when a modal is active.
#[test]
fn sync_hud_widget_bloom_hides_sources_and_composite_while_modal_is_visible() {
    let mut world = World::default();
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = crate::hud::HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    hud_state.message_box.visible = true;
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();
    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();
    world.run_system_once(sync_hud_widget_bloom).unwrap();

    assert_eq!(
        world
            .query::<&AgentListBloomSourceSprite>()
            .iter(&world)
            .count(),
        0
    );
    let mut composite_query = world.query::<(&Visibility, &AgentListBloomCompositeMarker)>();
    let (visibility, _) = composite_query.single(&world).unwrap();
    assert_eq!(visibility, &Visibility::Hidden);
}

/// Verifies that bloom sources are generated only for the active agent row.
///
/// Even if multiple rows exist, the bloom effect should track the currently focused/active terminal so
/// the visual emphasis stays singular.
#[test]
fn sync_hud_widget_bloom_only_uses_active_agent_source() {
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);

    let mut hud_state = crate::hud::HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world
        .run_system_once(crate::hud::sync_hud_view_models)
        .unwrap();
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();
    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();
    world.run_system_once(sync_hud_widget_bloom).unwrap();

    let source_sprites = world
        .query::<&AgentListBloomSourceSprite>()
        .iter(&world)
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(source_sprites.len(), 8);
    assert!(source_sprites
        .iter()
        .all(|sprite| sprite.terminal_id == id_two));
    assert!(source_sprites
        .iter()
        .all(|sprite| sprite.terminal_id != id_one));
}
