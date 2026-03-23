use crate::hud::{
    modules::{
        agent_row_rect, agent_rows, AgentListRowSection, AGENT_LIST_ACCENT_RED_B,
        AGENT_LIST_ACCENT_RED_G, AGENT_LIST_ACCENT_RED_R,
    },
    AgentDirectory, HudModuleId, HudRect, HudState,
};
use crate::terminals::{TerminalId, TerminalManager};
use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, CameraOutputMode, ClearColorConfig, RenderTarget},
    color::{LinearRgba, Srgba},
    ecs::system::SystemParam,
    image::ImageSampler,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Extent3d, ShaderType,
        TextureDimension, TextureFormat, TextureUsages,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, MeshMaterial2d},
    window::PrimaryWindow,
};
use std::env;

use super::compositor::HUD_COMPOSITE_FOREGROUND_Z;

const BLOOM_SOURCE_LAYER: usize = 29;
const BLOOM_BLUR_SMALL_H_LAYER: usize = 30;
const BLOOM_BLUR_SMALL_V_LAYER: usize = 31;
const BLOOM_BLUR_WIDE_H_LAYER: usize = 32;
const BLOOM_BLUR_WIDE_V_LAYER: usize = 33;
const BLOOM_COMPOSITE_LAYER: usize = 34;
const BLOOM_COMPOSITE_Z: f32 = HUD_COMPOSITE_FOREGROUND_Z + 0.1;
const BLOOM_TARGET_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const BLOOM_BLUR_SHADER_PATH: &str = "shaders/hud_agent_list_bloom_blur.wgsl";
const BLOOM_SCALE_DIVISOR: u32 = 1;
const DEFAULT_BLOOM_INTENSITY: f32 = 0.10;
const BLOOM_INTENSITY_SCALE: f32 = 14.0;
const SMALL_BLUR_GAIN: f32 = 2.0;
const WIDE_BLUR_GAIN: f32 = 1.5;
const SMALL_BLUR_STEP_SCALE: f32 = 6.0;
const WIDE_BLUR_STEP_SCALE: f32 = 18.0;
const BLOOM_DEBUG_PREVIEW_Z: f32 = HUD_COMPOSITE_FOREGROUND_Z + 2.0;
const BLOOM_DEBUG_PREVIEW_WIDTH: f32 = 160.0;
const BLOOM_DEBUG_PREVIEW_HEIGHT: f32 = 120.0;
const BLOOM_DEBUG_PREVIEW_MARGIN: f32 = 16.0;
const BLOOM_DEBUG_PREVIEW_GAP: f32 = 12.0;
const BLOOM_DEBUG_PREVIEW_BACKDROP_PADDING: f32 = 10.0;

#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct HudBloomSettings {
    pub(crate) agent_list_intensity: f32,
    pub(crate) debug_previews: bool,
}

