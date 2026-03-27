use crate::{
    app::AppSessionState,
    hud::{
        modules::{
            agent_row_rect, agent_rows, AgentListRowSection, AGENT_LIST_BLOOM_RED_B,
            AGENT_LIST_BLOOM_RED_G, AGENT_LIST_BLOOM_RED_R,
        },
        AgentListView, HudLayoutState, HudRect, HudWidgetKey,
    },
    terminals::TerminalId,
};
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
const BLOOM_BLUR_SMALL_LAYER: usize = 30;
const BLOOM_BLUR_WIDE_LAYER: usize = 31;
const BLOOM_COMPOSITE_LAYER: usize = 32;
const BLOOM_COMPOSITE_Z: f32 = HUD_COMPOSITE_FOREGROUND_Z + 0.1;
const BLOOM_TARGET_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const BLOOM_BLUR_SHADER_PATH: &str = "shaders/hud_agent_list_bloom_blur.wgsl";
const BLOOM_SCALE_DIVISOR: u32 = 1;
const DEFAULT_BLOOM_INTENSITY: f32 = 0.10;
const BLOOM_INTENSITY_SCALE: f32 = 9.0;
const SMALL_BLUR_GAIN: f32 = 1.25;
const WIDE_BLUR_GAIN: f32 = 0.85;
const SMALL_BLUR_STEP_SCALE: f32 = 5.25;
const WIDE_BLUR_STEP_SCALE: f32 = 12.5;
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
    /// Reads bloom tuning defaults from the environment and falls back to the built-in constants.
    ///
    /// This keeps the bloom pipeline configurable for visual verification without requiring a separate
    /// config file path.
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

/// Parses the scalar intensity multiplier for the agent-list bloom effect.
///
/// Only finite, non-negative values are accepted; anything else falls back to the default intensity.
pub(crate) fn resolve_agent_list_bloom_intensity(raw: Option<&str>) -> f32 {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(DEFAULT_BLOOM_INTENSITY)
}

/// Parses the boolean flag that enables on-screen debug previews for bloom intermediate stages.
///
/// The parser accepts a small truthy vocabulary and treats everything else as disabled.
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
pub(crate) struct AgentListBloomAdditiveCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurSmallCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurWideCameraMarker;

#[derive(Component)]
struct AgentListBloomBlurSmallQuadMarker;

