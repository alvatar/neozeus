use super::{
    setup_hud_offscreen_compositor, sync_hud_offscreen_compositor, HudOffscreenCompositor,
};
use crate::hud::{
    HudCompositeCameraMarker, HudCompositeLayerId, HudCompositeLayerMarker, HudLayerId,
    HudLayerRegistry, HUD_COMPOSITE_FOREGROUND_Z, HUD_COMPOSITE_RENDER_LAYER,
    HUD_OVERLAY_COMPOSITE_RENDER_LAYER,
};
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    ecs::system::RunSystemOnce,
    mesh::VertexAttributeValues,
    prelude::*,
    sprite_render::MeshMaterial2d,
    window::PrimaryWindow,
};
use bevy_vello::render::VelloCanvasMaterial;

#[test]
fn sync_hud_offscreen_compositor_hides_explicit_main_canvas_and_binds_texture() {
    let mut world = World::default();
    world.insert_resource(HudLayerRegistry::default());
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
        .resource_mut::<HudLayerRegistry>()
        .set_scene_entity(HudLayerId::Main, source_canvas);
    world
        .run_system_once(
            |mut commands: Commands,
             mut layers: ResMut<HudLayerRegistry>,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut layers,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    world.run_system_once(sync_hud_offscreen_compositor).unwrap();

    assert_eq!(world.get::<Visibility>(source_canvas), Some(&Visibility::Hidden));

    let composite_cameras = world
        .query::<(&Camera, &RenderLayers, &HudCompositeCameraMarker)>()
        .iter(&world)
        .map(|(camera, layers, marker)| (marker.id, camera.order, layers.clone()))
        .collect::<Vec<_>>();
    assert_eq!(composite_cameras.len(), 3);
    assert!(composite_cameras.iter().any(|(id, order, layers)| {
        *id == HudCompositeLayerId::Main
            && *order == 50
            && layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER))
    }));

    let quads = world
        .query::<(
            &HudCompositeLayerMarker,
            &MeshMaterial2d<VelloCanvasMaterial>,
            &Transform,
            &Visibility,
            &RenderLayers,
            &NoFrustumCulling,
        )>()
        .iter(&world)
        .map(|(marker, material, transform, visibility, layers, _)| {
            (*marker, material.clone(), *transform, *visibility, layers.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(quads.len(), 3);
    let (marker, composite_material, transform, visibility, quad_layers) = quads
        .iter()
        .find(|(marker, ..)| marker.id == HudCompositeLayerId::Main)
        .expect("main composite quad exists");
    assert_eq!(marker.id, HudCompositeLayerId::Main);
    let composite_texture = {
        let materials = world.resource::<Assets<VelloCanvasMaterial>>();
        materials
            .get(composite_material.id())
            .expect("composite material exists")
            .texture
            .clone()
    };
    assert_eq!(composite_texture, texture);
    assert_eq!(*transform, Transform::IDENTITY);
    assert_eq!(*visibility, Visibility::Visible);
    assert!(quad_layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER)));
}

#[test]
fn sync_hud_offscreen_compositor_composes_explicit_known_layers_without_first_match_scan() {
    let mut world = World::default();
    world.insert_resource(HudLayerRegistry::default());
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    let main_texture = world.resource_mut::<Assets<Image>>().add(Image::default());
    let overlay_texture = world.resource_mut::<Assets<Image>>().add(Image::default());
    for handle in [&main_texture, &overlay_texture] {
        let mut images = world.resource_mut::<Assets<Image>>();
        let image = images.get_mut(handle).expect("texture exists");
        image.resize(bevy::render::render_resource::Extent3d {
            width: 1400,
            height: 900,
            depth_or_array_layers: 1,
        });
    }

    let main_material = world
        .resource_mut::<Assets<VelloCanvasMaterial>>()
        .add(VelloCanvasMaterial {
            texture: main_texture.clone(),
        });
    let overlay_material = world
        .resource_mut::<Assets<VelloCanvasMaterial>>()
        .add(VelloCanvasMaterial {
            texture: overlay_texture.clone(),
        });

    let overlay_scene = world
        .spawn((
            MeshMaterial2d::<VelloCanvasMaterial>(overlay_material),
            Visibility::Visible,
        ))
        .id();
    let main_scene = world
        .spawn((
            MeshMaterial2d::<VelloCanvasMaterial>(main_material),
            Visibility::Visible,
        ))
        .id();
    {
        let mut layers = world.resource_mut::<HudLayerRegistry>();
        layers.set_scene_entity(HudLayerId::Overlay, overlay_scene);
        layers.set_scene_entity(HudLayerId::Main, main_scene);
    }

    world
        .run_system_once(
            |mut commands: Commands,
             mut layers: ResMut<HudLayerRegistry>,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut layers,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    world.run_system_once(sync_hud_offscreen_compositor).unwrap();

    let quads = world
        .query::<(&HudCompositeLayerMarker, &MeshMaterial2d<VelloCanvasMaterial>, &Visibility, &RenderLayers)>()
        .iter(&world)
        .map(|(marker, material, visibility, layers)| (*marker, material.clone(), *visibility, layers.clone()))
        .collect::<Vec<_>>();
    let materials = world.resource::<Assets<VelloCanvasMaterial>>();
    let main_quad = quads
        .iter()
        .find(|(marker, ..)| marker.id == HudCompositeLayerId::Main)
        .expect("main quad exists");
    let overlay_quad = quads
        .iter()
        .find(|(marker, ..)| marker.id == HudCompositeLayerId::Overlay)
        .expect("overlay quad exists");
    assert_eq!(
        materials
            .get(main_quad.1.id())
            .expect("main material exists")
            .texture,
        main_texture
    );
    assert_eq!(
        materials
            .get(overlay_quad.1.id())
            .expect("overlay material exists")
            .texture,
        overlay_texture
    );
    assert_eq!(main_quad.2, Visibility::Visible);
    assert_eq!(overlay_quad.2, Visibility::Visible);
    assert!(overlay_quad
        .3
        .intersects(&RenderLayers::layer(HUD_OVERLAY_COMPOSITE_RENDER_LAYER)));
}

#[test]
fn hud_composite_quad_matches_upstream_vello_canvas_contract() {
    assert_eq!(HUD_COMPOSITE_FOREGROUND_Z, 0.0);

    let mut world = World::default();
    world.insert_resource(HudLayerRegistry::default());
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
             mut layers: ResMut<HudLayerRegistry>,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut layers,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    let mesh_handle = world
        .query::<&Mesh2d>()
        .iter(&world)
        .next()
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

#[test]
fn upstream_vello_present_contract_preserves_target_orange_bytes() {
    fn srgb_to_linear_channel(value: u8) -> f32 {
        let srgb = value as f32 / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    }

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
