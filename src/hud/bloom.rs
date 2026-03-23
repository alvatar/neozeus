use crate::hud::{
    modules::{agent_row_rect, agent_rows, AgentListRowSection},
    AgentDirectory, HudModuleId, HudRect, HudState,
};
use crate::terminals::{TerminalId, TerminalManager};
use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, ClearColorConfig, RenderTarget},
    ecs::system::SystemParam,
    image::ImageSampler,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureUsages,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, MeshMaterial2d},
    window::PrimaryWindow,
};
use std::env;

use super::compositor::HUD_COMPOSITE_FOREGROUND_Z;

const BLOOM_SOURCE_LAYER: usize = 29;
const BLOOM_BLUR_H_LAYER: usize = 30;
const BLOOM_BLUR_V_LAYER: usize = 31;
const BLOOM_COMPOSITE_Z: f32 = HUD_COMPOSITE_FOREGROUND_Z + 0.1;
const BLOOM_TARGET_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const BLOOM_BLUR_SHADER_PATH: &str = "shaders/hud_agent_list_bloom_blur.wgsl";
const DEFAULT_BLOOM_INTENSITY: f32 = 1.35;
const BLOOM_COMPOSITE_GAIN: f32 = 0.92;

#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct HudBloomSettings {
    pub(crate) agent_list_intensity: f32,
}

impl Default for HudBloomSettings {
    fn default() -> Self {
        Self {
            agent_list_intensity: resolve_agent_list_bloom_intensity(
                env::var("NEOZEUS_AGENT_BLOOM_INTENSITY").ok().as_deref(),
            ),
        }
    }
}

pub(crate) fn resolve_agent_list_bloom_intensity(raw: Option<&str>) -> f32 {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(DEFAULT_BLOOM_INTENSITY)
}

#[derive(Component)]
pub(crate) struct AgentListBloomCameraMarker;

#[derive(Component)]
pub(crate) struct AgentListBloomCompositeMarker;

#[derive(Component)]
struct AgentListBloomBlurHorizontalCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurVerticalCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurHorizontalQuadMarker;

#[derive(Component)]
struct AgentListBloomBlurVerticalQuadMarker;

#[derive(Clone, Copy, Debug, ShaderType)]
pub(crate) struct AgentListBloomBlurUniform {
    pub(crate) texel_step_gain: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct AgentListBloomBlurMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub(crate) image: Handle<Image>,
    #[uniform(2)]
    pub(crate) uniform: AgentListBloomBlurUniform,
}

impl Material2d for AgentListBloomBlurMaterial {
    fn fragment_shader() -> ShaderRef {
        BLOOM_BLUR_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AgentListBloomSourceKind {
    Accent,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AgentListBloomSourceSprite {
    pub(crate) terminal_id: TerminalId,
    pub(crate) kind: AgentListBloomSourceKind,
}

#[derive(Clone, Debug)]
struct BloomSourceSpec {
    key: AgentListBloomSourceSprite,
    rect: HudRect,
    color: Color,
}

#[derive(Clone, Debug, Default)]
struct AgentListBloomPass {
    source_image: Handle<Image>,
    blur_h_image: Handle<Image>,
    blur_v_image: Handle<Image>,
    source_camera: Option<Entity>,
    blur_h_camera: Option<Entity>,
    blur_v_camera: Option<Entity>,
    blur_h_quad: Option<Entity>,
    blur_v_quad: Option<Entity>,
    composite_sprite: Option<Entity>,
}

#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct HudWidgetBloom {
    agent_list: AgentListBloomPass,
}

fn bloom_target_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0, 0, 0, 0, 0],
        BLOOM_TARGET_FORMAT,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    image
}

fn image_matches_size(images: &Assets<Image>, handle: &Handle<Image>, size: UVec2) -> bool {
    images
        .get(handle)
        .map(|image| {
            image.texture_descriptor.size.width == size.x.max(1)
                && image.texture_descriptor.size.height == size.y.max(1)
                && image.texture_descriptor.format == BLOOM_TARGET_FORMAT
        })
        .unwrap_or(false)
}

fn window_size(window: &Window) -> UVec2 {
    UVec2::new(
        window.width().round().max(1.0) as u32,
        window.height().round().max(1.0) as u32,
    )
}

fn rect_transform(window: &Window, rect: HudRect, z: f32) -> Transform {
    Transform::from_xyz(
        rect.x + rect.w * 0.5 - window.width() * 0.5,
        window.height() * 0.5 - (rect.y + rect.h * 0.5),
        z,
    )
}

