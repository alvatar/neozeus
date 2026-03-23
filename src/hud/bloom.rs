use crate::hud::{
    modules::{agent_row_rect, agent_rows, AgentListRowSection},
    AgentDirectory, HudModuleId, HudOffscreenCompositor, HudRect, HudState,
};
use crate::terminals::{TerminalId, TerminalManager};
use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, ClearColorConfig, RenderTarget},
    ecs::system::SystemParam,
    image::ImageSampler,
    mesh::{Mesh, Mesh2d, MeshVertexBufferLayoutRef},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Extent3d,
        RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension,
        TextureFormat, TextureUsages,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, MeshMaterial2d},
    window::PrimaryWindow,
};

use super::{compositor::HUD_COMPOSITE_FOREGROUND_Z, modules::agent_button_irregularities};
use std::env;

const BLOOM_SOURCE_LAYER: usize = 29;
const BLOOM_BLUR_LAYER: usize = 30;
const BLOOM_SHADER_PATH: &str = "shaders/hud_agent_list_bloom.wgsl";
const BLOOM_Z: f32 = HUD_COMPOSITE_FOREGROUND_Z - 0.1;
const DEFAULT_BLOOM_INTENSITY: f32 = 3.0;
const BLOOM_TEXEL_RADIUS: f32 = 1.0;
const BLOOM_STROKE_THICKNESS: f32 = 1.6;

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
pub(crate) struct AgentListBloomSourceCameraMarker;

#[derive(Component)]
pub(crate) struct AgentListBloomBlurCameraMarker;

#[derive(Component)]
pub(crate) struct AgentListBloomBlurQuadMarker;

