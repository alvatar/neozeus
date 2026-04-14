use bevy::{
    asset::RenderAssetUsages,
    camera::{
        visibility::{NoFrustumCulling, RenderLayers},
        ClearColorConfig, RenderTarget,
    },
    mesh::Indices,
    prelude::*,
    render::render_resource::{
        Extent3d, PrimitiveTopology, TextureDimension, TextureFormat, TextureUsages,
    },
    sprite_render::MeshMaterial2d,
    window::PrimaryWindow,
};
use bevy_vello::render::VelloCanvasMaterial;

use super::HudModalVectorSceneMarker;

pub(crate) const HUD_COMPOSITE_RENDER_LAYER: usize = 28;
pub(crate) const HUD_COMPOSITE_BLOOM_RENDER_LAYER: usize = 32;
pub(crate) const HUD_COMPOSITE_MODAL_RENDER_LAYER: usize = 34;
const HUD_COMPOSITE_CAMERA_ORDER: isize = 50;
pub(crate) const HUD_COMPOSITE_BLOOM_CAMERA_ORDER: isize = 51;
pub(crate) const HUD_COMPOSITE_MODAL_CAMERA_ORDER: isize = 52;

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

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudCompositeBloomCameraMarker;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudCompositeModalCameraMarker;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudCompositeModalSpriteMarker;

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
    bloom_camera_entity: Option<Entity>,
    modal_camera_entity: Option<Entity>,
    modal_sprite_entity: Option<Entity>,
    modal_target_image: Option<Handle<Image>>,
}

impl Default for HudOffscreenCompositor {
    /// Creates the default compositor state with one main-HUD layer and no spawned entities yet.
    ///
    /// Entity handles are left empty because setup is responsible for actually spawning the cameras and
    /// quads later.
    fn default() -> Self {
        Self {
            layers: vec![HudCompositeLayer {
                id: HudCompositeLayerId::MainHud,
                source: HudCompositeSource::VelloCanvas,
                composite_entity: None,
                texture: None,
            }],
            camera_entity: None,
            bloom_camera_entity: None,
            modal_camera_entity: None,
            modal_sprite_entity: None,
            modal_target_image: None,
        }
    }
}

impl HudOffscreenCompositor {
    /// Looks up one compositor layer definition by id.
    ///
    /// The compositor keeps a tiny vector of layers, so a linear search is sufficient and keeps the
    /// data structure simple.
    fn layer(&self, id: HudCompositeLayerId) -> Option<&HudCompositeLayer> {
        self.layers.iter().find(|layer| layer.id == id)
    }
}

/// Builds the fullscreen quad mesh used to display offscreen HUD textures.
///
/// The mesh is authored directly in clip-like space and then rendered by a dedicated compositor
/// camera, which avoids needing per-frame geometry generation.
fn create_modal_target_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image
}

fn image_matches_modal_target_size(
    images: &Assets<Image>,
    handle: &Handle<Image>,
    size: UVec2,
) -> bool {
    images
        .get(handle)
        .map(|image| {
            image.texture_descriptor.size.width == size.x.max(1)
                && image.texture_descriptor.size.height == size.y.max(1)
                && image.texture_descriptor.format == TextureFormat::Rgba8Unorm
        })
        .unwrap_or(false)
}

