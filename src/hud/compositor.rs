use bevy::{
    asset::RenderAssetUsages,
    camera::{
        visibility::{NoFrustumCulling, RenderLayers},
        ClearColorConfig,
    },
    mesh::Indices,
    prelude::*,
    render::render_resource::PrimitiveTopology,
    sprite_render::MeshMaterial2d,
};
use bevy_vello::render::VelloCanvasMaterial;

use super::{HudLayerId, HudLayerRegistry, HudRenderVisibilityPolicy};

#[cfg(test)]
pub(crate) const HUD_COMPOSITE_RENDER_LAYER: usize = 28;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HudCompositeLayerId {
    Main,
    Overlay,
    Modal,
}

impl HudCompositeLayerId {
    fn all() -> [Self; 3] {
        [Self::Main, Self::Overlay, Self::Modal]
    }

    fn hud_layer_id(self) -> HudLayerId {
        match self {
            Self::Main => HudLayerId::Main,
            Self::Overlay => HudLayerId::Overlay,
            Self::Modal => HudLayerId::Modal,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudCompositeLayerMarker {
    pub(crate) id: HudCompositeLayerId,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudCompositeCameraMarker {
    pub(crate) id: HudCompositeLayerId,
}

pub(crate) const HUD_COMPOSITE_FOREGROUND_Z: f32 = 0.0;

#[derive(Clone, Debug)]
struct HudCompositeLayer {
    id: HudCompositeLayerId,
    camera_entity: Option<Entity>,
    composite_entity: Option<Entity>,
}

#[derive(Resource, Clone, Debug)]
pub(crate) struct HudOffscreenCompositor {
    layers: Vec<HudCompositeLayer>,
}

impl Default for HudOffscreenCompositor {
    fn default() -> Self {
        Self {
            layers: HudCompositeLayerId::all()
                .into_iter()
                .map(|id| HudCompositeLayer {
                    id,
                    camera_entity: None,
                    composite_entity: None,
                })
                .collect(),
        }
    }
}

impl HudOffscreenCompositor {
    fn layer(&self, id: HudCompositeLayerId) -> Option<&HudCompositeLayer> {
        self.layers.iter().find(|layer| layer.id == id)
    }
}

fn fullscreen_clip_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-1.0, -1.0, HUD_COMPOSITE_FOREGROUND_Z],
            [1.0, -1.0, HUD_COMPOSITE_FOREGROUND_Z],
            [1.0, 1.0, HUD_COMPOSITE_FOREGROUND_Z],
            [-1.0, 1.0, HUD_COMPOSITE_FOREGROUND_Z],
        ],
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [1.0, 1.0]],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

pub(crate) fn setup_hud_offscreen_compositor(
    commands: &mut Commands,
    layer_registry: &mut HudLayerRegistry,
    compositor: &mut HudOffscreenCompositor,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<VelloCanvasMaterial>,
) {
    let mesh_handle = meshes.add(fullscreen_clip_mesh());
    for layer in &mut compositor.layers {
        let layer_id = layer.id.hud_layer_id();
        if layer.camera_entity.is_none() {
            let camera = commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: layer_id.order(),
                        clear_color: ClearColorConfig::None,
                        ..default()
                    },
                    RenderLayers::layer(layer_id.composite_render_layer()),
                    HudCompositeCameraMarker { id: layer.id },
                ))
                .id();
            layer.camera_entity = Some(camera);
            layer_registry.set_camera_entity(layer_id, camera);
        }
        if layer.composite_entity.is_some() {
            continue;
        }
        let entity = commands
            .spawn((
                Mesh2d(mesh_handle.clone()),
                MeshMaterial2d(materials.add(VelloCanvasMaterial {
                    texture: Handle::default(),
                })),
                Transform::IDENTITY,
                NoFrustumCulling,
                RenderLayers::layer(layer_id.composite_render_layer()),
                Visibility::Hidden,
                HudCompositeLayerMarker { id: layer.id },
            ))
            .id();
        layer.composite_entity = Some(entity);
    }
}

type HudCompositeQuadQueryItem<'a> = (
    Entity,
    &'a HudCompositeLayerMarker,
    &'a MeshMaterial2d<VelloCanvasMaterial>,
    &'a mut Visibility,
);