#[derive(Component)]
pub(crate) struct AgentListBloomCompositeMarker;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum BloomRectKind {
    Main,
    Marker,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AgentListBloomSourceSprite {
    terminal_id: TerminalId,
    rect_kind: BloomRectKind,
    segment_index: u8,
}

#[derive(Clone, Debug)]
struct BloomSourceSpec {
    key: AgentListBloomSourceSprite,
    rect: HudRect,
    color: Color,
}

#[derive(Clone, Copy, Debug, ShaderType)]
struct HudBloomParams {
    texel_step: Vec2,
    direction: Vec2,
    intensity: f32,
    _padding: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub(crate) struct HudBloomBlurMaterial {
    #[uniform(0)]
    params: HudBloomParams,
    #[texture(1)]
    #[sampler(2)]
    source_texture: Handle<Image>,
}

impl Material2d for HudBloomBlurMaterial {
    fn fragment_shader() -> ShaderRef {
        BLOOM_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub(crate) struct HudBloomCompositeMaterial {
    #[uniform(0)]
    params: HudBloomParams,
    #[texture(1)]
    #[sampler(2)]
    source_texture: Handle<Image>,
}

impl Material2d for HudBloomCompositeMaterial {
    fn fragment_shader() -> ShaderRef {
        BLOOM_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let Some(fragment) = descriptor.fragment.as_mut() else {
            return Ok(());
        };
        let Some(target) = fragment.targets.first_mut().and_then(Option::as_mut) else {
            return Ok(());
        };
        target.blend = Some(BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        });
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct AgentListBloomPass {
    source_image: Handle<Image>,
    blur_image: Handle<Image>,
    source_camera: Option<Entity>,
    blur_camera: Option<Entity>,
    blur_quad: Option<Entity>,
    composite_quad: Option<Entity>,
    blur_material: Option<Handle<HudBloomBlurMaterial>>,
    composite_material: Option<Handle<HudBloomCompositeMaterial>>,
}

#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct HudWidgetBloom {
    agent_list: AgentListBloomPass,
}

fn peniko_to_color(color: bevy_vello::vello::peniko::Color, alpha: f32) -> Color {
    let rgba = color.to_rgba8();
    let scaled_alpha = ((rgba.a as f32) * alpha.clamp(0.0, 1.0)).round() as u8;
    Color::srgba_u8(rgba.r, rgba.g, rgba.b, scaled_alpha)
}

fn agent_stroke_color(focused: bool) -> bevy_vello::vello::peniko::Color {
    if focused {
        bevy_vello::vello::peniko::Color::from_rgba8(175, 201, 181, 255)
    } else {
        bevy_vello::vello::peniko::Color::from_rgba8(181, 66, 11, 255)
    }
}

fn render_target_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Bgra8UnormSrgb,
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
        })
        .unwrap_or(false)
}

fn bloom_params(size: UVec2, direction: Vec2, intensity: f32) -> HudBloomParams {
    HudBloomParams {
        texel_step: Vec2::new(1.0 / size.x.max(1) as f32, 1.0 / size.y.max(1) as f32),
        direction,
        intensity,
        _padding: 0.0,
    }
}

fn edge_segments(rect: HudRect, thickness: f32) -> [HudRect; 4] {
    let edge = thickness.max(1.0);
    [
        HudRect {
            x: rect.x,
            y: rect.y,
            w: rect.w.max(1.0),
            h: edge,
        },
        HudRect {
            x: rect.x,
            y: rect.y + rect.h - edge,
            w: rect.w.max(1.0),
            h: edge,
        },
        HudRect {
            x: rect.x,
            y: rect.y,
            w: edge,
            h: rect.h.max(1.0),
        },
        HudRect {
            x: rect.x + rect.w - edge,
            y: rect.y,
            w: edge,
            h: rect.h.max(1.0),
        },
    ]
}

fn build_bloom_specs(
    content_rect: HudRect,
    scroll_offset: f32,
    hovered_terminal: Option<TerminalId>,
    terminal_manager: &TerminalManager,
    agent_directory: &AgentDirectory,
) -> Vec<BloomSourceSpec> {
    let mut specs = Vec::new();
    for row in agent_rows(
        content_rect,
        scroll_offset,
        hovered_terminal,
        terminal_manager,
        agent_directory,
    ) {
        if row.rect.y + row.rect.h < content_rect.y || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }

        let stroke = agent_stroke_color(row.focused);
        let edge_color = peniko_to_color(stroke, if row.focused { 0.40 } else { 0.32 });
        for (rect_kind, rect) in [
            (
                BloomRectKind::Main,
                agent_row_rect(row.rect, AgentListRowSection::Main),
            ),
            (
                BloomRectKind::Marker,
                agent_row_rect(row.rect, AgentListRowSection::Marker),
            ),
        ] {
            for (segment_index, edge_rect) in edge_segments(rect, BLOOM_STROKE_THICKNESS)
                .into_iter()
                .enumerate()
            {
                specs.push(BloomSourceSpec {
                    key: AgentListBloomSourceSprite {
                        terminal_id: row.terminal_id,
                        rect_kind,
                        segment_index: segment_index as u8,
                    },
                    rect: edge_rect,
                    color: edge_color,
                });
            }

            let band_seed_offset = match rect_kind {
                BloomRectKind::Main => 0,
                BloomRectKind::Marker => 7,
            };
            for (band_index, (band_rect, alpha)) in
                agent_button_irregularities(rect, row.terminal_id.0 as u32 * 23 + band_seed_offset)
                    .into_iter()
                    .enumerate()
            {
                specs.push(BloomSourceSpec {
                    key: AgentListBloomSourceSprite {
                        terminal_id: row.terminal_id,
                        rect_kind,
                        segment_index: 4 + band_index as u8,
                    },
                    rect: band_rect,
                    color: peniko_to_color(stroke, alpha * if row.focused { 0.46 } else { 0.38 }),
                });
            }
        }
    }
    specs
}

fn window_size(window: &Window) -> UVec2 {
    UVec2::new(
        window.width().round().max(1.0) as u32,
        window.height().round().max(1.0) as u32,
    )
}

#[derive(SystemParam)]
pub(crate) struct HudWidgetBloomSetupContext<'w, 's> {
    commands: Commands<'w, 's>,
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    settings: Res<'w, HudBloomSettings>,
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    blur_materials: ResMut<'w, Assets<HudBloomBlurMaterial>>,
    composite_materials: ResMut<'w, Assets<HudBloomCompositeMaterial>>,
    bloom: ResMut<'w, HudWidgetBloom>,
}

