use crate::{
    app::AppSessionState,
    startup::DaemonConnectionState,
    terminals::{ActiveTerminalContentState, TerminalFocusState, TerminalId},
};

use super::{
    modules::{
        agent_row_rect, agent_rows, row_main_rect, AgentListRowSection, AGENT_LIST_BLOOM_RED_B,
        AGENT_LIST_BLOOM_RED_G, AGENT_LIST_BLOOM_RED_R, AGENT_LIST_PAUSED_GRAY_B,
        AGENT_LIST_PAUSED_GRAY_G, AGENT_LIST_PAUSED_GRAY_R, AGENT_LIST_WORKING_GREEN_B,
        AGENT_LIST_WORKING_GREEN_G, AGENT_LIST_WORKING_GREEN_R,
    },
    state::{AgentListUiState, HudLayoutState, HudRect},
    view_models::{AgentListRowKey, AgentListView},
    widgets::HudWidgetKey,
};
use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, ClearColorConfig, RenderTarget},
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
use std::{collections::BTreeSet, env};

use super::compositor::{HUD_COMPOSITE_BLOOM_RENDER_LAYER, HUD_COMPOSITE_FOREGROUND_Z};

const BLOOM_SOURCE_LAYER: usize = 29;
const BLOOM_BLUR_SMALL_LAYER: usize = 30;
const BLOOM_BLUR_WIDE_LAYER: usize = 31;
const BLOOM_COMPOSITE_LAYER: usize = HUD_COMPOSITE_BLOOM_RENDER_LAYER;
const BLOOM_COMPOSITE_Z: f32 = HUD_COMPOSITE_FOREGROUND_Z + 0.1;
const BLOOM_TARGET_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const BLOOM_BLUR_SHADER_PATH: &str = "shaders/hud_agent_list_bloom_blur.wgsl";
const BLOOM_COMPOSITE_SHADER_PATH: &str = "shaders/hud_agent_list_bloom_composite.wgsl";
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

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HudBloomOcclusionState {
    pub(crate) rect: Option<HudRect>,
}

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
fn resolve_agent_list_bloom_intensity(raw: Option<&str>) -> f32 {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(DEFAULT_BLOOM_INTENSITY)
}

