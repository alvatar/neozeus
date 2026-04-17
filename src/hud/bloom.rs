use super::{
    state::{HudLayoutState, HudRect},
    widgets::HudWidgetKey,
    HudRenderVisibilityPolicy,
};
use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, CameraOutputMode, ClearColorConfig, RenderTarget},
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
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
};

use super::compositor::HUD_COMPOSITE_FOREGROUND_Z;
use super::HudLayerId;

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

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct AgentListBloomSourceSprite {
    layer_id: HudLayerId,
    source_id: u64,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct HudLayerBloomOwnerMarker {
    layer_id: HudLayerId,
}

#[derive(Clone, Debug)]
struct BloomSourceSpec {
    key: AgentListBloomSourceSprite,
    rect: HudRect,
    color: Color,
}

#[derive(Clone, Debug)]
struct HudLayerBloomPass {
    layer_id: HudLayerId,
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
    additive_camera: Option<Entity>,
}

impl Default for HudLayerBloomPass {
    fn default() -> Self {
        Self {
            layer_id: HudLayerId::Main,
            source_image: Handle::default(),
            blur_small_image: Handle::default(),
            blur_wide_image: Handle::default(),
            source_camera: None,
            blur_small_camera: None,
            blur_wide_camera: None,
            blur_small_quad: None,
            blur_wide_quad: None,
            composite_sprite: None,
            wide_composite_sprite: None,
            additive_camera: None,
        }
    }
}

#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct HudWidgetBloom {
    layers: BTreeMap<HudLayerId, HudLayerBloomPass>,
}

impl HudWidgetBloom {
    #[cfg(test)]
    fn pass(&self, layer_id: HudLayerId) -> Option<&HudLayerBloomPass> {
        self.layers.get(&layer_id)
    }

    fn pass_mut(&mut self, layer_id: HudLayerId) -> Option<&mut HudLayerBloomPass> {
        self.layers.get_mut(&layer_id)
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub(crate) struct HudBloomLayerConfig {
    enabled_layers: BTreeSet<HudLayerId>,
}

impl Default for HudBloomLayerConfig {
    fn default() -> Self {
        Self {
            enabled_layers: [HudLayerId::Main].into_iter().collect(),
        }
    }
}

impl HudBloomLayerConfig {
    fn enabled_layers(&self) -> &BTreeSet<HudLayerId> {
        &self.enabled_layers
    }
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

fn bloom_camera_order(layer_id: HudLayerId) -> isize {
    layer_id.bloom_order()
}

fn configured_bloom_layers(config: Option<&HudBloomLayerConfig>) -> BTreeSet<HudLayerId> {
    config
        .map(|config| config.enabled_layers().clone())
        .unwrap_or_else(|| [HudLayerId::Main].into_iter().collect())
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

fn spawn_bloom_source_camera(
    commands: &mut Commands,
    layer_id: HudLayerId,
    source_image: &Handle<Image>,
) -> Entity {
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
            HudLayerBloomOwnerMarker { layer_id },
        ))
        .id()
}

#[allow(
    clippy::too_many_arguments,
    reason = "blur-pass setup needs commands, assets, source/target images, and sizing together"
)]
fn spawn_small_blur_pass(
    commands: &mut Commands,
    layer_id: HudLayerId,
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
            HudLayerBloomOwnerMarker { layer_id },
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
            HudLayerBloomOwnerMarker { layer_id },
        ))
        .id();
    (blur_small_quad, blur_small_camera)
}

#[allow(
    clippy::too_many_arguments,
    reason = "blur-pass setup needs commands, assets, source/target images, and sizing together"
)]
fn spawn_wide_blur_pass(
    commands: &mut Commands,
    layer_id: HudLayerId,
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
            HudLayerBloomOwnerMarker { layer_id },
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
            HudLayerBloomOwnerMarker { layer_id },
        ))
        .id();
    (blur_wide_quad, blur_wide_camera)
}

