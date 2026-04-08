#[cfg(test)]
use crate::shared::readback::{align_copy_bytes_per_row, texture_bytes_to_ppm};
use crate::{
    hud::{AgentListBloomAdditiveCameraMarker, HudCompositeCameraMarker, HudModalCameraMarker},
    shared::{capture::CaptureRequestState, readback::write_texture_dump_to_path},
    terminals::TerminalCameraMarker,
    verification::{VerificationCaptureBarrierState, VerificationScenarioConfig},
};

use super::bootstrap::resolve_window_scale_factor;
use bevy::{
    app::AppExit,
    asset::RenderAssetUsages,
    camera::RenderTarget,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    },
    window::{PrimaryWindow, RequestRedraw},
};
use std::{env, path::PathBuf};

const DEFAULT_OUTPUT_WIDTH: u32 = 1920;
const DEFAULT_OUTPUT_HEIGHT: u32 = 1200;
const FINAL_FRAME_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputMode {
    Desktop,
    OffscreenVerify,
}

impl OutputMode {
    /// Returns whether this mode renders into an offscreen image instead of a real desktop window.
    ///
    /// The enum currently has only one offscreen variant, but the helper keeps the rest of the code
    /// phrased in terms of output intent rather than matching concrete variants everywhere.
    pub(crate) fn is_offscreen(self) -> bool {
        matches!(self, Self::OffscreenVerify)
    }
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct AppOutputConfig {
    pub(crate) mode: OutputMode,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale_factor_override: Option<f32>,
}

impl Default for AppOutputConfig {
    /// Provides the normal desktop-output defaults used when no environment overrides are present.
    ///
    /// The defaults intentionally bias toward an ordinary visible application window at a reasonable
    /// verification-friendly size, while leaving the scale factor unspecified so the host platform can
    /// choose it naturally.
    fn default() -> Self {
        Self {
            mode: OutputMode::Desktop,
            width: DEFAULT_OUTPUT_WIDTH,
            height: DEFAULT_OUTPUT_HEIGHT,
            scale_factor_override: None,
        }
    }
}

impl AppOutputConfig {
    /// Reads the output configuration from the `NEOZEUS_*` environment surface.
    ///
    /// Parsing is intentionally forgiving: mode and dimensions are normalized through dedicated
    /// helpers, and the window scale factor piggybacks on the shared bootstrap parser so offscreen and
    /// on-screen sizing follow the same rules.
    pub(crate) fn from_env() -> Self {
        Self {
            mode: resolve_output_mode(env::var("NEOZEUS_OUTPUT_MODE").ok().as_deref()),
            width: resolve_output_dimension(
                env::var("NEOZEUS_OFFSCREEN_WIDTH").ok().as_deref(),
                DEFAULT_OUTPUT_WIDTH,
            ),
            height: resolve_output_dimension(
                env::var("NEOZEUS_OFFSCREEN_HEIGHT").ok().as_deref(),
                DEFAULT_OUTPUT_HEIGHT,
            ),
            scale_factor_override: resolve_window_scale_factor(
                env::var("NEOZEUS_WINDOW_SCALE_FACTOR").ok().as_deref(),
            ),
        }
    }
}

/// Parses the requested output mode from an optional raw string.
///
/// The parser currently accepts both `offscreen` and `offscreen-verify` as aliases for the same
/// headless verification mode. Missing, empty, or unknown values deliberately fall back to desktop
/// mode instead of failing startup.
pub(crate) fn resolve_output_mode(raw: Option<&str>) -> OutputMode {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("offscreen") => OutputMode::OffscreenVerify,
        Some(value) if value.eq_ignore_ascii_case("offscreen-verify") => {
            OutputMode::OffscreenVerify
        }
        _ => OutputMode::Desktop,
    }
}

/// Parses a positive integer dimension while preserving a caller-supplied default on bad input.
///
/// The function trims the input, attempts `u32` parsing, rejects zero explicitly, and otherwise
/// returns `default`. That keeps environment-based configuration convenient without turning typos
/// into hard startup failures.
pub(crate) fn resolve_output_dimension(raw: Option<&str>, default: u32) -> u32 {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FinalFrameOutputTargetKind {
    #[default]
    Window,
    OffscreenImage,
}

#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct FinalFrameOutputState {
    pub(crate) target_kind: FinalFrameOutputTargetKind,
    pub(crate) target_image: Option<Handle<Image>>,
    pub(crate) size: UVec2,
}