impl Default for HudBloomSettings {
    fn default() -> Self {
        Self {
            agent_list_intensity: resolve_agent_list_bloom_intensity(
                env::var("NEOZEUS_AGENT_BLOOM_INTENSITY").ok().as_deref(),
            ),
            debug_previews: resolve_agent_list_bloom_debug_previews(
                env::var("NEOZEUS_AGENT_BLOOM_DEBUG_PREVIEWS")
                    .ok()
                    .as_deref(),
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

pub(crate) fn resolve_agent_list_bloom_debug_previews(raw: Option<&str>) -> bool {
    matches!(
        raw.map(str::trim).filter(|value| !value.is_empty()),
        Some(value)
            if value.eq_ignore_ascii_case("1")
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("yes")
                || value.eq_ignore_ascii_case("on")
    )
}

#[derive(Component)]
pub(crate) struct AgentListBloomCameraMarker;

#[derive(Component)]
pub(crate) struct AgentListBloomCompositeMarker;

#[derive(Component)]
struct AgentListBloomWideCompositeMarker;

#[derive(Component)]
struct AgentListBloomAdditiveCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurSmallHorizontalCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurSmallVerticalCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurWideHorizontalCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurWideVerticalCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurSmallHorizontalQuadMarker;

#[derive(Component)]
struct AgentListBloomBlurSmallVerticalQuadMarker;

#[derive(Component)]
struct AgentListBloomBlurWideHorizontalQuadMarker;

#[derive(Component)]
struct AgentListBloomBlurWideVerticalQuadMarker;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum AgentListBloomDebugPreviewStage {
    Source,
    SmallBlur,
    WideBlur,
}

#[derive(Component)]
struct AgentListBloomDebugPreviewMarker {
    stage: AgentListBloomDebugPreviewStage,
}

#[derive(Component)]
struct AgentListBloomDebugBackdropMarker;

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
        AlphaMode2d::Opaque
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
    blur_small_h_image: Handle<Image>,
    blur_small_v_image: Handle<Image>,
    blur_wide_h_image: Handle<Image>,
    blur_wide_v_image: Handle<Image>,
    source_camera: Option<Entity>,
    blur_small_h_camera: Option<Entity>,
    blur_small_v_camera: Option<Entity>,
    blur_wide_h_camera: Option<Entity>,
    blur_wide_v_camera: Option<Entity>,
    blur_small_h_quad: Option<Entity>,
    blur_small_v_quad: Option<Entity>,
    blur_wide_h_quad: Option<Entity>,
    blur_wide_v_quad: Option<Entity>,
    composite_sprite: Option<Entity>,
    wide_composite_sprite: Option<Entity>,
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

fn logical_window_size(window: &Window) -> UVec2 {
    UVec2::new(
        window.width().round().max(1.0) as u32,
        window.height().round().max(1.0) as u32,
    )
}

fn bloom_target_size(window: &Window) -> UVec2 {
    let size = logical_window_size(window);
    UVec2::new(
        (size.x / BLOOM_SCALE_DIVISOR).max(1),
        (size.y / BLOOM_SCALE_DIVISOR).max(1),
    )
}

fn rect_transform_for_frame(frame_size: Vec2, rect: HudRect, z: f32) -> Transform {
    Transform::from_xyz(
        rect.x + rect.w * 0.5 - frame_size.x * 0.5,
        frame_size.y * 0.5 - (rect.y + rect.h * 0.5),
        z,
    )
}

fn fullscreen_transform_for_frame(frame_size: Vec2, z: f32) -> Transform {
    Transform::from_xyz(0.0, 0.0, z).with_scale(Vec3::new(frame_size.x, frame_size.y, 1.0))
}

fn scale_rect_into_target(window: &Window, target_size: UVec2, rect: HudRect) -> HudRect {
    let scale_x = target_size.x.max(1) as f32 / window.width().max(1.0);
    let scale_y = target_size.y.max(1) as f32 / window.height().max(1.0);
    HudRect {
        x: rect.x * scale_x,
        y: rect.y * scale_y,
        w: rect.w * scale_x,
        h: rect.h * scale_y,
    }
}

fn bloom_debug_preview_transform(
    window: &Window,
    stage: AgentListBloomDebugPreviewStage,
    z: f32,
) -> Transform {
    let index = match stage {
        AgentListBloomDebugPreviewStage::Source => 0.0,
        AgentListBloomDebugPreviewStage::SmallBlur => 1.0,
        AgentListBloomDebugPreviewStage::WideBlur => 2.0,
    };
    let total_width = BLOOM_DEBUG_PREVIEW_WIDTH * 3.0 + BLOOM_DEBUG_PREVIEW_GAP * 2.0;
    let left = window.width() * 0.5 - BLOOM_DEBUG_PREVIEW_MARGIN - total_width;
    let x = left
        + index * (BLOOM_DEBUG_PREVIEW_WIDTH + BLOOM_DEBUG_PREVIEW_GAP)
        + BLOOM_DEBUG_PREVIEW_WIDTH * 0.5;
    let y = window.height() * 0.5 - BLOOM_DEBUG_PREVIEW_MARGIN - BLOOM_DEBUG_PREVIEW_HEIGHT * 0.5;
    Transform::from_xyz(x, y, z)
}

fn bloom_debug_backdrop_transform(window: &Window, z: f32) -> Transform {
    let total_width = BLOOM_DEBUG_PREVIEW_WIDTH * 3.0 + BLOOM_DEBUG_PREVIEW_GAP * 2.0;
    let panel_width = total_width + BLOOM_DEBUG_PREVIEW_BACKDROP_PADDING * 2.0;
    let panel_height = BLOOM_DEBUG_PREVIEW_HEIGHT + BLOOM_DEBUG_PREVIEW_BACKDROP_PADDING * 2.0;
    let x = window.width() * 0.5 - BLOOM_DEBUG_PREVIEW_MARGIN - panel_width * 0.5;
    let y = window.height() * 0.5 - BLOOM_DEBUG_PREVIEW_MARGIN - panel_height * 0.5;
    Transform::from_xyz(x, y, z).with_scale(Vec3::new(panel_width, panel_height, 1.0))
}

fn blur_uniform(texel_step: Vec2, gain: f32) -> AgentListBloomBlurUniform {
    AgentListBloomBlurUniform {
        texel_step_gain: Vec4::new(texel_step.x, texel_step.y, gain, 0.0),
    }
}

fn bloom_reference_red(scale: f32, alpha: f32) -> Color {
    let linear: LinearRgba = Srgba::rgba_u8(
        AGENT_LIST_ACCENT_RED_R,
        AGENT_LIST_ACCENT_RED_G,
        AGENT_LIST_ACCENT_RED_B,
        255,
    )
    .into();
    Color::linear_rgba(
        linear.red * scale,
        linear.green * scale,
        linear.blue * scale,
        alpha,
    )
}

fn bloom_source_color(focused: bool, hovered: bool, kind: AgentListBloomSourceKind) -> Color {
    match (focused, hovered, kind) {
        (true, _, AgentListBloomSourceKind::Accent) => bloom_reference_red(14.0, 1.0),
        (_, true, AgentListBloomSourceKind::Accent) => bloom_reference_red(6.0, 0.8),
        (_, _, AgentListBloomSourceKind::Accent) => bloom_reference_red(2.5, 0.35),
    }
}

fn additive_blend_state() -> BlendState {
    BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            src_factor: BlendFactor::Zero,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
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
    blur_materials: ResMut<'w, Assets<AgentListBloomBlurMaterial>>,
    bloom: ResMut<'w, HudWidgetBloom>,
}

pub(crate) fn setup_hud_widget_bloom(mut ctx: HudWidgetBloomSetupContext) {
    let target_size = bloom_target_size(&ctx.primary_window);
    let source_image = ctx.images.add(bloom_target_image(target_size));
    let blur_small_h_image = ctx.images.add(bloom_target_image(target_size));
    let blur_small_v_image = ctx.images.add(bloom_target_image(target_size));
    let blur_wide_h_image = ctx.images.add(bloom_target_image(target_size));
    let blur_wide_v_image = ctx.images.add(bloom_target_image(target_size));

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

    let small_h_material = ctx.blur_materials.add(AgentListBloomBlurMaterial {
        image: source_image.clone(),
        uniform: blur_uniform(
            Vec2::new(SMALL_BLUR_STEP_SCALE / target_size.x.max(1) as f32, 0.0),
            1.0,
        ),
    });
    let blur_small_h_quad = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(small_h_material),
            fullscreen_transform_for_frame(target_size.as_vec2(), 0.0),
            RenderLayers::layer(BLOOM_BLUR_SMALL_H_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurSmallHorizontalQuadMarker,
        ))
        .id();
    let blur_small_h_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -99,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_small_h_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_SMALL_H_LAYER),
            AgentListBloomBlurSmallHorizontalCameraMarker,
        ))
        .id();