fn spawn_composite_sprites(
    commands: &mut Commands,
    layer_id: HudLayerId,
    blur_small_image: &Handle<Image>,
    blur_wide_image: &Handle<Image>,
    primary_window: &Window,
) -> (Entity, Entity) {
    let composite_sprite = commands
        .spawn((
            Sprite {
                image: blur_small_image.clone(),
                color: Color::linear_rgba(SMALL_BLUR_GAIN, SMALL_BLUR_GAIN, SMALL_BLUR_GAIN, 1.0),
                custom_size: Some(Vec2::new(primary_window.width(), primary_window.height())),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, BLOOM_COMPOSITE_Z),
            RenderLayers::layer(BLOOM_COMPOSITE_LAYER),
            Visibility::Hidden,
            AgentListBloomCompositeMarker,
            HudLayerBloomOwnerMarker { layer_id },
        ))
        .id();
    let wide_composite_sprite = commands
        .spawn((
            Sprite {
                image: blur_wide_image.clone(),
                color: Color::linear_rgba(WIDE_BLUR_GAIN, WIDE_BLUR_GAIN, WIDE_BLUR_GAIN, 1.0),
                custom_size: Some(Vec2::new(primary_window.width(), primary_window.height())),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, BLOOM_COMPOSITE_Z + 0.01),
            RenderLayers::layer(BLOOM_COMPOSITE_LAYER),
            Visibility::Hidden,
            AgentListBloomWideCompositeMarker,
            HudLayerBloomOwnerMarker { layer_id },
        ))
        .id();
    (composite_sprite, wide_composite_sprite)
}

