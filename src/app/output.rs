#[cfg(test)]
use crate::shared::readback::{align_copy_bytes_per_row, texture_bytes_to_ppm};
use crate::{
    hud::{AgentListBloomAdditiveCameraMarker, HudCompositeCameraMarker},
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
    pub(crate) exit_after_completion_frames_remaining: u8,
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
            exit_after_completion_frames_remaining: 0,
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
) {
    let target =
        resolve_scene_output_target(&output, &primary_window, &mut images, &mut output_state);
    apply_scene_output_target(
        &mut commands,
        &target,
        terminal_cameras
            .iter()
            .chain(composite_cameras.iter())
            .chain(bloom_additive_cameras.iter()),
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
    mut commands: Commands,
    mut redraws: MessageWriter<RequestRedraw>,
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
    commands.entity(event.entity).despawn();
    if let Some(mut config) = config {
        config.request.mark_completed();
        if config.exit_after_capture {
            config.exit_after_completion_frames_remaining = 2;
            redraws.write(RequestRedraw);
        }
    }
}

pub(crate) fn finalize_final_frame_capture(
    config: Option<ResMut<FinalFrameCaptureConfig>>,
    mut exits: MessageWriter<AppExit>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let Some(mut config) = config else {
        return;
    };
    if !config.request.completed() || !config.exit_after_capture {
        return;
    }
    if config.exit_after_completion_frames_remaining == 0 {
        return;
    }
    redraws.write(RequestRedraw);
    config.exit_after_completion_frames_remaining -= 1;
    if config.exit_after_completion_frames_remaining == 0 {
        exits.write(AppExit::Success);
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
mod tests {
    use super::*;
    use super::super::bootstrap::primary_window_config_for;
    use crate::hud::{
        AgentListBloomAdditiveCameraMarker, HudCompositeCameraMarker, HudCompositeLayerId,
    };
    use bevy::{
        ecs::system::RunSystemOnce,
        render::{gpu_readback::Readback, render_resource::TextureFormat},
    };

    /// Covers the permissive parsing rules for offscreen output configuration.
    ///
    /// The test checks both halves of the parser surface: output mode selection and numeric dimension
    /// parsing. It verifies that recognized offscreen aliases map to `OffscreenVerify`, while empty,
    /// zero, and malformed dimension inputs fall back to the supplied defaults instead of failing.
    #[test]
    fn parses_output_mode_and_dimensions() {
        assert_eq!(resolve_output_mode(None), OutputMode::Desktop);
        assert_eq!(resolve_output_mode(Some("")), OutputMode::Desktop);
        assert_eq!(
            resolve_output_mode(Some("offscreen")),
            OutputMode::OffscreenVerify
        );
        assert_eq!(
            resolve_output_mode(Some("offscreen-verify")),
            OutputMode::OffscreenVerify
        );
        assert_eq!(resolve_output_dimension(None, 12), 12);
        assert_eq!(resolve_output_dimension(Some(""), 12), 12);
        assert_eq!(resolve_output_dimension(Some("1200"), 12), 1200);
        assert_eq!(resolve_output_dimension(Some("0"), 12), 12);
        assert_eq!(resolve_output_dimension(Some("abc"), 12), 12);
    }

    /// Verifies the synthetic primary-window shape used in offscreen mode.
    ///
    /// Offscreen rendering still needs a logical `PrimaryWindow`, but it must not behave like a real
    /// desktop window. The assertions check the important contract: hidden, undecorated, unfocused,
    /// forced to `Windowed`, and honoring the explicit physical size and scale-factor override.
    #[test]
    fn offscreen_window_config_is_hidden_and_windowed() {
        let output = AppOutputConfig {
            mode: OutputMode::OffscreenVerify,
            width: 1600,
            height: 1000,
            scale_factor_override: Some(1.5),
        };
        let window = primary_window_config_for(&output);
        assert!(!window.visible);
        assert!(!window.decorations);
        assert!(!window.focused);
        assert_eq!(window.mode, bevy::window::WindowMode::Windowed);
        assert_eq!(window.physical_width(), 1600);
        assert_eq!(window.physical_height(), 1000);
        assert_eq!(window.resolution.scale_factor_override(), Some(1.5));
    }

    /// Checks that the final-frame target image is created with the exact usage flags the render path
    /// needs.
    ///
    /// The offscreen capture pipeline relies on one image serving as a render attachment, a readback
    /// source, and a bindable texture. This test locks that contract down and also verifies that the
    /// helper preserves the requested dimensions and uses the expected final-frame format.
    #[test]
    fn create_final_frame_image_uses_renderable_srgb_target() {
        let image = create_final_frame_image(UVec2::new(1920, 1080));
        assert_eq!(image.texture_descriptor.format, final_frame_format());
        assert_eq!(image.texture_descriptor.size.width, 1920);
        assert_eq!(image.texture_descriptor.size.height, 1080);
        let usage = image.texture_descriptor.usage;
        assert!(usage.contains(TextureUsages::RENDER_ATTACHMENT));
        assert!(usage.contains(TextureUsages::COPY_SRC));
        assert!(usage.contains(TextureUsages::TEXTURE_BINDING));
    }

    #[test]
    fn resolve_scene_output_target_only_switches_between_window_and_single_image() {
        let mut images = Assets::<Image>::default();
        let mut output_state = FinalFrameOutputState::default();
        let window = primary_window_config_for(&AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1400,
            height: 900,
            scale_factor_override: None,
        });

        let desktop_target = resolve_scene_output_target(
            &AppOutputConfig {
                mode: OutputMode::Desktop,
                width: 1400,
                height: 900,
                scale_factor_override: None,
            },
            &window,
            &mut images,
            &mut output_state,
        );
        assert!(matches!(desktop_target, SceneOutputTarget::Window));
        assert!(!output_state.enabled());

        let offscreen_target = resolve_scene_output_target(
            &AppOutputConfig {
                mode: OutputMode::OffscreenVerify,
                width: 1400,
                height: 900,
                scale_factor_override: None,
            },
            &window,
            &mut images,
            &mut output_state,
        );
        let first_image = match offscreen_target {
            SceneOutputTarget::Image(handle) => handle,
            SceneOutputTarget::Window => panic!("offscreen mode must resolve to image target"),
        };
        assert!(output_state.enabled());

        let reused_target = resolve_scene_output_target(
            &AppOutputConfig {
                mode: OutputMode::OffscreenVerify,
                width: 1400,
                height: 900,
                scale_factor_override: None,
            },
            &window,
            &mut images,
            &mut output_state,
        );
        match reused_target {
            SceneOutputTarget::Image(handle) => assert_eq!(handle, first_image),
            SceneOutputTarget::Window => panic!("offscreen mode must keep image target"),
        }
    }

    /// Exercises render-target routing as the app flips between offscreen and desktop modes.
    ///
    /// The test first confirms that offscreen mode allocates a shared image target and attaches it to
    /// the terminal, composite, and bloom cameras. It then flips back to desktop mode and verifies that
    /// those cameras are returned to normal window rendering instead of keeping the stale image target.
    #[test]
    fn sync_final_frame_output_target_assigns_targets_only_in_offscreen_mode() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let mut world = World::default();
        world.insert_resource(AppOutputConfig {
            mode: OutputMode::OffscreenVerify,
            width: 1400,
            height: 900,
            scale_factor_override: None,
        });
        world.insert_resource(FinalFrameOutputState::default());
        world.insert_resource(Assets::<Image>::default());
        world.spawn((Window::default(), PrimaryWindow));
        let terminal = world.spawn((TerminalCameraMarker,)).id();
        let composite = world
            .spawn((HudCompositeCameraMarker {
                id: HudCompositeLayerId::Main,
            },))
            .id();
        let overlay = world
            .spawn((HudCompositeCameraMarker {
                id: HudCompositeLayerId::Overlay,
            },))
            .id();
        let modal = world
            .spawn((HudCompositeCameraMarker {
                id: HudCompositeLayerId::Modal,
            },))
            .id();
        let bloom = world.spawn((AgentListBloomAdditiveCameraMarker,)).id();

        world
            .run_system_once(sync_final_frame_output_target)
            .unwrap();

        let output = world.resource::<FinalFrameOutputState>();
        assert!(output.enabled());
        assert!(world.get::<RenderTarget>(terminal).is_some());
        assert!(world.get::<RenderTarget>(composite).is_some());
        assert!(world.get::<RenderTarget>(bloom).is_some());
        assert!(world.get::<RenderTarget>(overlay).is_some());
        assert!(world.get::<RenderTarget>(modal).is_some());

        world.resource_mut::<AppOutputConfig>().mode = OutputMode::Desktop;
        world
            .run_system_once(sync_final_frame_output_target)
            .unwrap();
        assert!(matches!(
            world.get::<RenderTarget>(terminal),
            Some(RenderTarget::Window(_))
        ));
        assert!(matches!(
            world.get::<RenderTarget>(composite),
            Some(RenderTarget::Window(_))
        ));
        assert!(matches!(
            world.get::<RenderTarget>(bloom),
            Some(RenderTarget::Window(_))
        ));
        assert!(matches!(
            world.get::<RenderTarget>(overlay),
            Some(RenderTarget::Window(_))
        ));
        assert!(matches!(
            world.get::<RenderTarget>(modal),
            Some(RenderTarget::Window(_))
        ));
    }

    #[test]
    fn sync_final_frame_output_target_only_retargets_existing_scene_cameras() {
        let mut world = World::default();
        world.insert_resource(AppOutputConfig {
            mode: OutputMode::OffscreenVerify,
            width: 1400,
            height: 900,
            scale_factor_override: None,
        });
        world.insert_resource(FinalFrameOutputState::default());
        world.insert_resource(Assets::<Image>::default());
        world.spawn((Window::default(), PrimaryWindow));
        world.spawn((TerminalCameraMarker,));
        world.spawn((HudCompositeCameraMarker {
            id: HudCompositeLayerId::Main,
        },));
        world.spawn((HudCompositeCameraMarker {
            id: HudCompositeLayerId::Overlay,
        },));
        world.spawn((HudCompositeCameraMarker {
            id: HudCompositeLayerId::Modal,
        },));
        world.spawn((AgentListBloomAdditiveCameraMarker,));
        let entity_count_before = world.entities().len();

        world
            .run_system_once(sync_final_frame_output_target)
            .unwrap();

        assert_eq!(world.entities().len(), entity_count_before);
    }

    /// Verifies that capture does not request GPU readback before an output target exists.
    ///
    /// This is an important guard because the capture system runs in the normal frame loop and can wake
    /// up before `sync_final_frame_output_target` has created the image. The expected behavior is to do
    /// nothing and leave the capture request pending.
    #[test]
    fn final_frame_capture_waits_for_target_before_requesting_readback() {
        let mut world = World::default();
        world.insert_resource(FinalFrameCaptureConfig {
            path: PathBuf::from("/tmp/final-frame-test.ppm"),
            request: CaptureRequestState::new(0),
            exit_after_capture: false,
            exit_after_completion_frames_remaining: 0,
        });
        world.insert_resource(FinalFrameOutputState::default());
        world.insert_resource(Assets::<Image>::default());
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(request_final_frame_capture).unwrap();
        assert_eq!(world.query::<&Readback>().iter(&world).count(), 0);
        assert!(!world
            .resource::<FinalFrameCaptureConfig>()
            .request
            .requested());
    }

    /// Verifies that final-frame capture stays blocked until a verification scenario finishes staging
    /// the scene.
    ///
    /// Without this gate the capture path could snapshot an intermediate frame before the deterministic
    /// verification setup has been applied. The test confirms that an unapplied scenario prevents any
    /// readback request from being spawned.
    #[test]
    fn final_frame_capture_waits_for_verification_scenario_to_finish() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let mut world = World::default();
        world.insert_resource(FinalFrameCaptureConfig {
            path: PathBuf::from("/tmp/final-frame-test.ppm"),
            request: CaptureRequestState::new(0),
            exit_after_capture: false,
            exit_after_completion_frames_remaining: 0,
        });
        world.insert_resource(VerificationScenarioConfig {
            scenario: crate::verification::VerificationScenario::AgentListBloom,
            frames_until_apply: 0,
            primed: false,
            applied: false,
            terminal_ids: Vec::new(),
        });
        world.insert_resource(FinalFrameOutputState::default());
        world.insert_resource(Assets::<Image>::default());
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(request_final_frame_capture).unwrap();
        assert_eq!(world.query::<&Readback>().iter(&world).count(), 0);
        assert!(!world
            .resource::<FinalFrameCaptureConfig>()
            .request
            .requested());
    }

    #[test]
    fn final_frame_capture_waits_for_verification_capture_barrier() {
        let mut world = World::default();
        world.insert_resource(FinalFrameCaptureConfig {
            path: PathBuf::from("/tmp/final-frame-test.ppm"),
            request: CaptureRequestState::new(0),
            exit_after_capture: false,
            exit_after_completion_frames_remaining: 0,
        });
        world.insert_resource(VerificationScenarioConfig {
            scenario: crate::verification::VerificationScenario::WorkingStateWorking,
            frames_until_apply: 0,
            primed: false,
            applied: true,
            terminal_ids: vec![crate::terminals::TerminalId(1)],
        });
        world.insert_resource(crate::verification::VerificationCaptureBarrierState::default());
        let mut images = Assets::<Image>::default();
        let target = images.add(create_final_frame_image(UVec2::new(8, 8)));
        world.insert_resource(FinalFrameOutputState {
            target_kind: FinalFrameOutputTargetKind::OffscreenImage,
            target_image: Some(target),
            size: UVec2::new(8, 8),
        });
        world.insert_resource(images);
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(request_final_frame_capture).unwrap();

        assert_eq!(world.query::<&Readback>().iter(&world).count(), 0);
        assert!(!world
            .resource::<FinalFrameCaptureConfig>()
            .request
            .requested());
    }

    /// Verifies that RGBA texture dumps ignore GPU row padding when producing the PPM payload.
    ///
    /// Readback buffers are aligned per row, so the helper must skip the padded bytes between logical
    /// rows. This test seeds a tiny 2×2 buffer with padding and checks that the output PPM contains only
    /// the logical RGB pixels in the expected order.
    #[test]
    fn finalize_final_frame_capture_waits_two_frames_before_exit() {
        let mut world = World::default();
        world.insert_resource(FinalFrameCaptureConfig {
            path: PathBuf::from("/tmp/final-frame-test.ppm"),
            request: {
                let mut request = CaptureRequestState::new(0);
                request.mark_completed();
                request
            },
            exit_after_capture: true,
            exit_after_completion_frames_remaining: 2,
        });
        world.init_resource::<Messages<AppExit>>();
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(finalize_final_frame_capture).unwrap();
        assert_eq!(world.resource::<Messages<AppExit>>().len(), 0);
        assert_eq!(
            world
                .resource::<FinalFrameCaptureConfig>()
                .exit_after_completion_frames_remaining,
            1
        );

        world.run_system_once(finalize_final_frame_capture).unwrap();
        assert_eq!(world.resource::<Messages<AppExit>>().len(), 1);
        assert_eq!(
            world
                .resource::<FinalFrameCaptureConfig>()
                .exit_after_completion_frames_remaining,
            0
        );
    }

    #[test]
    fn texture_dump_skips_row_padding_for_rgba() {
        let width = 2;
        let height = 2;
        let row_bytes = width as usize * 4;
        let aligned = align_copy_bytes_per_row(row_bytes);
        let mut bytes = vec![0u8; aligned * height as usize];
        bytes[..8].copy_from_slice(&[225, 129, 10, 255, 25, 215, 189, 255]);
        bytes[aligned..aligned + 8].copy_from_slice(&[0, 0, 0, 255, 255, 255, 255, 255]);
        let ppm = texture_bytes_to_ppm(
            width,
            height,
            TextureFormat::Rgba8Unorm,
            &bytes,
            "final frame",
        )
        .unwrap();
        assert_eq!(&ppm[..11], b"P6\n2 2\n255\n");
        assert_eq!(
            &ppm[11..],
            &[225, 129, 10, 25, 215, 189, 0, 0, 0, 255, 255, 255]
        );
    }
}