#[derive(Component)]
struct AgentListBloomBlurWideQuadMarker;

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
    /// Returns the WGSL shader used for the separable blur passes.
    ///
    /// The material implementation is just a small Rust wrapper around this custom shader asset.
    fn fragment_shader() -> ShaderRef {
        BLOOM_BLUR_SHADER_PATH.into()
    }

    /// Forces the blur material to render opaquely into its offscreen targets.
    ///
    /// Intermediate blur passes should overwrite the target pixels instead of alpha-blending with old
    /// data.
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AgentListBloomSourceKind {
    Main,
    Marker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AgentListBloomSourceSegment {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AgentListBloomSourceSprite {
    pub(crate) terminal_id: TerminalId,
    pub(crate) kind: AgentListBloomSourceKind,
    pub(crate) segment: AgentListBloomSourceSegment,
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
    blur_small_image: Handle<Image>,
    blur_wide_image: Handle<Image>,
    source_camera: Option<Entity>,
    blur_small_camera: Option<Entity>,
    blur_wide_camera: Option<Entity>,
    blur_small_quad: Option<Entity>,
    blur_wide_quad: Option<Entity>,
    composite_sprite: Option<Entity>,
    wide_composite_sprite: Option<Entity>,
}

#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct HudWidgetBloom {
    agent_list: AgentListBloomPass,
}

/// Allocates one float render target used by the bloom pipeline.
///
/// The target is transparent-initialized, marked renderable/bindable/copy-dst, and configured with a
/// linear sampler because the blur stages sample it as a texture.
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

/// Returns whether an existing bloom image handle already matches the required size and format.
///
/// This lets the sync path reuse existing images instead of reallocating every frame.
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

/// Computes the logical window size used as the bloom pipeline's layout space.
///
/// Bloom geometry tracks HUD layout, which is expressed in logical window coordinates rather than raw
/// physical pixels.
fn logical_window_size(window: &Window) -> UVec2 {
    UVec2::new(
        window.width().round().max(1.0) as u32,
        window.height().round().max(1.0) as u32,
    )
}

/// Computes the offscreen bloom target size from the logical window size and bloom scale divisor.
///
/// Keeping this in one helper makes it easy to downsample the whole bloom pipeline later by changing
/// a single constant.
fn bloom_target_size(window: &Window) -> UVec2 {
    let size = logical_window_size(window);
    UVec2::new(
        (size.x / BLOOM_SCALE_DIVISOR).max(1),
        (size.y / BLOOM_SCALE_DIVISOR).max(1),
    )
}

/// Converts a HUD-space rectangle into a centered Bevy transform inside a frame of known size.
///
/// HUD rectangles use top-left origin coordinates; Bevy sprites are positioned around the frame
/// center. This helper performs that coordinate conversion.
fn rect_transform_for_frame(frame_size: Vec2, rect: HudRect, z: f32) -> Transform {
    Transform::from_xyz(
        rect.x + rect.w * 0.5 - frame_size.x * 0.5,
        frame_size.y * 0.5 - (rect.y + rect.h * 0.5),
        z,
    )
}

/// Builds the centered transform for a full-frame quad at a given z layer.
///
/// The quad is positioned at the frame origin and scaled to the full frame size, which is what the
/// blur/composite passes need.
fn fullscreen_transform_for_frame(frame_size: Vec2, z: f32) -> Transform {
    Transform::from_xyz(0.0, 0.0, z).with_scale(Vec3::new(frame_size.x, frame_size.y, 1.0))
}

/// Scales a HUD rectangle from logical window space into bloom-target pixel space.
///
/// This is used when the bloom targets do not have a 1:1 correspondence with the window size.
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

/// Computes the transform for one debug-preview sprite showing an intermediate bloom stage.
///
/// The previews are laid out as a fixed three-panel strip near the top-right edge of the window.
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

/// Computes the transform for the background panel behind all bloom debug previews.
///
/// The backdrop is sized from the total preview strip width plus padding so the previews read as one
/// grouped diagnostic widget.
fn bloom_debug_backdrop_transform(window: &Window, z: f32) -> Transform {
    let total_width = BLOOM_DEBUG_PREVIEW_WIDTH * 3.0 + BLOOM_DEBUG_PREVIEW_GAP * 2.0;
    let panel_width = total_width + BLOOM_DEBUG_PREVIEW_BACKDROP_PADDING * 2.0;
    let panel_height = BLOOM_DEBUG_PREVIEW_HEIGHT + BLOOM_DEBUG_PREVIEW_BACKDROP_PADDING * 2.0;
    let x = window.width() * 0.5 - BLOOM_DEBUG_PREVIEW_MARGIN - panel_width * 0.5;
    let y = window.height() * 0.5 - BLOOM_DEBUG_PREVIEW_MARGIN - panel_height * 0.5;
    Transform::from_xyz(x, y, z).with_scale(Vec3::new(panel_width, panel_height, 1.0))
}

/// Packs blur shader parameters into the material uniform type.
///
/// The vector stores texel step, blur radius in pixels, and gain in the compact format expected by
/// the custom WGSL shader.
fn blur_uniform(texel_size: Vec2, radius_pixels: f32, gain: f32) -> AgentListBloomBlurUniform {
    AgentListBloomBlurUniform {
        texel_step_gain: Vec4::new(texel_size.x, texel_size.y, radius_pixels, gain),
    }
}

/// Builds a linear-space bloom source color from the reference emissive red constant.
///
/// The conversion goes through sRGB->linear once and then applies caller-controlled intensity and
/// alpha scaling so all bloom source colors derive from the same reference hue.
fn bloom_reference_red(scale: f32, alpha: f32) -> Color {
    let linear: LinearRgba = Srgba::rgba_u8(
        AGENT_LIST_BLOOM_RED_R,
        AGENT_LIST_BLOOM_RED_G,
        AGENT_LIST_BLOOM_RED_B,
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

/// Chooses the bloom source color for one border strip based on row state and source kind.
///
/// Focused rows glow most strongly, hovered rows get a medium boost, and idle rows still contribute a
/// faint glow. Marker strips are slightly hotter than main strips.
fn bloom_source_color(focused: bool, hovered: bool, kind: AgentListBloomSourceKind) -> Color {
    match (focused, hovered, kind) {
        (true, _, AgentListBloomSourceKind::Main) => bloom_reference_red(5.0, 1.0),
        (_, true, AgentListBloomSourceKind::Main) => bloom_reference_red(2.5, 0.85),
        (_, _, AgentListBloomSourceKind::Main) => bloom_reference_red(1.0, 0.3),
        (true, _, AgentListBloomSourceKind::Marker) => bloom_reference_red(6.0, 1.0),
        (_, true, AgentListBloomSourceKind::Marker) => bloom_reference_red(3.0, 0.9),
        (_, _, AgentListBloomSourceKind::Marker) => bloom_reference_red(1.2, 0.35),
    }
}

/// Splits a row rectangle into the four thin border strips used as bloom sources.
///
/// Thickness is clamped against the rectangle dimensions so degenerate tiny rectangles still produce
/// valid positive-sized strips.
fn bloom_border_rects(
    rect: HudRect,
    thickness: f32,
) -> [(AgentListBloomSourceSegment, HudRect); 4] {
    let horizontal = thickness.min((rect.h * 0.5).max(1.0));
    let vertical = thickness.min((rect.w * 0.5).max(1.0));
    [
        (
            AgentListBloomSourceSegment::Top,
            HudRect {
                x: rect.x,
                y: rect.y,
                w: rect.w.max(1.0),
                h: horizontal,
            },
        ),
        (
            AgentListBloomSourceSegment::Right,
            HudRect {
                x: rect.x + rect.w - vertical,
                y: rect.y,
                w: vertical,
                h: rect.h.max(1.0),
            },
        ),
        (
            AgentListBloomSourceSegment::Bottom,
            HudRect {
                x: rect.x,
                y: rect.y + rect.h - horizontal,
                w: rect.w.max(1.0),
                h: horizontal,
            },
        ),
        (
            AgentListBloomSourceSegment::Left,
            HudRect {
                x: rect.x,
                y: rect.y,
                w: vertical,
                h: rect.h.max(1.0),
            },
        ),
    ]
}

/// Returns the additive blend state used when compositing the bloom result back into the HUD.
///
/// Color channels add their energy together, while alpha is preserved from the destination so the
/// bloom layer behaves like light rather than like an opaque sprite.
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

/// Builds the set of bloom source strips that should exist for the current active agent row.
///
/// Only the active row participates. For that row, the function derives the main and marker sub-rects,
/// splits each into four border segments, and attaches the appropriate emissive color to each segment.
fn build_bloom_specs(
    content_rect: HudRect,
    scroll_offset: f32,
    hovered_terminal: Option<TerminalId>,
    agent_list_view: &AgentListView,
    focus_state: &crate::terminals::TerminalFocusState,
) -> Vec<BloomSourceSpec> {
    let Some(active_id) = focus_state.active_id() else {
        return Vec::new();
    };

    let mut specs = Vec::new();
    for row in agent_rows(
        content_rect,
        scroll_offset,
        hovered_terminal,
        agent_list_view,
    ) {
        if row.terminal_id != Some(active_id)
            || row.rect.y + row.rect.h < content_rect.y
            || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }

        for (kind, rect, thickness) in [
            (
                AgentListBloomSourceKind::Main,
                agent_row_rect(row.rect, AgentListRowSection::Main),
                3.0,
            ),
            (
                AgentListBloomSourceKind::Marker,
                agent_row_rect(row.rect, AgentListRowSection::Marker),
                2.5,
            ),
        ] {
            let color = bloom_source_color(row.focused, row.hovered, kind);
            for (segment, border_rect) in bloom_border_rects(rect, thickness) {
                specs.push(BloomSourceSpec {
                    key: AgentListBloomSourceSprite {
                        terminal_id: row
                            .terminal_id
                            .expect("active agent row should stay terminal-backed"),
                        kind,
                        segment,
                    },
                    rect: border_rect,
                    color,
                });
            }
        }
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

/// Creates the bloom render graph entities and offscreen images used by the agent-list bloom effect.
///
/// Startup allocates three float targets (source, small blur, wide blur), creates the cameras and
/// fullscreen quads that feed the blur passes, and spawns the hidden composite sprites that will later
/// be shown by the sync system once real bloom content exists.
pub(crate) fn setup_hud_widget_bloom(mut ctx: HudWidgetBloomSetupContext) {
    let target_size = bloom_target_size(&ctx.primary_window);
    let target_texel_size = Vec2::new(
        1.0 / target_size.x.max(1) as f32,
        1.0 / target_size.y.max(1) as f32,
    );
    let source_image = ctx.images.add(bloom_target_image(target_size));
    let blur_small_image = ctx.images.add(bloom_target_image(target_size));
    let blur_wide_image = ctx.images.add(bloom_target_image(target_size));

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

    let small_material = ctx.blur_materials.add(AgentListBloomBlurMaterial {
        image: source_image.clone(),
        uniform: blur_uniform(target_texel_size, SMALL_BLUR_STEP_SCALE, 1.0),
    });
    let blur_small_quad = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(small_material),
            fullscreen_transform_for_frame(target_size.as_vec2(), 0.0),
            RenderLayers::layer(BLOOM_BLUR_SMALL_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurSmallQuadMarker,
        ))
        .id();
    let blur_small_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -99,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_small_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_SMALL_LAYER),
            AgentListBloomBlurSmallCameraMarker,
        ))
        .id();

    let wide_material = ctx.blur_materials.add(AgentListBloomBlurMaterial {
        image: source_image.clone(),
        uniform: blur_uniform(target_texel_size, WIDE_BLUR_STEP_SCALE, 1.0),
    });
    let blur_wide_quad = ctx
        .commands
        .spawn((
            Mesh2d(ctx.meshes.add(Rectangle::default())),
            MeshMaterial2d(wide_material),
            fullscreen_transform_for_frame(target_size.as_vec2(), 0.0),
            RenderLayers::layer(BLOOM_BLUR_WIDE_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurWideQuadMarker,
        ))
        .id();
    let blur_wide_camera = ctx
        .commands
        .spawn((
            Camera2d,
            Camera {
                order: -98,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(blur_wide_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_WIDE_LAYER),
            AgentListBloomBlurWideCameraMarker,
        ))
        .id();

    let composite_sprite = ctx
        .commands
        .spawn((
            Sprite {
                image: blur_small_image.clone(),
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
                image: blur_wide_image.clone(),
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
                image: blur_small_image.clone(),
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
                image: blur_wide_image.clone(),
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
        blur_small_image,
        blur_wide_image,
        source_camera: Some(source_camera),
        blur_small_camera: Some(blur_small_camera),
        blur_wide_camera: Some(blur_wide_camera),
        blur_small_quad: Some(blur_small_quad),
        blur_wide_quad: Some(blur_wide_quad),
        composite_sprite: Some(composite_sprite),
        wide_composite_sprite: Some(wide_composite_sprite),
    };
}

type BloomSourceCameraFilter = (
    With<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurSmallCameraMarker>,
    Without<AgentListBloomBlurWideCameraMarker>,
);
type BloomBlurSmallCameraFilter = (
    With<AgentListBloomBlurSmallCameraMarker>,
    Without<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurWideCameraMarker>,
);
type BloomBlurWideCameraFilter = (
    With<AgentListBloomBlurWideCameraMarker>,
    Without<AgentListBloomCameraMarker>,
    Without<AgentListBloomBlurSmallCameraMarker>,
);
type BloomCompositeFilter = (
    With<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurSmallQuadMarker>,
    Without<AgentListBloomBlurWideQuadMarker>,
);
type BloomWideCompositeFilter = (
    With<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurSmallQuadMarker>,
    Without<AgentListBloomBlurWideQuadMarker>,
);
type BloomSourceSpriteFilter = (
    With<AgentListBloomSourceSprite>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomBlurSmallQuadMarker>,
    Without<AgentListBloomBlurWideQuadMarker>,
);
type BloomBlurSmallQuadFilter = (
    With<AgentListBloomBlurSmallQuadMarker>,
    Without<AgentListBloomBlurWideQuadMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
);
type BloomBlurWideQuadFilter = (
    With<AgentListBloomBlurWideQuadMarker>,
    Without<AgentListBloomBlurSmallQuadMarker>,
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
    Without<AgentListBloomBlurSmallQuadMarker>,
    Without<AgentListBloomBlurWideQuadMarker>,
);
type BloomDebugPreviewFilter = (
    With<AgentListBloomDebugPreviewMarker>,
    Without<AgentListBloomDebugBackdropMarker>,
    Without<AgentListBloomCompositeMarker>,
    Without<AgentListBloomWideCompositeMarker>,
    Without<AgentListBloomSourceSprite>,
    Without<AgentListBloomBlurSmallQuadMarker>,
    Without<AgentListBloomBlurWideQuadMarker>,
);

#[derive(SystemParam)]
pub(crate) struct HudWidgetBloomContext<'w, 's> {
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    layout_state: Res<'w, HudLayoutState>,
    app_session: Res<'w, AppSessionState>,
    focus_state: Res<'w, crate::terminals::TerminalFocusState>,
    agent_list_view: Res<'w, AgentListView>,
    settings: Res<'w, HudBloomSettings>,
    commands: Commands<'w, 's>,
    bloom: ResMut<'w, HudWidgetBloom>,
    images: ResMut<'w, Assets<Image>>,
    blur_materials: ResMut<'w, Assets<AgentListBloomBlurMaterial>>,
    source_cameras: Query<'w, 's, &'static mut RenderTarget, BloomSourceCameraFilter>,
    blur_small_cameras: Query<'w, 's, &'static mut RenderTarget, BloomBlurSmallCameraFilter>,
    blur_wide_cameras: Query<'w, 's, &'static mut RenderTarget, BloomBlurWideCameraFilter>,
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
    blur_small_quads: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomBlurMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomBlurSmallQuadFilter,
    >,
    blur_wide_quads: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomBlurMaterial>,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        BloomBlurWideQuadFilter,
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

/// Rebuilds the bloom source sprites and composite visibility from live HUD/terminal state.
///
/// The sync pass keeps bloom targets sized correctly, suppresses the whole effect while HUD modals are
/// visible, regenerates the active row's border sprites, updates debug previews when enabled, and
/// shows or hides the composite sprites based on whether there is any current bloom content.
pub(crate) fn sync_hud_widget_bloom(mut ctx: HudWidgetBloomContext) {
    let target_size = bloom_target_size(&ctx.primary_window);
    let pass = &mut ctx.bloom.agent_list;

    let target_texel_size = Vec2::new(
        1.0 / target_size.x.max(1) as f32,
        1.0 / target_size.y.max(1) as f32,
    );

    if !image_matches_size(&ctx.images, &pass.source_image, target_size) {
        pass.source_image = ctx.images.add(bloom_target_image(target_size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_small_image, target_size) {
        pass.blur_small_image = ctx.images.add(bloom_target_image(target_size));
    }
    if !image_matches_size(&ctx.images, &pass.blur_wide_image, target_size) {
        pass.blur_wide_image = ctx.images.add(bloom_target_image(target_size));
    }

    if let Some(camera) = pass.source_camera {
        if let Ok(mut target) = ctx.source_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.source_image.clone().into());
        }
    }
    if let Some(camera) = pass.blur_small_camera {
        if let Ok(mut target) = ctx.blur_small_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.blur_small_image.clone().into());
        }
    }
    if let Some(camera) = pass.blur_wide_camera {
        if let Ok(mut target) = ctx.blur_wide_cameras.get_mut(camera) {
            *target = RenderTarget::Image(pass.blur_wide_image.clone().into());
        }
    }

    if let Some(quad) = pass.blur_small_quad {
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.blur_small_quads.get_mut(quad)
        {
            transform.translation = Vec3::ZERO;
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::new(target_size.x as f32, target_size.y as f32, 1.0);
            *visibility = Visibility::Hidden;
            if let Some(material) = ctx.blur_materials.get_mut(material_handle.id()) {
                material.image = pass.source_image.clone();
                material.uniform = blur_uniform(target_texel_size, SMALL_BLUR_STEP_SCALE, 1.0);
            }
        }
    }
    if let Some(quad) = pass.blur_wide_quad {
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.blur_wide_quads.get_mut(quad)
        {
            transform.translation = Vec3::ZERO;
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::new(target_size.x as f32, target_size.y as f32, 1.0);
            *visibility = Visibility::Hidden;
            if let Some(material) = ctx.blur_materials.get_mut(material_handle.id()) {
                material.image = pass.source_image.clone();
                material.uniform = blur_uniform(target_texel_size, WIDE_BLUR_STEP_SCALE, 1.0);
            }
        }
    }

    let modal_visible = ctx.app_session.composer.message_editor.visible
        || ctx.app_session.composer.task_editor.visible;
    let enabled = ctx
        .layout_state
        .get(HudWidgetKey::AgentList)
        .map(|module| module.shell.enabled && module.shell.current_alpha > 0.01)
        .unwrap_or(false)
        && !modal_visible;
    let specs = if enabled {
        let module = ctx
            .layout_state
            .get(HudWidgetKey::AgentList)
            .expect("agent list exists when enabled");
        let crate::hud::HudModuleModel::AgentList(state) = &module.model else {
            unreachable!("agent list module must have agent-list model")
        };
        build_bloom_specs(
            module.shell.current_rect,
            state.scroll_offset,
            state.hovered_terminal,
            &ctx.agent_list_view,
            &ctx.focus_state,
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

    let bloom_ready = image_matches_size(&ctx.images, &pass.source_image, target_size)
        && image_matches_size(&ctx.images, &pass.blur_small_image, target_size)
        && image_matches_size(&ctx.images, &pass.blur_wide_image, target_size);

    let active = !specs.is_empty();
    if let Some(quad) = pass.blur_small_quad {
        if let Ok((_, _, mut visibility)) = ctx.blur_small_quads.get_mut(quad) {
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(quad) = pass.blur_wide_quad {
        if let Ok((_, _, mut visibility)) = ctx.blur_wide_quads.get_mut(quad) {
            *visibility = if active {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(composite) = pass.composite_sprite {
        if let Ok((mut sprite, mut transform, mut visibility)) = ctx.composites.get_mut(composite) {
            sprite.image = pass.blur_small_image.clone();
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
            *visibility = if active && bloom_ready {
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
            sprite.image = pass.blur_wide_image.clone();
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
            *visibility = if active && bloom_ready {
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
            AgentListBloomDebugPreviewStage::SmallBlur => pass.blur_small_image.clone(),
            AgentListBloomDebugPreviewStage::WideBlur => pass.blur_wide_image.clone(),
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

/// Exposes the bloom source render-layer id to tests.
///
/// The production code keeps the constant private; tests use this helper to verify setup wiring.
#[cfg(test)]
pub(crate) fn agent_list_bloom_layer() -> usize {
    BLOOM_SOURCE_LAYER
}

/// Exposes the composite sprite's z value to tests.
///
/// This lets tests assert that the bloom composite sits exactly where the production pipeline expects
/// it in the layered HUD scene.
#[cfg(test)]
pub(crate) fn agent_list_bloom_z() -> f32 {
    BLOOM_COMPOSITE_Z
}

#[cfg(test)]
mod tests;