pub(crate) fn setup_hud_widget_bloom(mut ctx: HudWidgetBloomSetupContext) {
    let size = window_size(&ctx.primary_window);
    let source_image = ctx.images.add(render_target_image(size));
    let blur_image = ctx.images.add(render_target_image(size));
    let quad_mesh = ctx.meshes.add(Rectangle::default());
    let blur_material = ctx.blur_materials.add(HudBloomBlurMaterial {
        params: bloom_params(size, Vec2::new(BLOOM_TEXEL_RADIUS, 0.0), 1.0),
        source_texture: source_image.clone(),
    });
    let composite_material = ctx.composite_materials.add(HudBloomCompositeMaterial {
        params: bloom_params(
            size,
            Vec2::new(0.0, BLOOM_TEXEL_RADIUS),
            ctx.settings.agent_list_intensity,
        ),
        source_texture: blur_image.clone(),
    });

    let source_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -110,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(source_image.clone().into()),
            RenderLayers::layer(BLOOM_SOURCE_LAYER),
            AgentListBloomSourceCameraMarker,
        ))
        .id();
    let blur_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -109,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_LAYER),
            AgentListBloomBlurCameraMarker,
        ))
        .id();
    let blur_quad = ctx
        .commands
        .spawn((
            Mesh2d(quad_mesh.clone()),
            MeshMaterial2d(blur_material.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::new(
                ctx.primary_window.width(),
                ctx.primary_window.height(),
                1.0,
            )),
            RenderLayers::layer(BLOOM_BLUR_LAYER),
            AgentListBloomBlurQuadMarker,
        ))
        .id();
    let composite_quad = ctx
        .commands
        .spawn((
            Mesh2d(quad_mesh.clone()),
            MeshMaterial2d(composite_material.clone()),
            Transform::from_xyz(0.0, 0.0, BLOOM_Z).with_scale(Vec3::new(
                ctx.primary_window.width(),
                ctx.primary_window.height(),
                1.0,
            )),
            Visibility::Hidden,
            AgentListBloomCompositeMarker,
        ))
        .id();

    ctx.bloom.agent_list = AgentListBloomPass {
        source_image,
        blur_image,
        source_camera: Some(source_camera),
        blur_camera: Some(blur_camera),
        blur_quad: Some(blur_quad),
        composite_quad: Some(composite_quad),
        blur_material: Some(blur_material),
        composite_material: Some(composite_material),
    };
}

type BloomSourceTargetFilter = (
    With<AgentListBloomSourceCameraMarker>,
    Without<AgentListBloomBlurCameraMarker>,
);
type BloomBlurTargetFilter = (
    With<AgentListBloomBlurCameraMarker>,
    Without<AgentListBloomSourceCameraMarker>,
);
type BloomBlurQuadItem = (
    &'static mut Transform,
    &'static MeshMaterial2d<HudBloomBlurMaterial>,
);
type BloomBlurQuadFilter = (
    With<AgentListBloomBlurQuadMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomCompositeMarker>,
);
type BloomCompositeQuadItem = (
    &'static mut Transform,
    &'static mut Visibility,
    &'static MeshMaterial2d<HudBloomCompositeMaterial>,
);
type BloomCompositeQuadFilter = (
    With<AgentListBloomCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurQuadMarker>,
);
type BloomSourceSpriteItem = (
    Entity,
    &'static AgentListBloomSourceSprite,
    &'static mut Sprite,
    &'static mut Transform,
    &'static mut Visibility,
);
type BloomSourceSpriteFilter = (
    Without<AgentListBloomBlurQuadMarker>,
    Without<AgentListBloomCompositeMarker>,
);

#[derive(SystemParam)]
pub(crate) struct HudWidgetBloomContext<'w, 's> {
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    hud_state: Res<'w, HudState>,
    terminal_manager: Res<'w, TerminalManager>,
    agent_directory: Res<'w, AgentDirectory>,
    _compositor: Res<'w, HudOffscreenCompositor>,
    settings: Res<'w, HudBloomSettings>,
    commands: Commands<'w, 's>,
    bloom: ResMut<'w, HudWidgetBloom>,
    images: ResMut<'w, Assets<Image>>,
    blur_materials: ResMut<'w, Assets<HudBloomBlurMaterial>>,
    composite_materials: ResMut<'w, Assets<HudBloomCompositeMaterial>>,
    source_targets: Query<'w, 's, &'static mut RenderTarget, BloomSourceTargetFilter>,
    blur_targets: Query<'w, 's, &'static mut RenderTarget, BloomBlurTargetFilter>,
    blur_quads: Query<'w, 's, BloomBlurQuadItem, BloomBlurQuadFilter>,
    composite_quads: Query<'w, 's, BloomCompositeQuadItem, BloomCompositeQuadFilter>,
    source_sprites: Query<'w, 's, BloomSourceSpriteItem, BloomSourceSpriteFilter>,
}

