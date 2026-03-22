use bevy::{prelude::*, sprite_render::MeshMaterial2d, window::PrimaryWindow};
use bevy_vello::render::VelloCanvasMaterial;

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

pub(crate) const HUD_COMPOSITE_FOREGROUND_Z: f32 = 10.0;

#[derive(Clone, Debug)]
struct HudCompositeLayer {
    id: HudCompositeLayerId,
    source: HudCompositeSource,
    z: f32,
    sprite_entity: Option<Entity>,
    texture: Option<Handle<Image>>,
}

#[derive(Resource, Clone, Debug)]
pub(crate) struct HudOffscreenCompositor {
    layers: Vec<HudCompositeLayer>,
}

impl Default for HudOffscreenCompositor {
    fn default() -> Self {
        Self {
            layers: vec![HudCompositeLayer {
                id: HudCompositeLayerId::MainHud,
                source: HudCompositeSource::VelloCanvas,
                z: HUD_COMPOSITE_FOREGROUND_Z,
                sprite_entity: None,
                texture: None,
            }],
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
) {
    for layer in &mut compositor.layers {
        if layer.sprite_entity.is_some() {
            continue;
        }

        let entity = commands
            .spawn((
                Sprite::from_image(Handle::default()),
                Transform::from_xyz(0.0, 0.0, layer.z),
                Visibility::Hidden,
                HudCompositeLayerMarker { id: layer.id },
            ))
            .id();
        layer.sprite_entity = Some(entity);
    }
}

pub(crate) fn sync_hud_offscreen_compositor(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut compositor: ResMut<HudOffscreenCompositor>,
    materials: Res<Assets<VelloCanvasMaterial>>,
    mut vello_canvases: Query<
        (
            &MeshMaterial2d<VelloCanvasMaterial>,
            Option<&mut Visibility>,
        ),
        Without<HudCompositeLayerMarker>,
    >,
    mut sprites: Query<(
        Entity,
        &HudCompositeLayerMarker,
        &mut Sprite,
        &mut Transform,
        &mut Visibility,
    )>,
) {
    let mut vello_texture = None;
    for (material_handle, maybe_visibility) in &mut vello_canvases {
        if let Some(mut visibility) = maybe_visibility {
            *visibility = Visibility::Hidden;
        }
        if vello_texture.is_none() {
            vello_texture = materials
                .get(material_handle.id())
                .map(|material| material.texture.clone());
        }
    }

    for layer in &mut compositor.layers {
        layer.texture = match layer.source {
            HudCompositeSource::VelloCanvas => vello_texture.clone(),
        };
    }

    let sprite_size = Vec2::new(primary_window.width(), primary_window.height());
    for (entity, marker, mut sprite, mut transform, mut visibility) in &mut sprites {
        let Some(layer) = compositor.layer(marker.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if layer.sprite_entity != Some(entity) {
            *visibility = Visibility::Hidden;
            continue;
        }

        transform.translation = Vec3::new(0.0, 0.0, layer.z);
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;
        sprite.custom_size = Some(sprite_size);

        if let Some(texture) = layer.texture.clone() {
            sprite.image = texture;
            sprite.color = Color::WHITE;
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}
