use super::{
    setup_hud_offscreen_compositor, sync_hud_offscreen_compositor, HudOffscreenCompositor,
};
use crate::hud::{
    HudCompositeBloomCameraMarker, HudCompositeCameraMarker, HudCompositeLayerId,
    HudCompositeLayerMarker, HudCompositeModalCameraMarker, HudModalCameraMarker,
    HudModalVectorSceneMarker, HUD_COMPOSITE_BLOOM_CAMERA_ORDER, HUD_COMPOSITE_BLOOM_RENDER_LAYER,
    HUD_COMPOSITE_FOREGROUND_Z, HUD_COMPOSITE_MODAL_CAMERA_ORDER, HUD_COMPOSITE_MODAL_RENDER_LAYER,
    HUD_COMPOSITE_RENDER_LAYER,
};
use bevy::{
    camera::{
        visibility::{NoFrustumCulling, RenderLayers},
        RenderTarget,
    },
    ecs::system::RunSystemOnce,
    mesh::VertexAttributeValues,
    prelude::*,
    sprite_render::MeshMaterial2d,
    window::PrimaryWindow,
};
use bevy_vello::render::VelloCanvasMaterial;

#[test]
fn setup_hud_offscreen_compositor_spawns_modal_composite_camera_above_bloom() {
    let mut world = World::default();
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(
            |mut commands: Commands,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    let mut main_camera_query =
        world.query_filtered::<(&Camera, &RenderLayers), With<HudCompositeCameraMarker>>();
    let (main_camera, main_layers) = main_camera_query
        .single(&world)
        .expect("main compositor camera should exist");
    assert_eq!(main_camera.order, 50);
    assert!(main_layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER)));

    let (bloom_camera_order, bloom_layers_ok) = {
        let mut bloom_camera_query =
            world.query_filtered::<(&Camera, &RenderLayers), With<HudCompositeBloomCameraMarker>>();
        let (bloom_camera, bloom_layers) = bloom_camera_query
            .single(&world)
            .expect("bloom compositor camera should exist");
        (
            bloom_camera.order,
            bloom_layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_BLOOM_RENDER_LAYER)),
        )
    };
    assert_eq!(bloom_camera_order, HUD_COMPOSITE_BLOOM_CAMERA_ORDER);
    assert!(bloom_layers_ok);

    let (modal_camera_order, modal_layers_ok) = {
        let mut modal_camera_query =
            world.query_filtered::<(&Camera, &RenderLayers), With<HudCompositeModalCameraMarker>>();
        let (modal_camera, modal_layers) = modal_camera_query
            .single(&world)
            .expect("modal compositor camera should exist");
        (
            modal_camera.order,
            modal_layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_MODAL_RENDER_LAYER)),
        )
    };
    assert_eq!(modal_camera_order, HUD_COMPOSITE_MODAL_CAMERA_ORDER);
    assert!(modal_layers_ok);
    assert!(bloom_camera_order < modal_camera_order);
}

/// Verifies that compositor sync hides the upstream Vello canvas and routes its texture through the
/// compositor quad instead.
#[test]
fn sync_hud_offscreen_compositor_hides_vello_canvas_and_binds_texture() {
    let mut world = World::default();
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.insert_resource(Assets::<Mesh>::default());
    let texture = world.resource_mut::<Assets<Image>>().add(Image::default());
    {
        let mut images = world.resource_mut::<Assets<Image>>();
        let image = images.get_mut(&texture).expect("texture should exist");
        image.resize(bevy::render::render_resource::Extent3d {
            width: 1400,
            height: 900,
            depth_or_array_layers: 1,
        });
    }
    let material = world
        .resource_mut::<Assets<VelloCanvasMaterial>>()
        .add(VelloCanvasMaterial {
            texture: texture.clone(),
        });
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));
    let source_canvas = world
        .spawn((
            MeshMaterial2d::<VelloCanvasMaterial>(material),
            Visibility::Visible,
        ))
        .id();
    world
        .run_system_once(
            |mut commands: Commands,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    world
        .run_system_once(sync_hud_offscreen_compositor)
        .unwrap();

    assert_eq!(
        world.get::<Visibility>(source_canvas),
        Some(&Visibility::Hidden)
    );

    let mut camera_query =
        world.query_filtered::<(&Camera, &RenderLayers), With<HudCompositeCameraMarker>>();
    let (camera, layers) = camera_query.single(&world).expect("camera should exist");
    assert_eq!(camera.order, 50);
    assert!(layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER)));

    let mut quad_query = world.query::<(
        &HudCompositeLayerMarker,
        &MeshMaterial2d<VelloCanvasMaterial>,
        &Transform,
        &Visibility,
        &RenderLayers,
        &NoFrustumCulling,
    )>();
    let (marker, composite_material, transform, visibility, quad_layers, _) =
        quad_query.single(&world).expect("quad should exist");
    assert_eq!(marker.id, HudCompositeLayerId::MainHud);
    let composite_texture = {
        let materials = world.resource::<Assets<VelloCanvasMaterial>>();
        materials
            .get(composite_material.id())
            .expect("composite material exists")
            .texture
            .clone()
    };
    assert_eq!(composite_texture, texture);
    assert_eq!(transform.scale, Vec3::ONE);
    assert_eq!(transform.translation, Vec3::ZERO);
    assert_eq!(visibility, &Visibility::Visible);
    assert!(quad_layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER)));
}

