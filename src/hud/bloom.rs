use crate::hud::{
    modules::{agent_row_rect, agent_rows, AgentListRowSection},
    AgentDirectory, HudModuleId, HudRect, HudState,
};
use crate::terminals::{TerminalId, TerminalManager};
use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, ClearColorConfig, RenderTarget},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    ecs::system::SystemParam,
    image::ImageSampler,
    post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter},
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
const BLOOM_COMPOSITE_Z: f32 = HUD_COMPOSITE_FOREGROUND_Z + 0.1;
const BLOOM_TARGET_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const BLOOM_COMPOSITE_SHADER_PATH: &str = "shaders/hud_agent_list_bloom_composite.wgsl";
const DEFAULT_BLOOM_INTENSITY: f32 = 1.35;
const BLOOM_COMPOSITE_ALPHA: f32 = 1.0;
const BLOOM_CORE_MASK_INSET_PX: f32 = 1.0;
const BLOOM_CORE_MASK_FEATHER_PX: f32 = 10.0;

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

#[derive(Clone, Copy, Debug, ShaderType)]
pub(crate) struct AgentListBloomCompositeUniform {
    pub(crate) core_center_size: Vec4,
    pub(crate) settings: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct AgentListBloomCompositeMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub(crate) image: Handle<Image>,
    #[uniform(2)]
    pub(crate) uniform: AgentListBloomCompositeUniform,
}

impl Material2d for AgentListBloomCompositeMaterial {
    fn fragment_shader() -> ShaderRef {
        BLOOM_COMPOSITE_SHADER_PATH.into()
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

fn active_accent_rect(specs: &[BloomSourceSpec]) -> Option<HudRect> {
    specs.first().map(|spec| spec.rect)
}

#[derive(Clone, Debug, Default)]
struct AgentListBloomPass {
    image: Handle<Image>,
    camera: Option<Entity>,
    composite_entity: Option<Entity>,
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

fn bloom_composite_uniform(
    window: &Window,
    rect: Option<HudRect>,
) -> AgentListBloomCompositeUniform {
    let Some(rect) = rect else {
        return AgentListBloomCompositeUniform {
            core_center_size: Vec4::ZERO,
            settings: Vec4::new(BLOOM_COMPOSITE_ALPHA, BLOOM_CORE_MASK_INSET_PX, 0.0, 0.0),
        };
    };

    let transform = rect_transform(window, rect, 0.0);
    AgentListBloomCompositeUniform {
        core_center_size: Vec4::new(
            transform.translation.x,
            transform.translation.y,
            (rect.w - BLOOM_CORE_MASK_INSET_PX * 2.0).max(0.0),
            (rect.h - BLOOM_CORE_MASK_INSET_PX * 2.0).max(0.0),
        ),
        settings: Vec4::new(BLOOM_COMPOSITE_ALPHA, BLOOM_CORE_MASK_FEATHER_PX, 0.0, 0.0),
    }
}

fn bloom_source_color(focused: bool, hovered: bool, kind: AgentListBloomSourceKind) -> Color {
    match (focused, hovered, kind) {
        (true, _, AgentListBloomSourceKind::Accent) => Color::linear_rgba(10.0, 0.42, 0.36, 0.92),
        (_, true, AgentListBloomSourceKind::Accent) => Color::linear_rgba(5.5, 0.24, 0.20, 0.32),
        (_, _, AgentListBloomSourceKind::Accent) => Color::linear_rgba(2.5, 0.16, 0.14, 0.10),
    }
}

fn bloom_component(intensity: f32) -> Bloom {
    Bloom {
        intensity,
        low_frequency_boost: 0.72,
        low_frequency_boost_curvature: 0.78,
        high_pass_frequency: 0.84,
        prefilter: BloomPrefilter {
            threshold: 0.0,
            threshold_softness: 0.0,
        },
        composite_mode: BloomCompositeMode::Additive,
        max_mip_dimension: 1024,
        scale: Vec2::ONE,
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
    settings: Res<'w, HudBloomSettings>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    composite_materials: ResMut<'w, Assets<AgentListBloomCompositeMaterial>>,
    bloom: ResMut<'w, HudWidgetBloom>,
}

pub(crate) fn setup_hud_widget_bloom(mut ctx: HudWidgetBloomSetupContext) {
    let size = window_size(&ctx.primary_window);
    let image = ctx.images.add(bloom_target_image(size));
    let camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -100,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            RenderLayers::layer(BLOOM_SOURCE_LAYER),
            bloom_component(ctx.settings.agent_list_intensity),
            Tonemapping::AgX,
            DebandDither::Enabled,
            AgentListBloomCameraMarker,
        ))
        .id();
    let composite_material = ctx
        .composite_materials
        .add(AgentListBloomCompositeMaterial {
            image: image.clone(),
            uniform: bloom_composite_uniform(&ctx.primary_window, None),
        });
    let composite_entity = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(composite_material.clone()),
            Transform::from_xyz(0.0, 0.0, BLOOM_COMPOSITE_Z).with_scale(Vec3::new(
                ctx.primary_window.width(),
                ctx.primary_window.height(),
                1.0,
            )),
            RenderLayers::layer(0),
            Visibility::Hidden,
            AgentListBloomCompositeMarker,
        ))
        .id();

    ctx.bloom.agent_list = AgentListBloomPass {
        image,
        camera: Some(camera),
        composite_entity: Some(composite_entity),
    };
}

type BloomCameraFilter = With<AgentListBloomCameraMarker>;
type BloomCompositeFilter = (
    With<AgentListBloomCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomCameraMarker>,
);
type BloomSourceSpriteFilter = (
    With<AgentListBloomSourceSprite>,
    Without<AgentListBloomCompositeMarker>,
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
    cameras: Query<'w, 's, (&'static mut RenderTarget, &'static mut Bloom), BloomCameraFilter>,
    composite_materials: ResMut<'w, Assets<AgentListBloomCompositeMaterial>>,
    composites: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomCompositeMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomCompositeFilter,
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

    if !image_matches_size(&ctx.images, &pass.image, size) {
        pass.image = ctx.images.add(bloom_target_image(size));
    }

    if let Some(camera) = pass.camera {
        if let Ok((mut target, mut bloom)) = ctx.cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.image.clone().into());
            *bloom = bloom_component(ctx.settings.agent_list_intensity);
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

    if let Some(composite) = pass.composite_entity {
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.composites.get_mut(composite)
        {
            transform.translation = Vec3::new(0.0, 0.0, BLOOM_COMPOSITE_Z);
            transform.rotation = Quat::IDENTITY;
            transform.scale =
                Vec3::new(ctx.primary_window.width(), ctx.primary_window.height(), 1.0);
            *visibility = if specs.is_empty() {
                Visibility::Hidden
            } else {
                Visibility::Visible
            };

            if let Some(material) = ctx.composite_materials.get_mut(material_handle.id()) {
                material.image = pass.image.clone();
                material.uniform =
                    bloom_composite_uniform(&ctx.primary_window, active_accent_rect(&specs));
            }
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