/// Parses the boolean flag that enables on-screen debug previews for bloom intermediate stages.
///
/// The parser accepts a small truthy vocabulary and treats everything else as disabled.
fn resolve_agent_list_bloom_debug_previews(raw: Option<&str>) -> bool {
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
struct AgentListBloomCameraMarker;

#[derive(Component)]
struct AgentListBloomCompositeMarker;

#[derive(Component)]
struct AgentListBloomWideCompositeMarker;

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
struct AgentListBloomBlurUniform {
    texel_step_gain: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct AgentListBloomBlurMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub(crate) image: Handle<Image>,
    #[uniform(2)]
    uniform: AgentListBloomBlurUniform,
}

#[derive(Clone, Copy, Debug, ShaderType)]
struct AgentListBloomCompositeUniform {
    tint: Vec4,
    occlusion_rect_uv: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct AgentListBloomCompositeMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub(crate) image: Handle<Image>,
    #[uniform(2)]
    uniform: AgentListBloomCompositeUniform,
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

impl Material2d for AgentListBloomCompositeMaterial {
    fn fragment_shader() -> ShaderRef {
        BLOOM_COMPOSITE_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AgentListBloomSourceKind {
    Main,
    Marker,
    Aegis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AgentListBloomSourceSegment {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct AgentListBloomSourceSprite {
    terminal_id: TerminalId,
    kind: AgentListBloomSourceKind,
    segment: AgentListBloomSourceSegment,
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

fn composite_occlusion_rect_uv(window: &Window, rect: Option<HudRect>) -> Vec4 {
    let Some(rect) = rect else {
        return Vec4::new(2.0, 2.0, -1.0, -1.0);
    };
    let width = window.width().max(1.0);
    let height = window.height().max(1.0);
    Vec4::new(
        rect.x / width,
        rect.y / height,
        (rect.x + rect.w) / width,
        (rect.y + rect.h) / height,
    )
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

/// Builds a linear-space bloom source color from one shared sRGB reference color.
///
/// The conversion goes through sRGB->linear once and then applies caller-controlled intensity and
/// alpha scaling so all bloom source colors derive from the same configurable hue source.
fn bloom_reference_color(red: u8, green: u8, blue: u8, scale: f32, alpha: f32) -> Color {
    let linear: LinearRgba = Srgba::rgba_u8(red, green, blue, 255).into();
    Color::linear_rgba(
        linear.red * scale,
        linear.green * scale,
        linear.blue * scale,
        alpha,
    )
}

fn bloom_reference_red(scale: f32, alpha: f32) -> Color {
    bloom_reference_color(
        AGENT_LIST_BLOOM_RED_R,
        AGENT_LIST_BLOOM_RED_G,
        AGENT_LIST_BLOOM_RED_B,
        scale,
        alpha,
    )
}

fn bloom_reference_working_green(scale: f32, alpha: f32) -> Color {
    bloom_reference_color(
        AGENT_LIST_WORKING_GREEN_R,
        AGENT_LIST_WORKING_GREEN_G,
        AGENT_LIST_WORKING_GREEN_B,
        scale,
        alpha,
    )
}

fn bloom_reference_aegis_pink(scale: f32, alpha: f32) -> Color {
    bloom_reference_color(255, 105, 180, scale, alpha)
}

fn bloom_reference_paused_gray(scale: f32, alpha: f32) -> Color {
    bloom_reference_color(
        AGENT_LIST_PAUSED_GRAY_R,
        AGENT_LIST_PAUSED_GRAY_G,
        AGENT_LIST_PAUSED_GRAY_B,
        scale,
        alpha,
    )
}

/// Chooses the bloom source color for one border strip based on selected-row state.
///
/// Paused selected rows use a dim gray glow, working rows use the green palette, and other selected
/// rows keep the existing selected red palette.
fn bloom_source_color(kind: AgentListBloomSourceKind, paused: bool, working: bool) -> Color {
    if kind == AgentListBloomSourceKind::Aegis {
        return bloom_reference_aegis_pink(7.0, 1.0);
    }
    if paused {
        return match kind {
            AgentListBloomSourceKind::Main => bloom_reference_paused_gray(4.0, 1.0),
            AgentListBloomSourceKind::Marker => bloom_reference_paused_gray(5.0, 1.0),
            AgentListBloomSourceKind::Aegis => bloom_reference_aegis_pink(7.0, 1.0),
        };
    }
    if working {
        return match kind {
            AgentListBloomSourceKind::Main => bloom_reference_working_green(5.0, 1.0),
            AgentListBloomSourceKind::Marker => bloom_reference_working_green(6.0, 1.0),
            AgentListBloomSourceKind::Aegis => bloom_reference_aegis_pink(7.0, 1.0),
        };
    }

    match kind {
        AgentListBloomSourceKind::Main => bloom_reference_red(5.0, 1.0),
        AgentListBloomSourceKind::Marker => bloom_reference_red(6.0, 1.0),
        AgentListBloomSourceKind::Aegis => bloom_reference_aegis_pink(7.0, 1.0),
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
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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
pub(crate) fn hud_bloom_additive_blend_state() -> BlendState {
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
fn active_bloom_row_key(
    focus_state: &TerminalFocusState,
    active_content: &ActiveTerminalContentState,
    agent_list_view: &AgentListView,
) -> Option<AgentListRowKey> {
    if let Some(session_uid) = active_content.selected_owned_tmux_session_uid() {
        return Some(AgentListRowKey::OwnedTmux(session_uid.to_owned()));
    }
    let active_terminal_id = focus_state.active_id()?;
    agent_list_view.rows.iter().find_map(|row| match &row.kind {
        crate::hud::view_models::AgentListRowKind::Agent {
            terminal_id: Some(terminal_id),
            ..
        } if *terminal_id == active_terminal_id => Some(row.key.clone()),
        _ => None,
    })
}

fn build_bloom_specs(
    content_rect: HudRect,
    scroll_offset: f32,
    hovered_row: Option<&crate::hud::view_models::AgentListRowKey>,
    active_row_key: Option<&AgentListRowKey>,
    aegis_row_keys: &BTreeSet<AgentListRowKey>,
    agent_list_view: &AgentListView,
) -> Vec<BloomSourceSpec> {
    let rows = agent_rows(content_rect, scroll_offset, hovered_row, agent_list_view);
    let owner_terminal_ids = rows
        .iter()
        .filter_map(|row| {
            (!row.is_tmux_child())
                .then(|| row.owner_agent_id().zip(row.terminal_id()))
                .flatten()
        })
        .collect::<std::collections::HashMap<_, _>>();

    let mut specs = Vec::new();
    for row in rows.into_iter().filter(|row| {
        active_row_key.is_some_and(|active_row_key| row.key == *active_row_key)
            || aegis_row_keys.contains(&row.key)
    }) {
        let working = row.activity() == Some(crate::hud::view_models::AgentListActivity::Working);
        let paused = row.paused();
        let terminal_id = if row.is_tmux_child() {
            let Some(terminal_id) = row
                .owner_agent_id()
                .and_then(|owner_agent_id| owner_terminal_ids.get(&owner_agent_id).copied())
            else {
                continue;
            };
            terminal_id
        } else if let Some(terminal_id) = row.terminal_id() {
            terminal_id
        } else {
            continue;
        };
        if row.rect.y + row.rect.h < content_rect.y || row.rect.y > content_rect.y + content_rect.h
        {
            continue;
        }

        let aegis_enabled = aegis_row_keys.contains(&row.key);
        if aegis_enabled && !row.is_tmux_child() {
            let expanded_rect = HudRect {
                x: row.rect.x - 4.0,
                y: row.rect.y - 3.0,
                w: row.rect.w + 8.0,
                h: row.rect.h + 6.0,
            };
            let color = bloom_source_color(AgentListBloomSourceKind::Aegis, false, false);
            for (segment, border_rect) in bloom_border_rects(expanded_rect, 5.0) {
                specs.push(BloomSourceSpec {
                    key: AgentListBloomSourceSprite {
                        terminal_id,
                        kind: AgentListBloomSourceKind::Aegis,
                        segment,
                    },
                    rect: border_rect,
                    color,
                });
            }
        }

        if row.is_tmux_child() {
            let main_rect = row_main_rect(&row);
            let color = bloom_source_color(AgentListBloomSourceKind::Main, false, false);
            for (segment, border_rect) in bloom_border_rects(main_rect, 3.0) {
                specs.push(BloomSourceSpec {
                    key: AgentListBloomSourceSprite {
                        terminal_id,
                        kind: AgentListBloomSourceKind::Main,
                        segment,
                    },
                    rect: border_rect,
                    color,
                });
            }
        } else if active_row_key.is_some_and(|active_row_key| row.key == *active_row_key) {
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
                let color = bloom_source_color(kind, paused, working);
                for (segment, border_rect) in bloom_border_rects(rect, thickness) {
                    specs.push(BloomSourceSpec {
                        key: AgentListBloomSourceSprite {
                            terminal_id,
                            kind,
                            segment,
                        },
                        rect: border_rect,
                        color,
                    });
                }
            }
        }
    }
    specs
}

fn create_bloom_images(
    images: &mut Assets<Image>,
    target_size: UVec2,
) -> (Handle<Image>, Handle<Image>, Handle<Image>) {
    (
        images.add(bloom_target_image(target_size)),
        images.add(bloom_target_image(target_size)),
        images.add(bloom_target_image(target_size)),
    )
}

fn spawn_bloom_source_camera(commands: &mut Commands, source_image: &Handle<Image>) -> Entity {
    commands
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
        .id()
}

fn spawn_small_blur_pass(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    blur_materials: &mut Assets<AgentListBloomBlurMaterial>,
    source_image: &Handle<Image>,
    target_image: &Handle<Image>,
    target_size: UVec2,
    target_texel_size: Vec2,
) -> (Entity, Entity) {
    let small_material = blur_materials.add(AgentListBloomBlurMaterial {
        image: source_image.clone(),
        uniform: blur_uniform(target_texel_size, SMALL_BLUR_STEP_SCALE, 1.0),
    });
    let blur_small_quad = commands
        .spawn((
            Mesh2d(meshes.add(Rectangle::default())),
            MeshMaterial2d(small_material),
            fullscreen_transform_for_frame(target_size.as_vec2(), 0.0),
            RenderLayers::layer(BLOOM_BLUR_SMALL_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurSmallQuadMarker,
        ))
        .id();
    let blur_small_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -99,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(target_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_SMALL_LAYER),
            AgentListBloomBlurSmallCameraMarker,
        ))
        .id();
    (blur_small_quad, blur_small_camera)
}

fn spawn_wide_blur_pass(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    blur_materials: &mut Assets<AgentListBloomBlurMaterial>,
    source_image: &Handle<Image>,
    target_image: &Handle<Image>,
    target_size: UVec2,
    target_texel_size: Vec2,
) -> (Entity, Entity) {
    let wide_material = blur_materials.add(AgentListBloomBlurMaterial {
        image: source_image.clone(),
        uniform: blur_uniform(target_texel_size, WIDE_BLUR_STEP_SCALE, 1.0),
    });
    let blur_wide_quad = commands
        .spawn((
            Mesh2d(meshes.add(Rectangle::default())),
            MeshMaterial2d(wide_material),
            fullscreen_transform_for_frame(target_size.as_vec2(), 0.0),
            RenderLayers::layer(BLOOM_BLUR_WIDE_LAYER),
            Visibility::Hidden,
            AgentListBloomBlurWideQuadMarker,
        ))
        .id();
    let blur_wide_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -98,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(target_image.clone().into()),
            RenderLayers::layer(BLOOM_BLUR_WIDE_LAYER),
            AgentListBloomBlurWideCameraMarker,
        ))
        .id();
    (blur_wide_quad, blur_wide_camera)
}

fn spawn_composite_sprites(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    composite_materials: &mut Assets<AgentListBloomCompositeMaterial>,
    blur_small_image: &Handle<Image>,
    blur_wide_image: &Handle<Image>,
    primary_window: &Window,
) -> (Entity, Entity) {
    let composite_mesh = meshes.add(Rectangle::default());
    let composite_sprite = commands
        .spawn((
            Mesh2d(composite_mesh.clone()),
            MeshMaterial2d(composite_materials.add(AgentListBloomCompositeMaterial {
                image: blur_small_image.clone(),
                uniform: AgentListBloomCompositeUniform {
                    tint: Vec4::new(SMALL_BLUR_GAIN, SMALL_BLUR_GAIN, SMALL_BLUR_GAIN, 1.0),
                    occlusion_rect_uv: Vec4::new(2.0, 2.0, -1.0, -1.0),
                },
            })),
            fullscreen_transform_for_frame(
                Vec2::new(primary_window.width(), primary_window.height()),
                BLOOM_COMPOSITE_Z,
            ),
            RenderLayers::layer(BLOOM_COMPOSITE_LAYER),
            Visibility::Hidden,
            AgentListBloomCompositeMarker,
        ))
        .id();
    let wide_composite_sprite = commands
        .spawn((
            Mesh2d(composite_mesh),
            MeshMaterial2d(composite_materials.add(AgentListBloomCompositeMaterial {
                image: blur_wide_image.clone(),
                uniform: AgentListBloomCompositeUniform {
                    tint: Vec4::new(WIDE_BLUR_GAIN, WIDE_BLUR_GAIN, WIDE_BLUR_GAIN, 1.0),
                    occlusion_rect_uv: Vec4::new(2.0, 2.0, -1.0, -1.0),
                },
            })),
            fullscreen_transform_for_frame(
                Vec2::new(primary_window.width(), primary_window.height()),
                BLOOM_COMPOSITE_Z + 0.01,
            ),
            RenderLayers::layer(BLOOM_COMPOSITE_LAYER),
            Visibility::Hidden,
            AgentListBloomWideCompositeMarker,
        ))
        .id();
    (composite_sprite, wide_composite_sprite)
}

fn spawn_debug_preview_entities(
    commands: &mut Commands,
    primary_window: &Window,
    source_image: &Handle<Image>,
    blur_small_image: &Handle<Image>,
    blur_wide_image: &Handle<Image>,
    debug_previews: bool,
) {
    let debug_backdrop = commands
        .spawn((
            Sprite {
                color: Color::srgba(0.0, 0.0, 0.0, 0.92),
                custom_size: Some(Vec2::new(1.0, 1.0)),
                ..default()
            },
            bloom_debug_backdrop_transform(primary_window, BLOOM_DEBUG_PREVIEW_Z),
            RenderLayers::layer(0),
            Visibility::Hidden,
            AgentListBloomDebugBackdropMarker,
        ))
        .id();
    let debug_source_preview = commands
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
                primary_window,
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
    let debug_small_preview = commands
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
                primary_window,
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
    let debug_wide_preview = commands
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
                primary_window,
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

    if debug_previews {
        commands.entity(debug_backdrop).insert(Visibility::Visible);
        commands
            .entity(debug_source_preview)
            .insert(Visibility::Visible);
        commands
            .entity(debug_small_preview)
            .insert(Visibility::Visible);
        commands
            .entity(debug_wide_preview)
            .insert(Visibility::Visible);
    }
}

#[derive(SystemParam)]
struct HudWidgetBloomSetupContext<'w, 's> {
    commands: Commands<'w, 's>,
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    settings: Res<'w, HudBloomSettings>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    blur_materials: ResMut<'w, Assets<AgentListBloomBlurMaterial>>,
    composite_materials: ResMut<'w, Assets<AgentListBloomCompositeMaterial>>,
    bloom: ResMut<'w, HudWidgetBloom>,
}

/// Creates the bloom render graph entities and offscreen images used by the agent-list bloom effect.
///
/// Startup allocates three float targets (source, small blur, wide blur), creates the cameras and
/// fullscreen quads that feed the blur passes, and spawns the hidden composite sprites that will later
/// be shown by the sync system once real bloom content exists.
pub(crate) fn setup_hud_widget_bloom(world: &mut World) {
    let mut state: bevy::ecs::system::SystemState<HudWidgetBloomSetupContext> =
        bevy::ecs::system::SystemState::new(world);
    let mut ctx = state.get_mut(world);
    let target_size = bloom_target_size(&ctx.primary_window);
    let target_texel_size = Vec2::new(
        1.0 / target_size.x.max(1) as f32,
        1.0 / target_size.y.max(1) as f32,
    );
    let (source_image, blur_small_image, blur_wide_image) =
        create_bloom_images(&mut ctx.images, target_size);
    let source_camera = spawn_bloom_source_camera(&mut ctx.commands, &source_image);
    let (blur_small_quad, blur_small_camera) = spawn_small_blur_pass(
        &mut ctx.commands,
        &mut ctx.meshes,
        &mut ctx.blur_materials,
        &source_image,
        &blur_small_image,
        target_size,
        target_texel_size,
    );
    let (blur_wide_quad, blur_wide_camera) = spawn_wide_blur_pass(
        &mut ctx.commands,
        &mut ctx.meshes,
        &mut ctx.blur_materials,
        &source_image,
        &blur_wide_image,
        target_size,
        target_texel_size,
    );
    let (composite_sprite, wide_composite_sprite) = spawn_composite_sprites(
        &mut ctx.commands,
        &mut ctx.meshes,
        &mut ctx.composite_materials,
        &blur_small_image,
        &blur_wide_image,
        &ctx.primary_window,
    );
    spawn_debug_preview_entities(
        &mut ctx.commands,
        &ctx.primary_window,
        &source_image,
        &blur_small_image,
        &blur_wide_image,
        ctx.settings.debug_previews,
    );

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
    state.apply(world);
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
struct HudWidgetBloomContext<'w, 's> {
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    layout_state: Res<'w, HudLayoutState>,
    app_session: Res<'w, AppSessionState>,
    agent_catalog: Option<Res<'w, crate::agents::AgentCatalog>>,
    aegis_policy: Res<'w, crate::aegis::AegisPolicyStore>,
    startup_connect: Option<Res<'w, DaemonConnectionState>>,
    focus_state: Res<'w, TerminalFocusState>,
    active_content: Res<'w, ActiveTerminalContentState>,
    agent_list_state: Res<'w, AgentListUiState>,
    agent_list_view: Res<'w, AgentListView>,
    settings: Res<'w, HudBloomSettings>,
    occlusion: Res<'w, HudBloomOcclusionState>,
    commands: Commands<'w, 's>,
    bloom: ResMut<'w, HudWidgetBloom>,
    images: ResMut<'w, Assets<Image>>,
    blur_materials: ResMut<'w, Assets<AgentListBloomBlurMaterial>>,
    composite_materials: ResMut<'w, Assets<AgentListBloomCompositeMaterial>>,
    source_cameras: Query<'w, 's, &'static mut RenderTarget, BloomSourceCameraFilter>,
    blur_small_cameras: Query<'w, 's, &'static mut RenderTarget, BloomBlurSmallCameraFilter>,
    blur_wide_cameras: Query<'w, 's, &'static mut RenderTarget, BloomBlurWideCameraFilter>,
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
    wide_composites: Query<
        'w,
        's,
        (
            &'static MeshMaterial2d<AgentListBloomCompositeMaterial>,
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

fn ensure_bloom_target_images(
    images: &mut Assets<Image>,
    pass: &mut AgentListBloomPass,
    target_size: UVec2,
) {
    if !image_matches_size(images, &pass.source_image, target_size) {
        pass.source_image = images.add(bloom_target_image(target_size));
    }
    if !image_matches_size(images, &pass.blur_small_image, target_size) {
        pass.blur_small_image = images.add(bloom_target_image(target_size));
    }
    if !image_matches_size(images, &pass.blur_wide_image, target_size) {
        pass.blur_wide_image = images.add(bloom_target_image(target_size));
    }
}

fn retarget_bloom_cameras(ctx: &mut HudWidgetBloomContext<'_, '_>, pass: &AgentListBloomPass) {
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
}

fn sync_blur_quads(
    ctx: &mut HudWidgetBloomContext<'_, '_>,
    pass: &AgentListBloomPass,
    target_size: UVec2,
    target_texel_size: Vec2,
) {
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
}

fn bloom_specs_for_sync(ctx: &HudWidgetBloomContext<'_, '_>) -> Vec<BloomSourceSpec> {
    let modal_visible = ctx.app_session.modal_visible()
        || ctx
            .startup_connect
            .as_ref()
            .is_some_and(|state| state.modal_visible());
    let enabled = ctx
        .layout_state
        .get(HudWidgetKey::AgentList)
        .map(|module| module.shell.enabled && module.shell.current_alpha > 0.01)
        .unwrap_or(false)
        && !modal_visible;
    if !enabled {
        return Vec::new();
    }
    let module = ctx
        .layout_state
        .get(HudWidgetKey::AgentList)
        .expect("agent list exists when enabled");
    let active_row_key =
        active_bloom_row_key(&ctx.focus_state, &ctx.active_content, &ctx.agent_list_view);
    let aegis_row_keys = ctx
        .agent_catalog
        .as_ref()
        .map(|agent_catalog| {
            agent_catalog
                .iter()
                .filter_map(|(agent_id, _)| {
                    agent_catalog
                        .uid(agent_id)
                        .filter(|agent_uid| ctx.aegis_policy.is_enabled(agent_uid))
                        .map(|_| AgentListRowKey::Agent(agent_id))
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    build_bloom_specs(
        module.shell.current_rect,
        ctx.agent_list_state.scroll_offset,
        ctx.agent_list_state.hovered_row.as_ref(),
        active_row_key.as_ref(),
        &aegis_row_keys,
        &ctx.agent_list_view,
    )
}

fn sync_bloom_source_sprites(
    ctx: &mut HudWidgetBloomContext<'_, '_>,
    specs: &[BloomSourceSpec],
    target_size: UVec2,
) {
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

    for spec in specs {
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
}

fn sync_bloom_composites(
    ctx: &mut HudWidgetBloomContext<'_, '_>,
    pass: &AgentListBloomPass,
    active: bool,
    bloom_ready: bool,
) {
    let occlusion_rect_uv = composite_occlusion_rect_uv(&ctx.primary_window, ctx.occlusion.rect);
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
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.composites.get_mut(composite)
        {
            if let Some(material) = ctx.composite_materials.get_mut(material_handle.id()) {
                let intensity = ctx.settings.agent_list_intensity * BLOOM_INTENSITY_SCALE;
                material.image = pass.blur_small_image.clone();
                material.uniform = AgentListBloomCompositeUniform {
                    tint: Vec4::new(
                        SMALL_BLUR_GAIN * intensity,
                        SMALL_BLUR_GAIN * intensity,
                        SMALL_BLUR_GAIN * intensity,
                        1.0,
                    ),
                    occlusion_rect_uv,
                };
            }
            *transform = fullscreen_transform_for_frame(
                Vec2::new(ctx.primary_window.width(), ctx.primary_window.height()),
                BLOOM_COMPOSITE_Z,
            );
            *visibility = if active && bloom_ready {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if let Some(composite) = pass.wide_composite_sprite {
        if let Ok((material_handle, mut transform, mut visibility)) =
            ctx.wide_composites.get_mut(composite)
        {
            if let Some(material) = ctx.composite_materials.get_mut(material_handle.id()) {
                let intensity = ctx.settings.agent_list_intensity * BLOOM_INTENSITY_SCALE;
                material.image = pass.blur_wide_image.clone();
                material.uniform = AgentListBloomCompositeUniform {
                    tint: Vec4::new(
                        WIDE_BLUR_GAIN * intensity,
                        WIDE_BLUR_GAIN * intensity,
                        WIDE_BLUR_GAIN * intensity,
                        1.0,
                    ),
                    occlusion_rect_uv,
                };
            }
            *transform = fullscreen_transform_for_frame(
                Vec2::new(ctx.primary_window.width(), ctx.primary_window.height()),
                BLOOM_COMPOSITE_Z + 0.01,
            );
            *visibility = if active && bloom_ready {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

fn sync_bloom_debug_previews(ctx: &mut HudWidgetBloomContext<'_, '_>, pass: &AgentListBloomPass) {
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

/// Rebuilds the bloom source sprites and composite visibility from live HUD/terminal state.
///
/// The sync pass keeps bloom targets sized correctly, suppresses the whole effect while HUD modals are
/// visible, regenerates the active row's border sprites, updates debug previews when enabled, and
/// shows or hides the composite sprites based on whether there is any current bloom content.
pub(crate) fn sync_hud_widget_bloom(world: &mut World) {
    let mut state: bevy::ecs::system::SystemState<HudWidgetBloomContext> =
        bevy::ecs::system::SystemState::new(world);
    let mut ctx = state.get_mut(world);
    let target_size = bloom_target_size(&ctx.primary_window);
    let target_texel_size = Vec2::new(
        1.0 / target_size.x.max(1) as f32,
        1.0 / target_size.y.max(1) as f32,
    );
    ensure_bloom_target_images(&mut ctx.images, &mut ctx.bloom.agent_list, target_size);
    let pass_snapshot = ctx.bloom.agent_list.clone();
    retarget_bloom_cameras(&mut ctx, &pass_snapshot);
    sync_blur_quads(&mut ctx, &pass_snapshot, target_size, target_texel_size);
    let specs = bloom_specs_for_sync(&ctx);
    sync_bloom_source_sprites(&mut ctx, &specs, target_size);
    let bloom_ready = image_matches_size(&ctx.images, &pass_snapshot.source_image, target_size)
        && image_matches_size(&ctx.images, &pass_snapshot.blur_small_image, target_size)
        && image_matches_size(&ctx.images, &pass_snapshot.blur_wide_image, target_size);
    sync_bloom_composites(&mut ctx, &pass_snapshot, !specs.is_empty(), bloom_ready);
    sync_bloom_debug_previews(&mut ctx, &pass_snapshot);
    state.apply(world);
}

/// Exposes the bloom source render-layer id to tests.
///
/// The production code keeps the constant private; tests use this helper to verify setup wiring.
#[cfg(test)]
fn agent_list_bloom_layer() -> usize {
    BLOOM_SOURCE_LAYER
}

/// Exposes the composite sprite's z value to tests.
///
/// This lets tests assert that the bloom composite sits exactly where the production pipeline expects
/// it in the layered HUD scene.
#[cfg(test)]
fn agent_list_bloom_z() -> f32 {
    BLOOM_COMPOSITE_Z
}

#[cfg(test)]
mod tests;
