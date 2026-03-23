use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, ClearColorConfig},
    mesh::Indices,
    prelude::*,
    render::render_resource::PrimitiveTopology,
    sprite_render::MeshMaterial2d,
};
use bevy_vello::render::VelloCanvasMaterial;

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

pub(crate) const HUD_COMPOSITE_FOREGROUND_Z: f32 = 10.0;

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
);

type HudCompositeQuadQueryItem<'a> = (
    Entity,
    &'a HudCompositeLayerMarker,
    &'a MeshMaterial2d<VelloCanvasMaterial>,
    &'a mut Visibility,
);

pub(crate) fn sync_hud_offscreen_compositor(
    mut compositor: ResMut<HudOffscreenCompositor>,
    mut vello_materials: ResMut<Assets<VelloCanvasMaterial>>,
    mut commands: Commands,
    mut vello_canvases: Query<VelloCanvasQueryItem<'_>, Without<HudCompositeLayerMarker>>,
    mut quads: Query<HudCompositeQuadQueryItem<'_>>,
) {
    let mut vello_texture = None;
    for (entity, material_handle, maybe_visibility) in &mut vello_canvases {
        if let Some(mut visibility) = maybe_visibility {
            *visibility = Visibility::Hidden;
        } else {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        if vello_texture.is_none() {
            vello_texture = vello_materials
                .get(material_handle.id())
                .map(|material| material.texture.clone());
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
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}