fn fullscreen_transform(window: &Window, z: f32) -> Transform {
    Transform::from_xyz(0.0, 0.0, z).with_scale(Vec3::new(window.width(), window.height(), 1.0))
}

fn blur_uniform(texel_step: Vec2) -> AgentListBloomBlurUniform {
    AgentListBloomBlurUniform {
        texel_step_gain: Vec4::new(texel_step.x, texel_step.y, 1.0, 0.0),
    }
}

fn bloom_source_color(focused: bool, hovered: bool, kind: AgentListBloomSourceKind) -> Color {
    match (focused, hovered, kind) {
        (true, _, AgentListBloomSourceKind::Accent) => Color::linear_rgba(3.2, 0.06, 0.12, 0.70),
        (_, true, AgentListBloomSourceKind::Accent) => Color::linear_rgba(1.5, 0.04, 0.08, 0.22),
        (_, _, AgentListBloomSourceKind::Accent) => Color::linear_rgba(0.8, 0.03, 0.06, 0.10),
    }
}

fn build_bloom_specs(
    content_rect: HudRect,
    scroll_offset: f32,
    hovered_terminal: Option<TerminalId>,
    terminal_manager: &TerminalManager,
    agent_directory: &AgentDirectory,
) -> Vec<BloomSourceSpec> {
    let Some(active_id) = terminal_manager.active_id() else {
        return Vec::new();
    };

    let mut specs = Vec::new();
    for row in agent_rows(
        content_rect,
        scroll_offset,
        hovered_terminal,
        terminal_manager,
        agent_directory,
    ) {
        if row.terminal_id != active_id
            || row.rect.y + row.rect.h < content_rect.y
            || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }

        let kind = AgentListBloomSourceKind::Accent;
        let rect = agent_row_rect(row.rect, AgentListRowSection::Accent);
        specs.push(BloomSourceSpec {
            key: AgentListBloomSourceSprite {
                terminal_id: row.terminal_id,
                kind,
            },
            rect,
            color: bloom_source_color(row.focused, row.hovered, kind),
        });
    }
    specs
}

#[derive(SystemParam)]
pub(crate) struct HudWidgetBloomSetupContext<'w, 's> {
    commands: Commands<'w, 's>,
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    blur_materials: ResMut<'w, Assets<AgentListBloomBlurMaterial>>,
    bloom: ResMut<'w, HudWidgetBloom>,
}

pub(crate) fn setup_hud_widget_bloom(mut ctx: HudWidgetBloomSetupContext) {
    let size = window_size(&ctx.primary_window);
    let source_image = ctx.images.add(bloom_target_image(size));
    let blur_h_image = ctx.images.add(bloom_target_image(size));
    let blur_v_image = ctx.images.add(bloom_target_image(size));

    let source_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -100,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(source_image.clone().into()),
            RenderLayers::layer(BLOOM_SOURCE_LAYER),
            AgentListBloomCameraMarker,
        ))
        .id();

    let blur_h_material = ctx.blur_materials.add(AgentListBloomBlurMaterial {
        image: source_image.clone(),
        uniform: blur_uniform(Vec2::new(1.0 / size.x.max(1) as f32, 0.0)),
    });
    let blur_h_quad = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(blur_h_material.clone()),
            fullscreen_transform(&ctx.primary_window, 0.0),
            RenderLayers::layer(BLOOM_BLUR_H_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurHorizontalQuadMarker,
        ))
        .id();
    let blur_h_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -99,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_h_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_H_LAYER),
            AgentListBloomBlurHorizontalCameraMarker,
        ))
        .id();

    let blur_v_material = ctx.blur_materials.add(AgentListBloomBlurMaterial {
        image: blur_h_image.clone(),
        uniform: blur_uniform(Vec2::new(0.0, 1.0 / size.y.max(1) as f32)),
    });
    let blur_v_quad = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(blur_v_material.clone()),
            fullscreen_transform(&ctx.primary_window, 0.0),
            RenderLayers::layer(BLOOM_BLUR_V_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurVerticalQuadMarker,
        ))
        .id();
    let blur_v_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -98,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_v_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_V_LAYER),
            AgentListBloomBlurVerticalCameraMarker,
        ))
        .id();

    let composite_sprite = ctx
        .commands
        .spawn((
            Sprite {
                image: blur_v_image.clone(),
                color: Color::linear_rgba(
                    BLOOM_COMPOSITE_GAIN,
                    BLOOM_COMPOSITE_GAIN,
                    BLOOM_COMPOSITE_GAIN,
                    1.0,
                ),
                custom_size: Some(Vec2::new(
                    ctx.primary_window.width(),
                    ctx.primary_window.height(),
                )),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, BLOOM_COMPOSITE_Z),
            RenderLayers::layer(0),
            Visibility::Hidden,
            AgentListBloomCompositeMarker,
        ))
        .id();

    ctx.bloom.agent_list = AgentListBloomPass {
        source_image,
        blur_h_image,
        blur_v_image,
        source_camera: Some(source_camera),
        blur_h_camera: Some(blur_h_camera),
        blur_v_camera: Some(blur_v_camera),
        blur_h_quad: Some(blur_h_quad),
        blur_v_quad: Some(blur_v_quad),
        composite_sprite: Some(composite_sprite),
    };
}

