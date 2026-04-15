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

use super::render::{HUD_MODAL_CAMERA_ORDER, HUD_OVERLAY_CAMERA_ORDER};
use super::{HudLayerId, HudLayerRegistry};

pub(crate) const HUD_COMPOSITE_RENDER_LAYER: usize = 28;
pub(crate) const HUD_OVERLAY_COMPOSITE_RENDER_LAYER: usize = 35;
pub(crate) const HUD_MODAL_COMPOSITE_RENDER_LAYER: usize = 36;
const HUD_COMPOSITE_CAMERA_ORDER: isize = 50;
const HUD_OVERLAY_COMPOSITE_CAMERA_ORDER: isize = HUD_OVERLAY_CAMERA_ORDER;
const HUD_MODAL_COMPOSITE_CAMERA_ORDER: isize = HUD_MODAL_CAMERA_ORDER;

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

    fn render_layer(self) -> usize {
        match self {
            Self::Main => HUD_COMPOSITE_RENDER_LAYER,
            Self::Overlay => HUD_OVERLAY_COMPOSITE_RENDER_LAYER,
            Self::Modal => HUD_MODAL_COMPOSITE_RENDER_LAYER,
        }
    }

    fn camera_order(self) -> isize {
        match self {
            Self::Main => HUD_COMPOSITE_CAMERA_ORDER,
            Self::Overlay => HUD_OVERLAY_COMPOSITE_CAMERA_ORDER,
            Self::Modal => HUD_MODAL_COMPOSITE_CAMERA_ORDER,
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
    texture: Option<Handle<Image>>,
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
                    texture: None,
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
        if layer.camera_entity.is_none() {
            let camera = commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: layer.id.camera_order(),
                        clear_color: ClearColorConfig::None,
                        ..default()
                    },
                    RenderLayers::layer(layer.id.render_layer()),
                    HudCompositeCameraMarker { id: layer.id },
                ))
                .id();
            layer.camera_entity = Some(camera);
            layer_registry.set_camera_entity(layer.id.hud_layer_id(), camera);
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
                RenderLayers::layer(layer.id.render_layer()),
                Visibility::Hidden,
                HudCompositeLayerMarker { id: layer.id },
            ))
            .id();
        layer.composite_entity = Some(entity);
    }
}

type VelloCanvasQueryItem<'a> = (
    &'a MeshMaterial2d<VelloCanvasMaterial>,
    Option<&'a mut Visibility>,
);

type HudCompositeQuadQueryItem<'a> = (
    Entity,
    &'a HudCompositeLayerMarker,
    &'a MeshMaterial2d<VelloCanvasMaterial>,
    &'a mut Visibility,
);

#[allow(
    clippy::too_many_arguments,
    reason = "compositor sync needs registry, image assets, materials, and visibility queries together"
)]
pub(crate) fn sync_hud_offscreen_compositor(
    mut compositor: ResMut<HudOffscreenCompositor>,
    layers: Res<HudLayerRegistry>,
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

    for layer in &mut compositor.layers {
        let Some(scene_entity) = layers
            .layer(layer.id.hud_layer_id())
            .and_then(|runtime| runtime.scene_entity)
        else {
            layer.texture = None;
            continue;
        };
        let Ok((material_handle, maybe_visibility)) = vello_canvases.get_mut(scene_entity) else {
            layer.texture = None;
            continue;
        };
        if let Some(mut visibility) = maybe_visibility {
            *visibility = Visibility::Hidden;
        } else {
            commands.entity(scene_entity).insert(Visibility::Hidden);
        }
        layer.texture = vello_materials
            .get(material_handle.id())
            .map(|material| material.texture.clone());
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
                material.texture = texture.clone();
            }
            let ready = images.get(texture.id()).is_some_and(|image| {
                UVec2::new(
                    image.texture_descriptor.size.width,
                    image.texture_descriptor.size.height,
                ) == expected_size
            });
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

#[cfg(test)]
mod tests;