    let small_v_material = ctx.blur_materials.add(AgentListBloomBlurMaterial {
        image: blur_small_h_image.clone(),
        uniform: blur_uniform(
            Vec2::new(0.0, SMALL_BLUR_STEP_SCALE / target_size.y.max(1) as f32),
            1.0,
        ),
    });
    let blur_small_v_quad = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(small_v_material),
            fullscreen_transform_for_frame(target_size.as_vec2(), 0.0),
            RenderLayers::layer(BLOOM_BLUR_SMALL_V_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurSmallVerticalQuadMarker,
        ))
        .id();
    let blur_small_v_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -98,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_small_v_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_SMALL_V_LAYER),
            AgentListBloomBlurSmallVerticalCameraMarker,
        ))
        .id();

    let wide_h_material = ctx.blur_materials.add(AgentListBloomBlurMaterial {
        image: blur_small_v_image.clone(),
        uniform: blur_uniform(
            Vec2::new(WIDE_BLUR_STEP_SCALE / target_size.x.max(1) as f32, 0.0),
            1.0,
        ),
    });
    let blur_wide_h_quad = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(wide_h_material),
            fullscreen_transform_for_frame(target_size.as_vec2(), 0.0),
            RenderLayers::layer(BLOOM_BLUR_WIDE_H_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurWideHorizontalQuadMarker,
        ))
        .id();
    let blur_wide_h_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -97,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_wide_h_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_WIDE_H_LAYER),
            AgentListBloomBlurWideHorizontalCameraMarker,
        ))
        .id();

    let wide_v_material = ctx.blur_materials.add(AgentListBloomBlurMaterial {
        image: blur_wide_h_image.clone(),
        uniform: blur_uniform(
            Vec2::new(0.0, WIDE_BLUR_STEP_SCALE / target_size.y.max(1) as f32),
            1.0,
        ),
    });
    let blur_wide_v_quad = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(wide_v_material),
            fullscreen_transform_for_frame(target_size.as_vec2(), 0.0),
            RenderLayers::layer(BLOOM_BLUR_WIDE_V_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurWideVerticalQuadMarker,
        ))
        .id();
    let blur_wide_v_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -96,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_wide_v_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_WIDE_V_LAYER),
            AgentListBloomBlurWideVerticalCameraMarker,
        ))
        .id();

    let composite_sprite = ctx
        .commands
        .spawn((
            Sprite {
                image: blur_small_v_image.clone(),
                color: Color::linear_rgba(SMALL_BLUR_GAIN, SMALL_BLUR_GAIN, SMALL_BLUR_GAIN, 1.0),
                custom_size: Some(Vec2::new(
                    ctx.primary_window.width(),
                    ctx.primary_window.height(),
                )),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, BLOOM_COMPOSITE_Z),
            RenderLayers::layer(BLOOM_COMPOSITE_LAYER),
            Visibility::Hidden,
            AgentListBloomCompositeMarker,
        ))
        .id();

    let wide_composite_sprite = ctx
        .commands
        .spawn((
            Sprite {
                image: blur_wide_v_image.clone(),
                color: Color::linear_rgba(WIDE_BLUR_GAIN, WIDE_BLUR_GAIN, WIDE_BLUR_GAIN, 1.0),
                custom_size: Some(Vec2::new(
                    ctx.primary_window.width(),
                    ctx.primary_window.height(),
                )),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, BLOOM_COMPOSITE_Z + 0.01),
            RenderLayers::layer(BLOOM_COMPOSITE_LAYER),
            Visibility::Hidden,
            AgentListBloomWideCompositeMarker,
        ))
        .id();

    ctx.commands.spawn((
        Camera2d,
        Camera {
            order: 100,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(additive_blend_state()),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
        RenderLayers::layer(BLOOM_COMPOSITE_LAYER),
        AgentListBloomAdditiveCameraMarker,
    ));

    let debug_backdrop = ctx
        .commands
        .spawn((
            Sprite {
                color: Color::srgba(0.0, 0.0, 0.0, 0.92),
                custom_size: Some(Vec2::new(1.0, 1.0)),
                ..default()
            },
            bloom_debug_backdrop_transform(&ctx.primary_window, BLOOM_DEBUG_PREVIEW_Z),
            RenderLayers::layer(0),
            Visibility::Hidden,
            AgentListBloomDebugBackdropMarker,
        ))
        .id();
    let debug_source_preview = ctx
        .commands
        .spawn((
            Sprite {
                image: source_image.clone(),
                color: Color::WHITE,
                custom_size: Some(Vec2::new(
                    BLOOM_DEBUG_PREVIEW_WIDTH,
                    BLOOM_DEBUG_PREVIEW_HEIGHT,
                )),
                ..default()
            },
            bloom_debug_preview_transform(
                &ctx.primary_window,
                AgentListBloomDebugPreviewStage::Source,
                BLOOM_DEBUG_PREVIEW_Z + 0.01,
            ),
            RenderLayers::layer(0),
            Visibility::Hidden,
            AgentListBloomDebugPreviewMarker {
                stage: AgentListBloomDebugPreviewStage::Source,
            },
        ))
        .id();
    let debug_small_preview = ctx
        .commands
        .spawn((
            Sprite {
                image: blur_small_v_image.clone(),
                color: Color::WHITE,
                custom_size: Some(Vec2::new(
                    BLOOM_DEBUG_PREVIEW_WIDTH,
                    BLOOM_DEBUG_PREVIEW_HEIGHT,
                )),
                ..default()
            },
            bloom_debug_preview_transform(
                &ctx.primary_window,
                AgentListBloomDebugPreviewStage::SmallBlur,
                BLOOM_DEBUG_PREVIEW_Z + 0.01,
            ),
            RenderLayers::layer(0),
            Visibility::Hidden,
            AgentListBloomDebugPreviewMarker {
                stage: AgentListBloomDebugPreviewStage::SmallBlur,
            },
        ))
        .id();
    let debug_wide_preview = ctx
        .commands
        .spawn((
            Sprite {
                image: blur_wide_v_image.clone(),
                color: Color::WHITE,
                custom_size: Some(Vec2::new(
                    BLOOM_DEBUG_PREVIEW_WIDTH,
                    BLOOM_DEBUG_PREVIEW_HEIGHT,
                )),
                ..default()
            },
            bloom_debug_preview_transform(
                &ctx.primary_window,
                AgentListBloomDebugPreviewStage::WideBlur,
                BLOOM_DEBUG_PREVIEW_Z + 0.01,
            ),
            RenderLayers::layer(0),
            Visibility::Hidden,
            AgentListBloomDebugPreviewMarker {
                stage: AgentListBloomDebugPreviewStage::WideBlur,
            },
        ))
        .id();

    if ctx.settings.debug_previews {
        ctx.commands
            .entity(debug_backdrop)
            .insert(Visibility::Visible);
        ctx.commands
            .entity(debug_source_preview)
            .insert(Visibility::Visible);
        ctx.commands
            .entity(debug_small_preview)
            .insert(Visibility::Visible);
        ctx.commands
            .entity(debug_wide_preview)
            .insert(Visibility::Visible);
    }

    ctx.bloom.agent_list = AgentListBloomPass {
        source_image,
        blur_small_h_image,
        blur_small_v_image,
        blur_wide_h_image,
        blur_wide_v_image,
        source_camera: Some(source_camera),
        blur_small_h_camera: Some(blur_small_h_camera),
        blur_small_v_camera: Some(blur_small_v_camera),
        blur_wide_h_camera: Some(blur_wide_h_camera),
        blur_wide_v_camera: Some(blur_wide_v_camera),
        blur_small_h_quad: Some(blur_small_h_quad),
        blur_small_v_quad: Some(blur_small_v_quad),
        blur_wide_h_quad: Some(blur_wide_h_quad),
        blur_wide_v_quad: Some(blur_wide_v_quad),
        composite_sprite: Some(composite_sprite),
        wide_composite_sprite: Some(wide_composite_sprite),
    };
}