fn spawn_additive_camera(commands: &mut Commands, layer_id: HudLayerId) -> Entity {
    commands
        .spawn((
            Camera2d,
            Camera {
                order: bloom_camera_order(layer_id),
                output_mode: CameraOutputMode::Write {
                    blend_state: Some(additive_blend_state()),
                    clear_color: ClearColorConfig::None,
                },
                ..default()
            },
            RenderLayers::layer(BLOOM_COMPOSITE_LAYER),
            AgentListBloomAdditiveCameraMarker,
            HudLayerBloomOwnerMarker { layer_id },
        ))
        .id()
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
    config: Option<Res<'w, HudBloomLayerConfig>>,
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
pub(crate) fn setup_hud_widget_bloom(world: &mut World) {
    let mut state: bevy::ecs::system::SystemState<HudWidgetBloomSetupContext> =
        bevy::ecs::system::SystemState::new(world);
    let mut ctx = state.get_mut(world);
    let target_size = bloom_target_size(&ctx.primary_window);
    let target_texel_size = Vec2::new(
        1.0 / target_size.x.max(1) as f32,
        1.0 / target_size.y.max(1) as f32,
    );
    let configured_layers = configured_bloom_layers(ctx.config.as_deref());
    for layer_id in configured_layers {
        let (source_image, blur_small_image, blur_wide_image) =
            create_bloom_images(&mut ctx.images, target_size);
        let source_camera = spawn_bloom_source_camera(&mut ctx.commands, layer_id, &source_image);
        let (blur_small_quad, blur_small_camera) = spawn_small_blur_pass(
            &mut ctx.commands,
            layer_id,
            &mut ctx.meshes,
            &mut ctx.blur_materials,
            &source_image,
            &blur_small_image,
            target_size,
            target_texel_size,
        );
        let (blur_wide_quad, blur_wide_camera) = spawn_wide_blur_pass(
            &mut ctx.commands,
            layer_id,
            &mut ctx.meshes,
            &mut ctx.blur_materials,
            &source_image,
            &blur_wide_image,
            target_size,
            target_texel_size,
        );
        let (composite_sprite, wide_composite_sprite) = spawn_composite_sprites(
            &mut ctx.commands,
            layer_id,
            &blur_small_image,
            &blur_wide_image,
            &ctx.primary_window,
        );
        let additive_camera = spawn_additive_camera(&mut ctx.commands, layer_id);

        if layer_id == HudLayerId::Main {
            spawn_debug_preview_entities(
                &mut ctx.commands,
                &ctx.primary_window,
                &source_image,
                &blur_small_image,
                &blur_wide_image,
                ctx.settings.debug_previews,
            );
        }

        ctx.bloom.layers.insert(
            layer_id,
            HudLayerBloomPass {
                layer_id,
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
                additive_camera: Some(additive_camera),
            },
        );
    }
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
    visibility_policy: Res<'w, HudRenderVisibilityPolicy>,
    settings: Res<'w, HudBloomSettings>,
    config: Option<Res<'w, HudBloomLayerConfig>>,
    bloom_groups: Res<'w, super::HudBloomGroupAuthoring>,
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

fn ensure_bloom_target_images(
    images: &mut Assets<Image>,
    pass: &mut HudLayerBloomPass,
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

fn retarget_bloom_cameras(ctx: &mut HudWidgetBloomContext<'_, '_>, pass: &HudLayerBloomPass) {
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
    pass: &HudLayerBloomPass,
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

fn bloom_specs_for_sync(
    ctx: &HudWidgetBloomContext<'_, '_>,
    layer_id: HudLayerId,
) -> Vec<BloomSourceSpec> {
    let enabled = ctx
        .layout_state
        .get(HudWidgetKey::AgentList)
        .map(|module| module.shell.enabled && module.shell.current_alpha > 0.01)
        .unwrap_or(false)
        && ctx.visibility_policy.bloom_visible;
    if !enabled {
        return Vec::new();
    }
    ctx.bloom_groups
        .rects_for(layer_id, super::HudBloomGroupId::AgentListSelection)
        .map(|spec| BloomSourceSpec {
            key: AgentListBloomSourceSprite {
                layer_id,
                source_id: spec.source_id,
            },
            rect: spec.rect,
            color: spec.color,
        })
        .collect()
}

fn sync_bloom_source_sprites(
    ctx: &mut HudWidgetBloomContext<'_, '_>,
    layer_id: HudLayerId,
    specs: &[BloomSourceSpec],
    target_size: UVec2,
) {
    let mut existing = std::collections::HashMap::new();
    for (entity, marker, sprite, transform, visibility) in &mut ctx.source_sprites {
        if marker.layer_id != layer_id {
            continue;
        }
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
    pass: &HudLayerBloomPass,
    active: bool,
    bloom_ready: bool,
) {
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
}

fn sync_bloom_debug_previews(ctx: &mut HudWidgetBloomContext<'_, '_>, pass: &HudLayerBloomPass) {
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
    let configured_layers = configured_bloom_layers(ctx.config.as_deref());
    for layer_id in configured_layers {
        let pass_snapshot = {
            let Some(pass) = ctx.bloom.pass_mut(layer_id) else {
                continue;
            };
            ensure_bloom_target_images(&mut ctx.images, pass, target_size);
            pass.clone()
        };
        debug_assert_eq!(pass_snapshot.layer_id, layer_id);
        debug_assert!(pass_snapshot.additive_camera.is_some());
        retarget_bloom_cameras(&mut ctx, &pass_snapshot);
        sync_blur_quads(&mut ctx, &pass_snapshot, target_size, target_texel_size);
        let specs = bloom_specs_for_sync(&ctx, layer_id);
        sync_bloom_source_sprites(&mut ctx, layer_id, &specs, target_size);
        let bloom_ready = image_matches_size(&ctx.images, &pass_snapshot.source_image, target_size)
            && image_matches_size(&ctx.images, &pass_snapshot.blur_small_image, target_size)
            && image_matches_size(&ctx.images, &pass_snapshot.blur_wide_image, target_size);
        sync_bloom_composites(&mut ctx, &pass_snapshot, !specs.is_empty(), bloom_ready);
        if layer_id == HudLayerId::Main {
            sync_bloom_debug_previews(&mut ctx, &pass_snapshot);
        }
    }
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