fn fullscreen_clip_mesh() -> Mesh {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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

/// Spawns the compositor camera and one fullscreen quad per configured HUD composite layer.
///
/// The setup is idempotent: if the camera or a layer quad already exists, it is left alone. New quads
/// start hidden and with an empty Vello canvas material until sync wires real textures into them.
pub(crate) fn setup_hud_offscreen_compositor(
    commands: &mut Commands,
    compositor: &mut HudOffscreenCompositor,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<VelloCanvasMaterial>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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
    if compositor.bloom_camera_entity.is_none() {
        compositor.bloom_camera_entity = Some(
            commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: HUD_COMPOSITE_BLOOM_CAMERA_ORDER,
                        output_mode: bevy::camera::CameraOutputMode::Write {
                            blend_state: Some(crate::hud::bloom::hud_bloom_additive_blend_state()),
                            clear_color: ClearColorConfig::None,
                        },
                        ..default()
                    },
                    RenderLayers::layer(HUD_COMPOSITE_BLOOM_RENDER_LAYER),
                    HudCompositeBloomCameraMarker,
                ))
                .id(),
        );
    }
    if compositor.modal_camera_entity.is_none() {
        compositor.modal_camera_entity = Some(
            commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: HUD_COMPOSITE_MODAL_CAMERA_ORDER,
                        clear_color: ClearColorConfig::None,
                        ..default()
                    },
                    RenderLayers::layer(HUD_COMPOSITE_MODAL_RENDER_LAYER),
                    HudCompositeModalCameraMarker,
                ))
                .id(),
        );
    }
    if compositor.modal_sprite_entity.is_none() {
        compositor.modal_sprite_entity = Some(
            commands
                .spawn((
                    Sprite {
                        image: Handle::default(),
                        custom_size: Some(Vec2::ONE),
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, HUD_COMPOSITE_FOREGROUND_Z + 0.02),
                    RenderLayers::layer(HUD_COMPOSITE_MODAL_RENDER_LAYER),
                    Visibility::Hidden,
                    HudCompositeModalSpriteMarker,
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

type ModalCompositeSpriteQueryItem<'a> = &'a mut Sprite;

#[allow(
    clippy::too_many_arguments,
    reason = "compositor sync needs window, image assets, materials, and visibility queries together"
)]
/// Synchronizes the compositor quads with the latest Vello canvas texture and window size.
///
/// The system hides the original Vello canvas entities, captures the first non-modal canvas texture,
/// assigns that texture to the compositor layer(s), and only makes the composite quad visible once the
/// texture size matches the expected primary-window size.
pub(crate) fn sync_hud_offscreen_compositor(
    mut compositor: ResMut<HudOffscreenCompositor>,
    mut images: ResMut<Assets<Image>>,
    mut vello_materials: ResMut<Assets<VelloCanvasMaterial>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut commands: Commands,
    modal_vello_cameras: Query<Entity, With<crate::hud::HudModalCameraMarker>>,
    mut vello_canvases: Query<VelloCanvasQueryItem<'_>, Without<HudCompositeLayerMarker>>,
    mut quads: Query<HudCompositeQuadQueryItem<'_>, Without<HudCompositeModalSpriteMarker>>,
    mut modal_sprite: Query<
        ModalCompositeSpriteQueryItem<'_>,
        (
            With<HudCompositeModalSpriteMarker>,
            Without<HudCompositeLayerMarker>,
        ),
    >,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let expected_size = UVec2::new(
        primary_window.physical_width().max(1),
        primary_window.physical_height().max(1),
    );
    if compositor
        .modal_target_image
        .as_ref()
        .is_none_or(|handle| !image_matches_modal_target_size(&images, handle, expected_size))
    {
        compositor.modal_target_image = Some(images.add(create_modal_target_image(expected_size)));
    }
    if let Some(modal_target_image) = compositor.modal_target_image.clone() {
        for modal_camera in &modal_vello_cameras {
            commands
                .entity(modal_camera)
                .insert(RenderTarget::Image(modal_target_image.clone().into()));
        }
        if let Some(modal_sprite_entity) = compositor.modal_sprite_entity {
            if let Ok(mut sprite) = modal_sprite.get_mut(modal_sprite_entity) {
                sprite.image = modal_target_image;
                sprite.custom_size =
                    Some(Vec2::new(primary_window.width(), primary_window.height()));
                commands
                    .entity(modal_sprite_entity)
                    .insert(Visibility::Visible);
            }
        }
    }

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

#[cfg(test)]
mod tests;