type BloomSourceCameraFilter = (
    With<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurSmallHorizontalCameraMarker>,
    Without<AgentListBloomBlurSmallVerticalCameraMarker>,
    Without<AgentListBloomBlurWideHorizontalCameraMarker>,
    Without<AgentListBloomBlurWideVerticalCameraMarker>,
);
type BloomBlurSmallHorizontalCameraFilter = (
    With<AgentListBloomBlurSmallHorizontalCameraMarker>,
    Without<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurSmallVerticalCameraMarker>,
    Without<AgentListBloomBlurWideHorizontalCameraMarker>,
    Without<AgentListBloomBlurWideVerticalCameraMarker>,
);
type BloomBlurSmallVerticalCameraFilter = (
    With<AgentListBloomBlurSmallVerticalCameraMarker>,
    Without<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurSmallHorizontalCameraMarker>,
    Without<AgentListBloomBlurWideHorizontalCameraMarker>,
    Without<AgentListBloomBlurWideVerticalCameraMarker>,
);
type BloomBlurWideHorizontalCameraFilter = (
    With<AgentListBloomBlurWideHorizontalCameraMarker>,
    Without<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurSmallHorizontalCameraMarker>,
    Without<AgentListBloomBlurSmallVerticalCameraMarker>,
    Without<AgentListBloomBlurWideVerticalCameraMarker>,
);
type BloomBlurWideVerticalCameraFilter = (
    With<AgentListBloomBlurWideVerticalCameraMarker>,
    Without<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurSmallHorizontalCameraMarker>,
    Without<AgentListBloomBlurSmallVerticalCameraMarker>,
    Without<AgentListBloomBlurWideHorizontalCameraMarker>,
);
type BloomCompositeFilter = (
    With<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomBlurWideVerticalQuadMarker>,
);
type BloomWideCompositeFilter = (
    With<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomBlurWideVerticalQuadMarker>,
);
type BloomSourceSpriteFilter = (
    With<AgentListBloomSourceSprite>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomBlurWideVerticalQuadMarker>,
);
type BloomBlurSmallHorizontalQuadFilter = (
    With<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomBlurWideVerticalQuadMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
);
type BloomBlurSmallVerticalQuadFilter = (
    With<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomBlurWideVerticalQuadMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
);
type BloomBlurWideHorizontalQuadFilter = (
    With<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurWideVerticalQuadMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
);
type BloomBlurWideVerticalQuadFilter = (
    With<AgentListBloomBlurWideVerticalQuadMarker>,
    Without<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
);
type BloomDebugBackdropFilter = (
    With<AgentListBloomDebugBackdropMarker>,
    Without<AgentListBloomDebugPreviewMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomBlurWideVerticalQuadMarker>,
);
type BloomDebugPreviewFilter = (
    With<AgentListBloomDebugPreviewMarker>,
    Without<AgentListBloomDebugBackdropMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurSmallHorizontalQuadMarker>,
    Without<AgentListBloomBlurSmallVerticalQuadMarker>,
    Without<AgentListBloomBlurWideHorizontalQuadMarker>,
    Without<AgentListBloomBlurWideVerticalQuadMarker>,
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
    blur_small_h_cameras:
        Query<'w, 's, &'static mut RenderTarget, BloomBlurSmallHorizontalCameraFilter>,
    blur_small_v_cameras:
        Query<'w, 's, &'static mut RenderTarget, BloomBlurSmallVerticalCameraFilter>,
    blur_wide_h_cameras:
        Query<'w, 's, &'static mut RenderTarget, BloomBlurWideHorizontalCameraFilter>,
    blur_wide_v_cameras:
        Query<'w, 's, &'static mut RenderTarget, BloomBlurWideVerticalCameraFilter>,
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
    wide_composites: Query<
        'w,
        's,
        (
            &'static mut Sprite,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomWideCompositeFilter,
    >,
    debug_backdrops:
        Query<'w, 's, (&'static mut Transform, &'static mut Visibility), BloomDebugBackdropFilter>,
    debug_previews: Query<
        'w,
        's,
        (
            &'static AgentListBloomDebugPreviewMarker,
            &'static mut Sprite,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomDebugPreviewFilter,
    >,
    blur_small_h_quads: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomBlurMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomBlurSmallHorizontalQuadFilter,
    >,
    blur_small_v_quads: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomBlurMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomBlurSmallVerticalQuadFilter,
    >,
    blur_wide_h_quads: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomBlurMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomBlurWideHorizontalQuadFilter,
    >,
    blur_wide_v_quads: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomBlurMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomBlurWideVerticalQuadFilter,
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
    let target_size = bloom_target_size(&ctx.primary_window);
    let pass = &mut ctx.bloom.agent_list;

    if !image_matches_size(&ctx.images, &pass.source_image, target_size) {
        pass.source_image = ctx.images.add(bloom_target_image(target_size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_small_h_image, target_size) {
        pass.blur_small_h_image = ctx.images.add(bloom_target_image(target_size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_small_v_image, target_size) {
        pass.blur_small_v_image = ctx.images.add(bloom_target_image(target_size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_wide_h_image, target_size) {
        pass.blur_wide_h_image = ctx.images.add(bloom_target_image(target_size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_wide_v_image, target_size) {
        pass.blur_wide_v_image = ctx.images.add(bloom_target_image(target_size));
    }

    if let Some(camera) = pass.source_camera {
        if let Ok(mut target) = ctx.source_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.source_image.clone().into());
        }
    }
    if let Some(camera) = pass.blur_small_h_camera {
        if let Ok(mut target) = ctx.blur_small_h_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.blur_small_h_image.clone().into());
        }
    }
    if let Some(camera) = pass.blur_small_v_camera {
        if let Ok(mut target) = ctx.blur_small_v_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.blur_small_v_image.clone().into());
        }
    }
    if let Some(camera) = pass.blur_wide_h_camera {
        if let Ok(mut target) = ctx.blur_wide_h_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.blur_wide_h_image.clone().into());
        }
    }
    if let Some(camera) = pass.blur_wide_v_camera {
        if let Ok(mut target) = ctx.blur_wide_v_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.blur_wide_v_image.clone().into());
        }
    }

    if let Some(quad) = pass.blur_small_h_quad {
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.blur_small_h_quads.get_mut(quad)
        {
            transform.translation = Vec3::ZERO;
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::new(target_size.x as f32, target_size.y as f32, 1.0);
            *visibility = Visibility::Hidden;
            if let Some(material) = ctx.blur_materials.get_mut(material_handle.id()) {
                material.image = pass.source_image.clone();
                material.uniform = blur_uniform(
                    Vec2::new(SMALL_BLUR_STEP_SCALE / target_size.x.max(1) as f32, 0.0),
                    1.0,
                );
            }
        }
    }
    if let Some(quad) = pass.blur_small_v_quad {
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.blur_small_v_quads.get_mut(quad)
        {
            transform.translation = Vec3::ZERO;
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::new(target_size.x as f32, target_size.y as f32, 1.0);
            *visibility = Visibility::Hidden;
            if let Some(material) = ctx.blur_materials.get_mut(material_handle.id()) {
                material.image = pass.blur_small_h_image.clone();
                material.uniform = blur_uniform(
                    Vec2::new(0.0, SMALL_BLUR_STEP_SCALE / target_size.y.max(1) as f32),
                    1.0,
                );
            }
        }
    }
    if let Some(quad) = pass.blur_wide_h_quad {
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.blur_wide_h_quads.get_mut(quad)
        {
            transform.translation = Vec3::ZERO;
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::new(target_size.x as f32, target_size.y as f32, 1.0);
            *visibility = Visibility::Hidden;
            if let Some(material) = ctx.blur_materials.get_mut(material_handle.id()) {
                material.image = pass.blur_small_v_image.clone();
                material.uniform = blur_uniform(
                    Vec2::new(WIDE_BLUR_STEP_SCALE / target_size.x.max(1) as f32, 0.0),
                    1.0,
                );
            }
        }
    }
    if let Some(quad) = pass.blur_wide_v_quad {
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.blur_wide_v_quads.get_mut(quad)
        {
            transform.translation = Vec3::ZERO;
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::new(target_size.x as f32, target_size.y as f32, 1.0);
            *visibility = Visibility::Hidden;
            if let Some(material) = ctx.blur_materials.get_mut(material_handle.id()) {
                material.image = pass.blur_wide_h_image.clone();
                material.uniform = blur_uniform(
                    Vec2::new(0.0, WIDE_BLUR_STEP_SCALE / target_size.y.max(1) as f32),
                    1.0,
                );
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
        let target_rect = scale_rect_into_target(&ctx.primary_window, target_size, spec.rect);
        if let Some((entity, _, _, _)) = existing.get(&spec.key) {
            if let Ok((_, _, mut sprite, mut transform, mut visibility)) =
                ctx.source_sprites.get_mut(*entity)
            {
                sprite.color = spec.color;
                sprite.custom_size = Some(Vec2::new(target_rect.w, target_rect.h));
                transform.translation =
                    rect_transform_for_frame(target_size.as_vec2(), target_rect, 0.0).translation;
                *visibility = Visibility::Visible;
            }
        } else {
            ctx.commands.spawn((
                Sprite {
                    color: spec.color,
                    custom_size: Some(Vec2::new(target_rect.w, target_rect.h)),
                    ..default()
                },
                rect_transform_for_frame(target_size.as_vec2(), target_rect, 0.0),
                RenderLayers::layer(BLOOM_SOURCE_LAYER),
                Visibility::Visible,
                spec.key,
            ));
        }
    }

    let active = !specs.is_empty();
    if let Some(quad) = pass.blur_small_h_quad {
        if let Ok((_, _, mut visibility)) = ctx.blur_small_h_quads.get_mut(quad) {
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(quad) = pass.blur_small_v_quad {
        if let Ok((_, _, mut visibility)) = ctx.blur_small_v_quads.get_mut(quad) {
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(quad) = pass.blur_wide_h_quad {
        if let Ok((_, _, mut visibility)) = ctx.blur_wide_h_quads.get_mut(quad) {
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(quad) = pass.blur_wide_v_quad {
        if let Ok((_, _, mut visibility)) = ctx.blur_wide_v_quads.get_mut(quad) {
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(composite) = pass.composite_sprite {
        if let Ok((mut sprite, mut transform, mut visibility)) = ctx.composites.get_mut(composite) {
            sprite.image = pass.blur_small_v_image.clone();
            let intensity = ctx.settings.agent_list_intensity * BLOOM_INTENSITY_SCALE;
            sprite.color = Color::linear_rgba(
                SMALL_BLUR_GAIN * intensity,
                SMALL_BLUR_GAIN * intensity,
                SMALL_BLUR_GAIN * intensity,
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
    if let Some(composite) = pass.wide_composite_sprite {
        if let Ok((mut sprite, mut transform, mut visibility)) =
            ctx.wide_composites.get_mut(composite)
        {
            sprite.image = pass.blur_wide_v_image.clone();
            let intensity = ctx.settings.agent_list_intensity * BLOOM_INTENSITY_SCALE;
            sprite.color = Color::linear_rgba(
                WIDE_BLUR_GAIN * intensity,
                WIDE_BLUR_GAIN * intensity,
                WIDE_BLUR_GAIN * intensity,
                1.0,
            );
            sprite.custom_size = Some(Vec2::new(
                ctx.primary_window.width(),
                ctx.primary_window.height(),
            ));
            transform.translation = Vec3::new(0.0, 0.0, BLOOM_COMPOSITE_Z + 0.01);
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::ONE;
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }

    let previews_visible = ctx.settings.debug_previews;
    for (mut transform, mut visibility) in &mut ctx.debug_backdrops {
        *transform = bloom_debug_backdrop_transform(&ctx.primary_window, BLOOM_DEBUG_PREVIEW_Z);
        *visibility = if previews_visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (marker, mut sprite, mut transform, mut visibility) in &mut ctx.debug_previews {
        sprite.image = match marker.stage {
            AgentListBloomDebugPreviewStage::Source => pass.source_image.clone(),
            AgentListBloomDebugPreviewStage::SmallBlur => pass.blur_small_v_image.clone(),
            AgentListBloomDebugPreviewStage::WideBlur => pass.blur_wide_v_image.clone(),
        };
        sprite.custom_size = Some(Vec2::new(
            BLOOM_DEBUG_PREVIEW_WIDTH,
            BLOOM_DEBUG_PREVIEW_HEIGHT,
        ));
        *transform = bloom_debug_preview_transform(
            &ctx.primary_window,
            marker.stage,
            BLOOM_DEBUG_PREVIEW_Z + 0.01,
        );
        *visibility = if previews_visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
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