type BloomSourceCameraFilter = (
    With<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurHorizontalCameraMarker>,
    Without<AgentListBloomBlurVerticalCameraMarker>,
);
type BloomBlurHorizontalCameraFilter = (
    With<AgentListBloomBlurHorizontalCameraMarker>,
    Without<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurVerticalCameraMarker>,
);
type BloomBlurVerticalCameraFilter = (
    With<AgentListBloomBlurVerticalCameraMarker>,
    Without<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurHorizontalCameraMarker>,
);
type BloomCompositeFilter = (
    With<AgentListBloomCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurHorizontalQuadMarker>,
    Without<AgentListBloomBlurVerticalQuadMarker>,
);
type BloomSourceSpriteFilter = (
    With<AgentListBloomSourceSprite>,
    Without<AgentListBloomCompositeMarker>,
);
type BloomBlurHorizontalQuadFilter = (
    With<AgentListBloomBlurHorizontalQuadMarker>,
    Without<AgentListBloomBlurVerticalQuadMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
);
type BloomBlurVerticalQuadFilter = (
    With<AgentListBloomBlurVerticalQuadMarker>,
    Without<AgentListBloomBlurHorizontalQuadMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
);

#[derive(SystemParam)]
pub(crate) struct HudWidgetBloomContext<'w, 's> {
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    hud_state: Res<'w, HudState>,
    terminal_manager: Res<'w, TerminalManager>,
    agent_directory: Res<'w, AgentDirectory>,
    settings: Res<'w, HudBloomSettings>,
    commands: Commands<'w, 's>,
    bloom: ResMut<'w, HudWidgetBloom>,
    images: ResMut<'w, Assets<Image>>,
    blur_materials: ResMut<'w, Assets<AgentListBloomBlurMaterial>>,
    source_cameras: Query<'w, 's, &'static mut RenderTarget, BloomSourceCameraFilter>,
    blur_h_cameras: Query<'w, 's, &'static mut RenderTarget, BloomBlurHorizontalCameraFilter>,
    blur_v_cameras: Query<'w, 's, &'static mut RenderTarget, BloomBlurVerticalCameraFilter>,
    composites: Query<
        'w,
        's,
        (
            &'static mut Sprite,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomCompositeFilter,
    >,
    blur_h_quads: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomBlurMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomBlurHorizontalQuadFilter,
    >,
    blur_v_quads: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomBlurMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomBlurVerticalQuadFilter,
    >,
    source_sprites: Query<
        'w,
        's,
        (
            Entity,
            &'static AgentListBloomSourceSprite,
            &'static mut Sprite,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomSourceSpriteFilter,
    >,
}

pub(crate) fn sync_hud_widget_bloom(mut ctx: HudWidgetBloomContext) {
    let size = window_size(&ctx.primary_window);
    let pass = &mut ctx.bloom.agent_list;

    if !image_matches_size(&ctx.images, &pass.source_image, size) {
        pass.source_image = ctx.images.add(bloom_target_image(size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_h_image, size) {
        pass.blur_h_image = ctx.images.add(bloom_target_image(size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_v_image, size) {
        pass.blur_v_image = ctx.images.add(bloom_target_image(size));
    }

    if let Some(camera) = pass.source_camera {
        if let Ok(mut target) = ctx.source_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.source_image.clone().into());
        }
    }
    if let Some(camera) = pass.blur_h_camera {
        if let Ok(mut target) = ctx.blur_h_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.blur_h_image.clone().into());
        }
    }
    if let Some(camera) = pass.blur_v_camera {
        if let Ok(mut target) = ctx.blur_v_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.blur_v_image.clone().into());
        }
    }

