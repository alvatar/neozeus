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
    window::PrimaryWindow,
};
use bevy_vello::render::VelloCanvasMaterial;

use super::HudModalVectorSceneMarker;

pub(crate) const HUD_COMPOSITE_RENDER_LAYER: usize = 28;
const HUD_COMPOSITE_CAMERA_ORDER: isize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HudCompositeLayerId {
    MainHud,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum HudCompositeSource {
    VelloCanvas,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudCompositeLayerMarker {
    pub(crate) id: HudCompositeLayerId,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudCompositeCameraMarker;

pub(crate) const HUD_COMPOSITE_FOREGROUND_Z: f32 = 0.0;

#[derive(Clone, Debug)]
struct HudCompositeLayer {
    id: HudCompositeLayerId,
    source: HudCompositeSource,
    composite_entity: Option<Entity>,
    texture: Option<Handle<Image>>,
}

#[derive(Resource, Clone, Debug)]
pub(crate) struct HudOffscreenCompositor {
    layers: Vec<HudCompositeLayer>,
    camera_entity: Option<Entity>,
}

impl Default for HudOffscreenCompositor {
    fn default() -> Self {
        Self {
            layers: vec![HudCompositeLayer {
                id: HudCompositeLayerId::MainHud,
                source: HudCompositeSource::VelloCanvas,
                composite_entity: None,
                texture: None,
            }],
            camera_entity: None,
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
    compositor: &mut HudOffscreenCompositor,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<VelloCanvasMaterial>,
) {
    if compositor.camera_entity.is_none() {
        compositor.camera_entity = Some(
            commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: HUD_COMPOSITE_CAMERA_ORDER,
                        clear_color: ClearColorConfig::None,
                        ..default()
                    },
                    RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER),
                    HudCompositeCameraMarker,
                ))
                .id(),
        );
    }

    let mesh_handle = meshes.add(fullscreen_clip_mesh());
    for layer in &mut compositor.layers {
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
                RenderLayers::layer(HUD_COMPOSITE_RENDER_LAYER),
                Visibility::Hidden,
                HudCompositeLayerMarker { id: layer.id },
            ))
            .id();
        layer.composite_entity = Some(entity);
    }
}

type VelloCanvasQueryItem<'a> = (
    Entity,
    &'a MeshMaterial2d<VelloCanvasMaterial>,
    Option<&'a mut Visibility>,
    Option<&'a HudModalVectorSceneMarker>,
);

type HudCompositeQuadQueryItem<'a> = (
    Entity,
    &'a HudCompositeLayerMarker,
    &'a MeshMaterial2d<VelloCanvasMaterial>,
    &'a mut Visibility,
);

#[allow(
    clippy::too_many_arguments,
    reason = "compositor sync needs window, image assets, materials, and visibility queries together"
)]
pub(crate) fn sync_hud_offscreen_compositor(
    mut compositor: ResMut<HudOffscreenCompositor>,
    images: Res<Assets<Image>>,
    mut vello_materials: ResMut<Assets<VelloCanvasMaterial>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut commands: Commands,
    mut vello_canvases: Query<VelloCanvasQueryItem<'_>, Without<HudCompositeLayerMarker>>,
    mut quads: Query<HudCompositeQuadQueryItem<'_>>,
) {
    let expected_size = UVec2::new(
        primary_window.physical_width().max(1),
        primary_window.physical_height().max(1),
    );
    let mut vello_texture = None;
    let mut vello_texture_size = None;
    for (entity, material_handle, maybe_visibility, modal_marker) in &mut vello_canvases {
        if modal_marker.is_some() {
            continue;
        }
        if let Some(mut visibility) = maybe_visibility {
            *visibility = Visibility::Hidden;
        } else {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        if vello_texture.is_none() {
            if let Some(material) = vello_materials.get(material_handle.id()) {
                let texture = material.texture.clone();
                vello_texture_size = images.get(texture.id()).map(|image| {
                    UVec2::new(
                        image.texture_descriptor.size.width,
                        image.texture_descriptor.size.height,
                    )
                });
                vello_texture = Some(texture);
            }
        }
    }

    for layer in &mut compositor.layers {
        layer.texture = match layer.source {
            HudCompositeSource::VelloCanvas => vello_texture.clone(),
        };
    }

    for (entity, marker, material_handle, mut visibility) in &mut quads {
        let Some(layer) = compositor.layer(marker.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if layer.composite_entity != Some(entity) {
            *visibility = Visibility::Hidden;
            continue;
        }

        if let Some(texture) = layer.texture.clone() {
            if let Some(material) = vello_materials.get_mut(material_handle.id()) {
                material.texture = texture;
            }
            let ready = vello_texture_size == Some(expected_size);
            *visibility = if ready {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}