/// Verifies that compositor sync retargets the modal Vello camera into an offscreen image and
/// exposes that image through the topmost modal composite sprite.
#[test]
fn sync_hud_offscreen_compositor_routes_modal_vello_through_top_composite_sprite() {
    let mut world = World::default();
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.insert_resource(Assets::<Mesh>::default());
    let texture = world.resource_mut::<Assets<Image>>().add(Image::default());
    {
        let mut images = world.resource_mut::<Assets<Image>>();
        let image = images.get_mut(&texture).expect("texture should exist");
        image.resize(bevy::render::render_resource::Extent3d {
            width: 1400,
            height: 900,
            depth_or_array_layers: 1,
        });
    }
    let material = world
        .resource_mut::<Assets<VelloCanvasMaterial>>()
        .add(VelloCanvasMaterial {
            texture: texture.clone(),
        });
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));
    world.spawn((
        MeshMaterial2d::<VelloCanvasMaterial>(material),
        Visibility::Visible,
        HudModalVectorSceneMarker,
    ));
    let modal_vello_camera = world.spawn((HudModalCameraMarker,)).id();
    world
        .run_system_once(
            |mut commands: Commands,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    world
        .run_system_once(sync_hud_offscreen_compositor)
        .unwrap();

    let modal_target_handle = match world.get::<RenderTarget>(modal_vello_camera) {
        Some(RenderTarget::Image(modal_target)) => modal_target.handle.clone(),
        _ => panic!("modal Vello camera should render into offscreen modal target"),
    };

    let mut sprite_query =
        world.query_filtered::<(&Sprite, &Visibility, &RenderLayers), Without<Mesh2d>>();
    let (sprite, visibility, _layers) = sprite_query
        .iter(&world)
        .find(|(sprite, _, layers)| {
            sprite.image == modal_target_handle
                && layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_MODAL_RENDER_LAYER))
        })
        .expect("modal composite sprite should exist");
    assert_eq!(visibility, &Visibility::Visible);
    assert_eq!(sprite.custom_size, Some(Vec2::new(1400.0, 900.0)));
}

/// Verifies the compositor quad mesh/UV contract expected by the upstream Vello texture-present path.
#[test]
fn hud_composite_quad_matches_upstream_vello_canvas_contract() {
    assert_eq!(HUD_COMPOSITE_FOREGROUND_Z, 0.0);

    let mut world = World::default();
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(
            |mut commands: Commands,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    let mesh_handle = world
        .query::<&Mesh2d>()
        .single(&world)
        .expect("composite mesh exists")
        .0
        .clone();
    let meshes = world.resource::<Assets<Mesh>>();
    let mesh = meshes.get(&mesh_handle).expect("mesh asset exists");
    let positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .expect("positions present")
        .as_float3()
        .expect("positions are float3");
    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0).expect("uvs present") {
        VertexAttributeValues::Float32x2(values) => values.as_slice(),
        other => panic!("unexpected uv attribute format: {other:?}"),
    };
    assert_eq!(
        positions,
        &[
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ]
    );
    assert_eq!(uvs, &[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [1.0, 1.0]]);
}

/// Verifies the color-space roundtrip assumption behind the HUD orange byte-preservation check.
#[test]
fn upstream_vello_present_contract_preserves_target_orange_bytes() {
    /// Converts one sRGB byte channel into linear space using the standard IEC transfer curve.
    fn srgb_to_linear_channel(value: u8) -> f32 {
        let srgb = value as f32 / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    }

    /// Converts one linear-space channel back into an sRGB byte using the inverse transfer curve.
    fn linear_to_srgb_channel(value: f32) -> u8 {
        let srgb = if value <= 0.0031308 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (srgb.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    let target = (225u8, 129u8, 10u8);
    let visible = (
        linear_to_srgb_channel(srgb_to_linear_channel(target.0)),
        linear_to_srgb_channel(srgb_to_linear_channel(target.1)),
        linear_to_srgb_channel(srgb_to_linear_channel(target.2)),
    );
    let wrong = (255u8, 177u8, 18u8);

    assert_eq!(visible, target);
    let target_dist = ((visible.0 as f32 - target.0 as f32).powi(2)
        + (visible.1 as f32 - target.1 as f32).powi(2)
        + (visible.2 as f32 - target.2 as f32).powi(2))
    .sqrt();
    let wrong_dist = ((visible.0 as f32 - wrong.0 as f32).powi(2)
        + (visible.1 as f32 - wrong.1 as f32).powi(2)
        + (visible.2 as f32 - wrong.2 as f32).powi(2))
    .sqrt();
    assert!(target_dist < wrong_dist);
}
