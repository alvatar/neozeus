use crate::{
    hud::{AgentListBloomAdditiveCameraMarker, HudCompositeCameraMarker, HudModalCameraMarker},
    terminals::TerminalCameraMarker,
    verification::VerificationScenarioConfig,
};
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

const DEFAULT_OUTPUT_WIDTH: u32 = 1400;
const DEFAULT_OUTPUT_HEIGHT: u32 = 900;
const FINAL_FRAME_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputMode {
    Desktop,
    OffscreenVerify,
}

impl OutputMode {
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
            scale_factor_override: crate::app::resolve_window_scale_factor(
                env::var("NEOZEUS_WINDOW_SCALE_FACTOR").ok().as_deref(),
            ),
        }
    }
}

pub(crate) fn resolve_output_mode(raw: Option<&str>) -> OutputMode {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("offscreen") => OutputMode::OffscreenVerify,
        Some(value) if value.eq_ignore_ascii_case("offscreen-verify") => {
            OutputMode::OffscreenVerify
        }
        _ => OutputMode::Desktop,
    }
}

pub(crate) fn resolve_output_dimension(raw: Option<&str>, default: u32) -> u32 {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct FinalFrameOutputState {
    pub(crate) target_image: Option<Handle<Image>>,
    pub(crate) size: UVec2,
}

impl FinalFrameOutputState {
    #[cfg(test)]
    pub(crate) fn enabled(&self) -> bool {
        self.target_image.is_some()
    }
}

pub(crate) fn create_final_frame_image(size: UVec2) -> Image {
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
    pub(crate) frames_until_capture: u32,
    pub(crate) requested: bool,
    pub(crate) completed: bool,
    pub(crate) exit_after_capture: bool,
}

impl FinalFrameCaptureConfig {
    pub(crate) fn from_env() -> Option<Self> {
        Some(Self {
            path: PathBuf::from(env::var("NEOZEUS_CAPTURE_FINAL_FRAME_PATH").ok()?),
            frames_until_capture: env::var("NEOZEUS_CAPTURE_FINAL_FRAME_DELAY_FRAMES")
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(2),
            requested: false,
            completed: false,
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
pub(crate) struct FinalFrameReadbackMeta {
    pub(crate) path: PathBuf,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: TextureFormat,
}

impl FinalFrameReadbackMeta {
    pub(crate) fn from_image(path: PathBuf, image: &Image) -> Self {
        Self {
            path,
            width: image.texture_descriptor.size.width,
            height: image.texture_descriptor.size.height,
            format: image.texture_descriptor.format,
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "camera target routing needs output state, image assets, and multiple camera marker queries"
)]
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
    if !output.mode.is_offscreen() {
        if output_state.target_image.take().is_some() {
            output_state.size = UVec2::ZERO;
        }
        for entity in terminal_cameras
            .iter()
            .chain(composite_cameras.iter())
            .chain(bloom_additive_cameras.iter())
            .chain(modal_cameras.iter())
        {
            commands.entity(entity).remove::<RenderTarget>();
        }
        return;
    }

    let target_size = UVec2::new(
        primary_window.physical_width().max(1),
        primary_window.physical_height().max(1),
    );
    let needs_recreate = output_state.target_image.is_none() || output_state.size != target_size;
    if needs_recreate {
        output_state.target_image = Some(images.add(create_final_frame_image(target_size)));
        output_state.size = target_size;
    }
    let Some(target_image) = output_state.target_image.clone() else {
        return;
    };

    for entity in terminal_cameras.iter() {
        commands
            .entity(entity)
            .insert(RenderTarget::Image(target_image.clone().into()));
    }
    for entity in composite_cameras.iter() {
        commands
            .entity(entity)
            .insert(RenderTarget::Image(target_image.clone().into()));
    }
    for entity in bloom_additive_cameras.iter() {
        commands
            .entity(entity)
            .insert(RenderTarget::Image(target_image.clone().into()));
    }
    for entity in modal_cameras.iter() {
        commands
            .entity(entity)
            .insert(RenderTarget::Image(target_image.clone().into()));
    }
}

pub(crate) fn request_final_frame_capture(
    mut commands: Commands,
    config: Option<ResMut<FinalFrameCaptureConfig>>,
    verification_scenario: Option<Res<VerificationScenarioConfig>>,
    output_state: Res<FinalFrameOutputState>,
    images: Res<Assets<Image>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let Some(mut config) = config else {
        return;
    };
    if config.completed {
        return;
    }
    if verification_scenario.is_some_and(|scenario| !scenario.applied) {
        redraws.write(RequestRedraw);
        return;
    }
    redraws.write(RequestRedraw);
    if config.requested {
        return;
    }
    if config.frames_until_capture > 0 {
        config.frames_until_capture -= 1;
        return;
    }
    let Some(target_image) = output_state.target_image.clone() else {
        crate::terminals::append_debug_log("final frame capture waiting for target image");
        return;
    };
    let Some(image) = images.get(target_image.id()) else {
        crate::terminals::append_debug_log("final frame capture waiting for target image asset");
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
    config.requested = true;
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
    if let Err(error) = write_texture_dump(meta, &event.data) {
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
        config.completed = true;
        if exit_after_capture {
            exits.write(AppExit::Success);
        }
    }
}

fn write_texture_dump(meta: &FinalFrameReadbackMeta, bytes: &[u8]) -> Result<(), String> {
    let ppm = texture_bytes_to_ppm(meta.width, meta.height, meta.format, bytes)?;
    std::fs::write(&meta.path, ppm)
        .map_err(|error| format!("failed to write {}: {error}", meta.path.display()))
}

fn texture_bytes_to_ppm(
    width: u32,
    height: u32,
    format: TextureFormat,
    bytes: &[u8],
) -> Result<Vec<u8>, String> {
    let pixel_size = match format {
        TextureFormat::Rgba8Unorm
        | TextureFormat::Rgba8UnormSrgb
        | TextureFormat::Bgra8Unorm
        | TextureFormat::Bgra8UnormSrgb => 4usize,
        other => return Err(format!("unsupported final frame format: {other:?}")),
    };
    let packed_row_bytes = width as usize * pixel_size;
    let aligned_row_bytes = if height > 1 {
        align_copy_bytes_per_row(packed_row_bytes)
    } else {
        packed_row_bytes
    };
    let expected_len = aligned_row_bytes * height as usize;
    if bytes.len() < expected_len {
        return Err(format!(
            "short readback buffer: got {}, expected at least {}",
            bytes.len(),
            expected_len
        ));
    }

    let mut ppm = format!("P6\n{} {}\n255\n", width, height).into_bytes();
    for row in bytes.chunks_exact(aligned_row_bytes).take(height as usize) {
        for pixel in row[..packed_row_bytes].chunks_exact(pixel_size) {
            match format {
                TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => {
                    ppm.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(ppm)
}

fn align_copy_bytes_per_row(value: usize) -> usize {
    const ALIGNMENT: usize = 256;
    (value + (ALIGNMENT - 1)) & !(ALIGNMENT - 1)
}

#[cfg(test)]
pub(crate) fn final_frame_format() -> TextureFormat {
    FINAL_FRAME_FORMAT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::{AgentListBloomAdditiveCameraMarker, HudCompositeCameraMarker};
    use bevy::{
        ecs::system::RunSystemOnce,
        render::{gpu_readback::Readback, render_resource::TextureFormat},
    };

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

    #[test]
    fn offscreen_window_config_is_hidden_and_windowed() {
        let output = AppOutputConfig {
            mode: OutputMode::OffscreenVerify,
            width: 1600,
            height: 1000,
            scale_factor_override: Some(1.5),
        };
        let window = crate::scene::primary_window_config_for(&output);
        assert!(!window.visible);
        assert!(!window.decorations);
        assert!(!window.focused);
        assert_eq!(window.mode, bevy::window::WindowMode::Windowed);
        assert_eq!(window.physical_width(), 1600);
        assert_eq!(window.physical_height(), 1000);
        assert_eq!(window.resolution.scale_factor_override(), Some(1.5));
    }

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
    fn sync_final_frame_output_target_assigns_targets_only_in_offscreen_mode() {
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
        let composite = world.spawn((HudCompositeCameraMarker,)).id();
        let bloom = world.spawn((AgentListBloomAdditiveCameraMarker,)).id();

        world
            .run_system_once(sync_final_frame_output_target)
            .unwrap();

        let output = world.resource::<FinalFrameOutputState>();
        assert!(output.enabled());
        assert!(world.get::<RenderTarget>(terminal).is_some());
        assert!(world.get::<RenderTarget>(composite).is_some());
        assert!(world.get::<RenderTarget>(bloom).is_some());

        world.resource_mut::<AppOutputConfig>().mode = OutputMode::Desktop;
        world
            .run_system_once(sync_final_frame_output_target)
            .unwrap();
        assert!(world.get::<RenderTarget>(terminal).is_none());
        assert!(world.get::<RenderTarget>(composite).is_none());
        assert!(world.get::<RenderTarget>(bloom).is_none());
    }

    #[test]
    fn final_frame_capture_waits_for_target_before_requesting_readback() {
        let mut world = World::default();
        world.insert_resource(FinalFrameCaptureConfig {
            path: PathBuf::from("/tmp/final-frame-test.ppm"),
            frames_until_capture: 0,
            requested: false,
            completed: false,
            exit_after_capture: false,
        });
        world.insert_resource(FinalFrameOutputState::default());
        world.insert_resource(Assets::<Image>::default());
        world.init_resource::<Messages<RequestRedraw>>();

        world.run_system_once(request_final_frame_capture).unwrap();
        assert_eq!(world.query::<&Readback>().iter(&world).count(), 0);
        assert!(!world.resource::<FinalFrameCaptureConfig>().requested);
    }

    #[test]
    fn final_frame_capture_waits_for_verification_scenario_to_finish() {
        let mut world = World::default();
        world.insert_resource(FinalFrameCaptureConfig {
            path: PathBuf::from("/tmp/final-frame-test.ppm"),
            frames_until_capture: 0,
            requested: false,
            completed: false,
            exit_after_capture: false,
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
        assert!(!world.resource::<FinalFrameCaptureConfig>().requested);
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
        let ppm = texture_bytes_to_ppm(width, height, TextureFormat::Rgba8Unorm, &bytes).unwrap();
        assert_eq!(&ppm[..11], b"P6\n2 2\n255\n");
        assert_eq!(
            &ppm[11..],
            &[225, 129, 10, 25, 215, 189, 0, 0, 0, 255, 255, 255]
        );
    }
}