impl FinalFrameOutputState {
    /// Test-only helper that reports whether an offscreen target image is currently allocated.
    ///
    /// The production code just inspects `target_image` directly, but tests use this named predicate
    /// to assert the state transition without depending on the resource layout.
    #[cfg(test)]
    pub(crate) fn enabled(&self) -> bool {
        self.target_kind == FinalFrameOutputTargetKind::OffscreenImage
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalFrameCaptureReadiness {
    WaitingForScenario,
    WaitingForBarrier,
    WaitingForDelay,
    WaitingForTarget,
    WaitingForImageAsset,
    Ready,
}

fn final_frame_capture_readiness(
    config: &FinalFrameCaptureConfig,
    verification_scenario: Option<&VerificationScenarioConfig>,
    verification_barrier: Option<&VerificationCaptureBarrierState>,
    output_state: &FinalFrameOutputState,
    images: &Assets<Image>,
) -> FinalFrameCaptureReadiness {
    if verification_scenario.is_some_and(|scenario| !scenario.applied) {
        return FinalFrameCaptureReadiness::WaitingForScenario;
    }
    if verification_scenario.is_some()
        && !verification_barrier.is_some_and(|barrier| barrier.ready())
    {
        return FinalFrameCaptureReadiness::WaitingForBarrier;
    }
    if config.request.delay_pending() {
        return FinalFrameCaptureReadiness::WaitingForDelay;
    }
    let Some(target_image) = output_state.target_image.as_ref() else {
        return FinalFrameCaptureReadiness::WaitingForTarget;
    };
    if images.get(target_image.id()).is_none() {
        return FinalFrameCaptureReadiness::WaitingForImageAsset;
    }
    FinalFrameCaptureReadiness::Ready
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SceneOutputTarget {
    Window,
    Image(Handle<Image>),
}

impl SceneOutputTarget {
    fn render_target(&self) -> RenderTarget {
        match self {
            Self::Window => RenderTarget::default(),
            Self::Image(image) => RenderTarget::Image(image.clone().into()),
        }
    }
}

/// Allocates the GPU image that all offscreen cameras will render into.
///
/// The image is created in the final presentation format and with the exact usage flags needed by
/// the pipeline: render attachment for drawing, copy source for readback, and texture binding so it
/// can participate in later composition if needed.
fn create_final_frame_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        FINAL_FRAME_FORMAT,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC | TextureUsages::RENDER_ATTACHMENT;
    image
}

#[derive(Resource, Clone, Debug)]
pub(crate) struct FinalFrameCaptureConfig {
    pub(crate) path: PathBuf,
    pub(crate) request: CaptureRequestState,
    pub(crate) exit_after_capture: bool,
}

impl FinalFrameCaptureConfig {
    /// Builds the final-frame capture request from environment variables, or returns `None` when
    /// capture is not configured.
    ///
    /// The presence of `NEOZEUS_CAPTURE_FINAL_FRAME_PATH` is the feature gate. Delay frames and the
    /// exit-after-capture behavior are parsed permissively so ad-hoc verification runs stay easy to
    /// configure from the shell.
    pub(crate) fn from_env() -> Option<Self> {
        // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
        let frames_until_capture = env::var("NEOZEUS_CAPTURE_FINAL_FRAME_DELAY_FRAMES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(2);
        Some(Self {
            path: PathBuf::from(env::var("NEOZEUS_CAPTURE_FINAL_FRAME_PATH").ok()?),
            request: CaptureRequestState::new(frames_until_capture),
            exit_after_capture: env::var("NEOZEUS_EXIT_AFTER_CAPTURE")
                .ok()
                .map(|value| {
                    !matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "0" | "false" | "no" | "off"
                    )
                })
                .unwrap_or(true),
        })
    }
}

#[derive(Component, Clone, Debug)]
struct FinalFrameReadbackMeta {
    pub(crate) path: PathBuf,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: TextureFormat,
}

impl FinalFrameReadbackMeta {
    /// Captures the metadata needed to write a completed GPU readback to disk later.
    ///
    /// The readback callback only receives the raw bytes and the observing entity, so width, height,
    /// format, and destination path are snapshotted here at request time and carried on the spawned
    /// entity as a component.
    fn from_image(path: PathBuf, image: &Image) -> Self {
        Self {
            path,
            width: image.texture_descriptor.size.width,
            height: image.texture_descriptor.size.height,
            format: image.texture_descriptor.format,
        }
    }
}

fn resolve_scene_output_target(
    output: &AppOutputConfig,
    primary_window: &Window,
    images: &mut Assets<Image>,
    output_state: &mut FinalFrameOutputState,
) -> SceneOutputTarget {
    if !output.mode.is_offscreen() {
        output_state.target_kind = FinalFrameOutputTargetKind::Window;
        output_state.target_image = None;
        output_state.size = UVec2::ZERO;
        return SceneOutputTarget::Window;
    }

    // The synthetic/real primary window already knows the effective physical size, including any
    // scale-factor override, so use it as the single source of truth for the offscreen image size.
    let target_size = UVec2::new(
        primary_window.physical_width().max(1),
        primary_window.physical_height().max(1),
    );
    let needs_recreate = output_state.target_image.is_none() || output_state.size != target_size;
    if needs_recreate {
        output_state.target_image = Some(images.add(create_final_frame_image(target_size)));
        output_state.size = target_size;
    }
    output_state.target_kind = FinalFrameOutputTargetKind::OffscreenImage;
    if let Some(target_image) = output_state.target_image.clone() {
        SceneOutputTarget::Image(target_image)
    } else {
        output_state.size = UVec2::ZERO;
        SceneOutputTarget::Window
    }
}

fn apply_scene_output_target(
    commands: &mut Commands,
    target: &SceneOutputTarget,
    entities: impl Iterator<Item = Entity>,
) {
    let render_target = target.render_target();
    for entity in entities {
        commands.entity(entity).insert(render_target.clone());
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "camera target routing needs output state, image assets, and multiple camera marker queries"
)]
/// Keeps every scene camera pointed at exactly one selected output target.
///
/// This system is intentionally just a target selector plus target application. The scene cameras do
/// not branch into separate visible/offscreen render pipelines; the only mode-dependent choice is
/// whether those same cameras draw into the real window or into one shared offscreen image.
pub(crate) fn sync_final_frame_output_target(
    output: Res<AppOutputConfig>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut output_state: ResMut<FinalFrameOutputState>,
    mut commands: Commands,
    terminal_cameras: Query<Entity, With<TerminalCameraMarker>>,
    composite_cameras: Query<Entity, With<HudCompositeCameraMarker>>,
    bloom_additive_cameras: Query<Entity, With<AgentListBloomAdditiveCameraMarker>>,
    modal_cameras: Query<Entity, With<HudModalCameraMarker>>,
) {
    let target =
        resolve_scene_output_target(&output, &primary_window, &mut images, &mut output_state);
    apply_scene_output_target(
        &mut commands,
        &target,
        terminal_cameras
            .iter()
            .chain(composite_cameras.iter())
            .chain(bloom_additive_cameras.iter())
            .chain(modal_cameras.iter()),
    );
}

/// Drives the "capture the next finished offscreen frame" state machine.
///
/// The system keeps requesting redraws until capture can happen, waits for any verification scenario
/// to finish applying, honors the configured frame delay, and then spawns a GPU readback observer on
/// the current target image. If the image resource does not exist yet, it logs the wait and retries
/// on later frames instead of failing.
pub(crate) fn request_final_frame_capture(
    mut commands: Commands,
    config: Option<ResMut<FinalFrameCaptureConfig>>,
    verification_scenario: Option<Res<VerificationScenarioConfig>>,
    verification_barrier: Option<Res<VerificationCaptureBarrierState>>,
    output_state: Res<FinalFrameOutputState>,
    images: Res<Assets<Image>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let Some(mut config) = config else {
        return;
    };
    if config.request.completed() {
        return;
    }
    redraws.write(RequestRedraw);
    if config.request.requested() {
        return;
    }
    match final_frame_capture_readiness(
        &config,
        verification_scenario.as_deref(),
        verification_barrier.as_deref(),
        &output_state,
        &images,
    ) {
        FinalFrameCaptureReadiness::WaitingForScenario
        | FinalFrameCaptureReadiness::WaitingForBarrier => return,
        FinalFrameCaptureReadiness::WaitingForDelay => {
            let _ = config.request.wait_delay();
            return;
        }
        FinalFrameCaptureReadiness::WaitingForTarget => {
            crate::terminals::append_debug_log("final frame capture waiting for target image");
            return;
        }
        FinalFrameCaptureReadiness::WaitingForImageAsset => {
            crate::terminals::append_debug_log(
                "final frame capture waiting for target image asset",
            );
            return;
        }
        FinalFrameCaptureReadiness::Ready => {}
    }
    let Some(target_image) = output_state.target_image.clone() else {
        return;
    };
    let Some(image) = images.get(target_image.id()) else {
        return;
    };
    crate::terminals::append_debug_log(format!(
        "final frame capture requested path={} size={}x{} format={:?}",
        config.path.display(),
        image.texture_descriptor.size.width,
        image.texture_descriptor.size.height,
        image.texture_descriptor.format,
    ));
    commands
        .spawn((
            Readback::texture(target_image),
            FinalFrameReadbackMeta::from_image(config.path.clone(), image),
        ))
        .observe(handle_final_frame_capture_complete);
    config.request.mark_requested();
}

fn handle_final_frame_capture_complete(
    event: On<ReadbackComplete>,
    metas: Query<&FinalFrameReadbackMeta>,
    mut exits: MessageWriter<AppExit>,
    config: Option<ResMut<FinalFrameCaptureConfig>>,
) {
    let Ok(meta) = metas.get(event.entity) else {
        return;
    };
    if let Err(error) = write_texture_dump_to_path(
        &meta.path,
        meta.width,
        meta.height,
        meta.format,
        &event.data,
        "final frame",
    ) {
        crate::terminals::append_debug_log(format!(
            "final frame capture write failed path={} error={error}",
            meta.path.display()
        ));
    } else {
        crate::terminals::append_debug_log(format!(
            "final frame capture wrote {}",
            meta.path.display()
        ));
    }
    if let Some(mut config) = config {
        let exit_after_capture = config.exit_after_capture;
        config.request.mark_completed();
        if exit_after_capture {
            exits.write(AppExit::Success);
        }
    }
}

/// Exposes the production final-frame texture format to tests.
///
/// The constant itself is private to this module, but tests need a stable way to assert the helper
/// uses the same format the runtime capture path expects.
#[cfg(test)]
fn final_frame_format() -> TextureFormat {
    FINAL_FRAME_FORMAT
}

#[cfg(test)]
mod tests;