    if let Some(quad) = pass.blur_h_quad {
        if let Ok((material_handle, mut transform, mut visibility)) = ctx.blur_h_quads.get_mut(quad)
        {
            transform.translation = Vec3::ZERO;
            transform.rotation = Quat::IDENTITY;
            transform.scale =
                Vec3::new(ctx.primary_window.width(), ctx.primary_window.height(), 1.0);
            *visibility = Visibility::Hidden;
            if let Some(material) = ctx.blur_materials.get_mut(material_handle.id()) {
                material.image = pass.source_image.clone();
                material.uniform = blur_uniform(Vec2::new(1.0 / size.x.max(1) as f32, 0.0));
            }
        }
    }
    if let Some(quad) = pass.blur_v_quad {
        if let Ok((material_handle, mut transform, mut visibility)) = ctx.blur_v_quads.get_mut(quad)
        {
            transform.translation = Vec3::ZERO;
            transform.rotation = Quat::IDENTITY;
            transform.scale =
                Vec3::new(ctx.primary_window.width(), ctx.primary_window.height(), 1.0);
            *visibility = Visibility::Hidden;
            if let Some(material) = ctx.blur_materials.get_mut(material_handle.id()) {
                material.image = pass.blur_h_image.clone();
                material.uniform = blur_uniform(Vec2::new(0.0, 1.0 / size.y.max(1) as f32));
            }
        }
    }

    let enabled = ctx
        .hud_state
        .get(HudModuleId::AgentList)
        .map(|module| module.shell.enabled && module.shell.current_alpha > 0.01)
        .unwrap_or(false);
    let specs = if enabled {
        let module = ctx
            .hud_state
            .get(HudModuleId::AgentList)
            .expect("agent list exists when enabled");
        let crate::hud::HudModuleModel::AgentList(state) = &module.model else {
            unreachable!("agent list module must have agent-list model")
        };
        build_bloom_specs(
            module.shell.current_rect,
            state.scroll_offset,
            state.hovered_terminal,
            &ctx.terminal_manager,
            &ctx.agent_directory,
        )
    } else {
        Vec::new()
    };

    let mut existing = std::collections::HashMap::new();
    for (entity, marker, sprite, transform, visibility) in &mut ctx.source_sprites {
        existing.insert(*marker, (entity, sprite.clone(), *transform, *visibility));
    }
    let existing_keys = existing
        .keys()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let desired_keys = specs
        .iter()
        .map(|spec| spec.key)
        .collect::<std::collections::HashSet<_>>();

    for key in existing_keys.difference(&desired_keys) {
        if let Some((entity, _, _, _)) = existing.get(key) {
            ctx.commands.entity(*entity).despawn();
        }
    }

    for spec in &specs {
        if let Some((entity, _, _, _)) = existing.get(&spec.key) {
            if let Ok((_, _, mut sprite, mut transform, mut visibility)) =
                ctx.source_sprites.get_mut(*entity)
            {
                sprite.color = spec.color;
                sprite.custom_size = Some(Vec2::new(spec.rect.w, spec.rect.h));
                transform.translation =
                    rect_transform(&ctx.primary_window, spec.rect, 0.0).translation;
                *visibility = Visibility::Visible;
            }
        } else {
            ctx.commands.spawn((
                Sprite {
                    color: spec.color,
                    custom_size: Some(Vec2::new(spec.rect.w, spec.rect.h)),
                    ..default()
                },
                rect_transform(&ctx.primary_window, spec.rect, 0.0),
                RenderLayers::layer(BLOOM_SOURCE_LAYER),
                Visibility::Visible,
                spec.key,
            ));
        }
    }

    let active = !specs.is_empty();
    if let Some(quad) = pass.blur_h_quad {
        if let Ok((_, _, mut visibility)) = ctx.blur_h_quads.get_mut(quad) {
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(quad) = pass.blur_v_quad {
        if let Ok((_, _, mut visibility)) = ctx.blur_v_quads.get_mut(quad) {
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(composite) = pass.composite_sprite {
        if let Ok((mut sprite, mut transform, mut visibility)) = ctx.composites.get_mut(composite) {
            sprite.image = pass.blur_v_image.clone();
            sprite.color = Color::linear_rgba(
                BLOOM_COMPOSITE_GAIN * ctx.settings.agent_list_intensity,
                BLOOM_COMPOSITE_GAIN * ctx.settings.agent_list_intensity,
                BLOOM_COMPOSITE_GAIN * ctx.settings.agent_list_intensity,
                1.0,
            );
            sprite.custom_size = Some(Vec2::new(
                ctx.primary_window.width(),
                ctx.primary_window.height(),
            ));
            transform.translation = Vec3::new(0.0, 0.0, BLOOM_COMPOSITE_Z);
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::ONE;
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

#[cfg(test)]
pub(crate) fn agent_list_bloom_layer() -> usize {
    BLOOM_SOURCE_LAYER
}

#[cfg(test)]
pub(crate) fn agent_list_bloom_z() -> f32 {
    BLOOM_COMPOSITE_Z
}
