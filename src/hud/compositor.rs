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
mod tests;
