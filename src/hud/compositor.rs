use bevy::{
    camera::{visibility::RenderLayers, ClearColorConfig},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, MeshMaterial2d},
    window::PrimaryWindow,
};
use bevy_vello::render::VelloCanvasMaterial;

const HUD_COMPOSITE_SHADER_PATH: &str = "shaders/hud_composite.wgsl";
pub(crate) const HUD_COMPOSITE_RENDER_LAYER: usize = 28;
const HUD_COMPOSITE_CAMERA_ORDER: isize = 50;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct HudCompositeMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub(crate) texture: Handle<Image>,
}

impl Material2d for HudCompositeMaterial {
    fn fragment_shader() -> ShaderRef {
        HUD_COMPOSITE_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(target) = descriptor.fragment.as_mut() {
            if let Some(Some(target)) = target.targets.get_mut(0) {
                target.blend = Some(bevy::render::render_resource::BlendState::ALPHA_BLENDING);
            }
        }
        Ok(())
    }
}

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
    z: f32,
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
                z: HUD_COMPOSITE_FOREGROUND_Z,
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

pub(crate) fn setup_hud_offscreen_compositor(
    commands: &mut Commands,
    compositor: &mut HudOffscreenCompositor,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<HudCompositeMaterial>,
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

    for layer in &mut compositor.layers {
        if layer.composite_entity.is_some() {
            continue;
        }

        let entity = commands
            .spawn((
                Mesh2d(meshes.add(Rectangle::default())),
                MeshMaterial2d(materials.add(HudCompositeMaterial {
                    texture: Handle::default(),
                })),
                Transform::from_xyz(0.0, 0.0, layer.z),
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
    &'a MeshMaterial2d<HudCompositeMaterial>,
    &'a mut Transform,
    &'a mut Visibility,
);

pub(crate) fn sync_hud_offscreen_compositor(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut compositor: ResMut<HudOffscreenCompositor>,
    vello_materials: Res<Assets<VelloCanvasMaterial>>,
    mut composite_materials: ResMut<Assets<HudCompositeMaterial>>,
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

    let quad_size = Vec2::new(primary_window.width(), primary_window.height());
    for (entity, marker, material_handle, mut transform, mut visibility) in &mut quads {
        let Some(layer) = compositor.layer(marker.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if layer.composite_entity != Some(entity) {
            *visibility = Visibility::Hidden;
            continue;
        }

        transform.translation = Vec3::new(0.0, 0.0, layer.z);
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::new(quad_size.x, quad_size.y, 1.0);

        if let Some(texture) = layer.texture.clone() {
            if let Some(material) = composite_materials.get_mut(material_handle.id()) {
                material.texture = texture;
            }
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}