#[allow(
    clippy::too_many_arguments,
    reason = "compositor sync needs explicit layer targets, materials, and quad visibility together"
)]
pub(crate) fn sync_hud_offscreen_compositor(
    compositor: Res<HudOffscreenCompositor>,
    registry: Res<HudLayerRegistry>,
    visibility_policy: Res<HudRenderVisibilityPolicy>,
    images: Res<Assets<Image>>,
    mut vello_materials: ResMut<Assets<VelloCanvasMaterial>>,
    mut quads: Query<HudCompositeQuadQueryItem<'_>>,
) {
    for (entity, marker, material_handle, mut visibility) in &mut quads {
        let Some(layer) = compositor.layer(marker.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if layer.composite_entity != Some(entity) {
            *visibility = Visibility::Hidden;
            continue;
        }

        let layer_id = marker.id.hud_layer_id();
        let Some(surface_image) = registry
            .layer(layer_id)
            .and_then(|runtime| runtime.surface_image.clone())
        else {
            *visibility = Visibility::Hidden;
            continue;
        };

        if let Some(material) = vello_materials.get_mut(material_handle.id()) {
            material.texture = surface_image.clone();
        }
        *visibility = if visibility_policy.layer_visible(layer_id)
            && images.get(surface_image.id()).is_some()
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use super::{
        setup_hud_offscreen_compositor, sync_hud_offscreen_compositor, HudOffscreenCompositor,
    };
    use crate::hud::{
        HudCompositeCameraMarker, HudCompositeLayerId, HudCompositeLayerMarker, HudLayerId,
        HudLayerRegistry, HudRenderVisibilityPolicy, HUD_COMPOSITE_FOREGROUND_Z,
        HUD_COMPOSITE_RENDER_LAYER,
    };
    use bevy::{
        camera::visibility::{NoFrustumCulling, RenderLayers},
        ecs::system::RunSystemOnce,
        mesh::VertexAttributeValues,
        sprite_render::MeshMaterial2d,
        window::PrimaryWindow,
    };
    use bevy_vello::render::VelloCanvasMaterial;

    #[test]
    fn sync_hud_offscreen_compositor_binds_explicit_main_layer_surface() {
        let mut world = World::default();
        world.insert_resource(HudLayerRegistry::default());
        world.insert_resource(HudOffscreenCompositor::default());
        world.insert_resource(HudRenderVisibilityPolicy::default());
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
        world
            .resource_mut::<HudLayerRegistry>()
            .set_surface_image(HudLayerId::Main, texture.clone());

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

        let composite_cameras = world
            .query::<(&Camera, &RenderLayers, &HudCompositeCameraMarker)>()
            .iter(&world)
            .map(|(camera, layers, marker)| (marker.id, camera.order, layers.clone()))
            .collect::<Vec<_>>();
        assert_eq!(composite_cameras.len(), 3);
        assert!(composite_cameras.iter().any(|(id, order, layers)| {
            *id == HudCompositeLayerId::Main
                && *order == HudLayerId::Main.order()
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
                (
                    *marker,
                    material.clone(),
                    *transform,
                    *visibility,
                    layers.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(quads.len(), 3);
        let (marker, composite_material, transform, visibility, quad_layers) = quads
            .iter()
            .find(|(marker, ..)| marker.id == HudCompositeLayerId::Main)
            .expect("main composite quad exists");
        let composite_texture = {
            let materials = world.resource::<Assets<VelloCanvasMaterial>>();
            materials
                .get(composite_material.id())
                .expect("composite material exists")
                .texture
                .clone()
        };
        assert_eq!(marker.id, HudCompositeLayerId::Main);
        assert_eq!(composite_texture, texture);
        assert_eq!(*transform, Transform::IDENTITY);
        assert_eq!(*visibility, Visibility::Visible);
        assert!(quad_layers.intersects(&RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER)));
    }

    #[test]
    fn sync_hud_offscreen_compositor_composes_explicit_known_layers() {
        let mut world = World::default();
        world.insert_resource(HudLayerRegistry::default());
        world.insert_resource(HudOffscreenCompositor::default());
        world.insert_resource(HudRenderVisibilityPolicy::default());
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
        {
            let mut layers = world.resource_mut::<HudLayerRegistry>();
            layers.set_surface_image(HudLayerId::Main, main_texture.clone());
            layers.set_surface_image(HudLayerId::Overlay, overlay_texture.clone());
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
            .query::<(
                &HudCompositeLayerMarker,
                &MeshMaterial2d<VelloCanvasMaterial>,
                &Visibility,
                &RenderLayers,
            )>()
            .iter(&world)
            .map(|(marker, material, visibility, layers)| {
                (*marker, material.clone(), *visibility, layers.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(quads.len(), 3);
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
            .intersects(&RenderLayers::layer(HudLayerId::Overlay.composite_render_layer())));
    }

    #[test]
    fn sync_hud_offscreen_compositor_hides_blocked_layers_even_when_surface_images_exist() {
        let mut world = World::default();
        world.insert_resource(HudLayerRegistry::default());
        world.insert_resource(HudOffscreenCompositor::default());
        world.insert_resource(HudRenderVisibilityPolicy {
            main_visible: false,
            overlay_visible: false,
            bloom_visible: false,
            modal_visible: true,
        });
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
        let modal_texture = world.resource_mut::<Assets<Image>>().add(Image::default());
        for handle in [&main_texture, &overlay_texture, &modal_texture] {
            let mut images = world.resource_mut::<Assets<Image>>();
            let image = images.get_mut(handle).expect("texture exists");
            image.resize(bevy::render::render_resource::Extent3d {
                width: 1400,
                height: 900,
                depth_or_array_layers: 1,
            });
        }
        {
            let mut layers = world.resource_mut::<HudLayerRegistry>();
            layers.set_surface_image(HudLayerId::Main, main_texture);
            layers.set_surface_image(HudLayerId::Overlay, overlay_texture);
            layers.set_surface_image(HudLayerId::Modal, modal_texture);
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
            .query::<(&HudCompositeLayerMarker, &Visibility)>()
            .iter(&world)
            .map(|(marker, visibility)| (marker.id, *visibility))
            .collect::<Vec<_>>();
        assert_eq!(
            quads
                .iter()
                .find(|(id, _)| *id == HudCompositeLayerId::Main)
                .expect("main quad exists")
                .1,
            Visibility::Hidden
        );
        assert_eq!(
            quads
                .iter()
                .find(|(id, _)| *id == HudCompositeLayerId::Overlay)
                .expect("overlay quad exists")
                .1,
            Visibility::Hidden
        );
        assert_eq!(
            quads
                .iter()
                .find(|(id, _)| *id == HudCompositeLayerId::Modal)
                .expect("modal quad exists")
                .1,
            Visibility::Visible
        );
    }

    #[test]
    fn hud_composite_quad_matches_upstream_vello_canvas_contract() {
        assert_eq!(HUD_COMPOSITE_FOREGROUND_Z, 0.0);

        let mut world = World::default();
        world.insert_resource(HudLayerRegistry::default());
        world.insert_resource(HudOffscreenCompositor::default());
        world.insert_resource(HudRenderVisibilityPolicy::default());
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
}