pub(crate) fn sync_hud_widget_bloom(mut ctx: HudWidgetBloomContext) {
    let size = window_size(&ctx.primary_window);
    let pass = &mut ctx.bloom.agent_list;

    if !image_matches_size(&ctx.images, &pass.source_image, size) {
        pass.source_image = ctx.images.add(render_target_image(size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_image, size) {
        pass.blur_image = ctx.images.add(render_target_image(size));
    }

    if let Some(entity) = pass.source_camera {
        if let Ok(mut target) = ctx.source_targets.get_mut(entity) {
            *target = RenderTarget::Image(pass.source_image.clone().into());
        }
    }
    if let Some(entity) = pass.blur_camera {
        if let Ok(mut target) = ctx.blur_targets.get_mut(entity) {
            *target = RenderTarget::Image(pass.blur_image.clone().into());
        }
    }
    if let Some(handle) = pass.blur_material.clone() {
        if let Some(material) = ctx.blur_materials.get_mut(&handle) {
            material.source_texture = pass.source_image.clone();
            material.params = bloom_params(size, Vec2::new(BLOOM_TEXEL_RADIUS, 0.0), 1.0);
        }
    }
    if let Some(handle) = pass.composite_material.clone() {
        if let Some(material) = ctx.composite_materials.get_mut(&handle) {
            material.source_texture = pass.blur_image.clone();
            material.params = bloom_params(
                size,
                Vec2::new(0.0, BLOOM_TEXEL_RADIUS),
                ctx.settings.agent_list_intensity,
            );
        }
    }
    if let Some(entity) = pass.blur_quad {
        if let Ok((mut transform, material)) = ctx.blur_quads.get_mut(entity) {
            transform.translation = Vec3::ZERO;
            transform.scale =
                Vec3::new(ctx.primary_window.width(), ctx.primary_window.height(), 1.0);
            if let Some(handle) = pass.blur_material.clone() {
                if material.0 != handle {
                    ctx.commands.entity(entity).insert(MeshMaterial2d(handle));
                }
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
                transform.translation = Vec3::new(
                    spec.rect.x + spec.rect.w * 0.5 - ctx.primary_window.width() * 0.5,
                    ctx.primary_window.height() * 0.5 - (spec.rect.y + spec.rect.h * 0.5),
                    0.0,
                );
                *visibility = Visibility::Visible;
            }
        } else {
            ctx.commands.spawn((
                Sprite {
                    color: spec.color,
                    custom_size: Some(Vec2::new(spec.rect.w, spec.rect.h)),
                    ..default()
                },
                Transform::from_xyz(
                    spec.rect.x + spec.rect.w * 0.5 - ctx.primary_window.width() * 0.5,
                    ctx.primary_window.height() * 0.5 - (spec.rect.y + spec.rect.h * 0.5),
                    0.0,
                ),
                RenderLayers::layer(BLOOM_SOURCE_LAYER),
                spec.key,
            ));
        }
    }

    if let Some(entity) = pass.composite_quad {
        if let Ok((mut transform, mut visibility, material)) = ctx.composite_quads.get_mut(entity) {
            transform.translation = Vec3::new(0.0, 0.0, BLOOM_Z);
            transform.scale =
                Vec3::new(ctx.primary_window.width(), ctx.primary_window.height(), 1.0);
            *visibility = if specs.is_empty() {
                Visibility::Hidden
            } else {
                Visibility::Visible
            };
            if let Some(handle) = pass.composite_material.clone() {
                if material.0 != handle {
                    ctx.commands.entity(entity).insert(MeshMaterial2d(handle));
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn agent_list_bloom_layers() -> (usize, usize) {
    (BLOOM_SOURCE_LAYER, BLOOM_BLUR_LAYER)
}

#[cfg(test)]
pub(crate) fn agent_list_bloom_z() -> f32 {
    BLOOM_Z
}
