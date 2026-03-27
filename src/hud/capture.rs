use std::{env, fs, path::PathBuf};

use bevy::{
    app::AppExit,
    asset::RenderAssetUsages,
    camera::RenderTarget,
    image::ImageSampler,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        view::screenshot::{save_to_disk, Capturing, Screenshot},
    },
    sprite_render::MeshMaterial2d,
    window::{PrimaryWindow, RequestRedraw},
};
use bevy_vello::render::VelloCanvasMaterial;

use crate::hud::compositor::{HudCompositeCameraMarker, HudCompositeLayerMarker};

#[derive(Resource, Clone, Debug)]
pub(crate) struct HudTextureCaptureConfig {
    path: PathBuf,
    frames_until_capture: u32,
    requested: bool,
    completed: bool,
}

impl HudTextureCaptureConfig {
    /// Reads the HUD-texture capture configuration from the environment.
    ///
    /// The capture is enabled only when a destination path is provided; the optional frame delay lets
    /// the caller wait a couple of frames before readback.
    pub(crate) fn from_env() -> Option<Self> {
        let path = env::var("NEOZEUS_CAPTURE_HUD_TEXTURE_PATH").ok()?;
        let frames_until_capture = env::var("NEOZEUS_CAPTURE_HUD_TEXTURE_DELAY_FRAMES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(2);
        Some(Self {
            path: PathBuf::from(path),
            frames_until_capture,
            requested: false,
            completed: false,
        })
    }
}

#[derive(Resource, Clone, Debug)]
pub(crate) struct WindowCaptureConfig {
    path: PathBuf,
    frames_until_capture: u32,
    requested: bool,
    completed: bool,
}

impl WindowCaptureConfig {
    /// Reads the full-window screenshot capture configuration from the environment.
    ///
    /// As with the other capture configs, the path opt-in enables the feature and the optional frame
    /// delay postpones the screenshot request.
    pub(crate) fn from_env() -> Option<Self> {
        let path = env::var("NEOZEUS_CAPTURE_WINDOW_PATH").ok()?;
        let frames_until_capture = env::var("NEOZEUS_CAPTURE_WINDOW_DELAY_FRAMES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(2);
        Some(Self {
            path: PathBuf::from(path),
            frames_until_capture,
            requested: false,
            completed: false,
        })
    }
}

#[derive(Resource, Clone, Debug)]
pub(crate) struct HudCompositeCaptureConfig {
    path: PathBuf,
    frames_until_capture: u32,
    armed: bool,
    requested: bool,
    completed: bool,
    target_image: Option<Handle<Image>>,
}

impl HudCompositeCaptureConfig {
    /// Reads the HUD-composite capture configuration from the environment.
    ///
    /// Composite capture has a slightly richer state machine than plain HUD texture capture, but the
    /// opt-in environment surface is still just path plus optional frame delay.
    pub(crate) fn from_env() -> Option<Self> {
        let path = env::var("NEOZEUS_CAPTURE_HUD_COMPOSITE_PATH").ok()?;
        let frames_until_capture = env::var("NEOZEUS_CAPTURE_HUD_COMPOSITE_DELAY_FRAMES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(2);
        Some(Self {
            path: PathBuf::from(path),
            frames_until_capture,
            armed: false,
            requested: false,
            completed: false,
            target_image: None,
        })
    }
}

#[derive(Component, Clone, Debug)]
struct HudTextureReadbackMeta {
    path: PathBuf,
    width: u32,
    height: u32,
    format: TextureFormat,
}

impl HudTextureReadbackMeta {
    /// Snapshots the metadata needed to turn a later GPU readback into a file on disk.
    ///
    /// Width, height, format, and output path are stored on the spawned readback entity because the
    /// readback callback itself only receives raw bytes and the entity id.
    fn from_image(path: PathBuf, image: &Image) -> Self {
        Self {
            path,
            width: image.texture_descriptor.size.width,
            height: image.texture_descriptor.size.height,
            format: image.texture_descriptor.format,
        }
    }
}

/// Allocates the temporary render target used when capturing the composited HUD layer.
///
/// Unlike the bloom pipeline's float targets, this capture target is an 8-bit RGBA image intended
/// purely for readback and optional sampling by the compositor camera.
fn composite_capture_target_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    image
}

/// Advances the HUD-composite capture state machine and spawns a readback request when ready.
///
/// The first stage allocates and attaches a dedicated target image to the composite camera. After
/// that, the system waits until the composite layer is actually visible, counts down the requested
/// delay, arms itself for one more frame, and only then spawns the GPU readback entity.
pub(crate) fn request_hud_composite_capture(
    mut commands: Commands,
    config: Option<ResMut<HudCompositeCaptureConfig>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    composite_cameras: Query<Entity, With<HudCompositeCameraMarker>>,
    composite_layers: Query<&Visibility, With<HudCompositeLayerMarker>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let Some(mut config) = config else {
        return;
    };
    if config.completed {
        return;
    }
    redraws.write(RequestRedraw);

    let physical_size = primary_window.physical_size();
    if config.target_image.is_none() {
        let Some(camera_entity) = composite_cameras.iter().next() else {
            crate::terminals::append_debug_log(
                "hud composite capture waiting for composite camera",
            );
            return;
        };
        let image_handle = images.add(composite_capture_target_image(physical_size));
        commands
            .entity(camera_entity)
            .insert(RenderTarget::Image(image_handle.clone().into()));
        config.target_image = Some(image_handle);
        crate::terminals::append_debug_log(format!(
            "hud composite capture target initialized path={} size={}x{} camera={}",
            config.path.display(),
            physical_size.x,
            physical_size.y,
            camera_entity.index(),
        ));
        return;
    }

    if composite_cameras.is_empty() {
        return;
    }
    let composite_visible = composite_layers
        .iter()
        .any(|visibility| *visibility == Visibility::Visible);
    if !composite_visible {
        crate::terminals::append_debug_log(
            "hud composite capture waiting for visible composite layer",
        );
        return;
    }
    if config.requested {
        return;
    }
    if !config.armed {
        if config.frames_until_capture > 0 {
            config.frames_until_capture -= 1;
            return;
        }
        config.armed = true;
        crate::terminals::append_debug_log("hud composite capture armed");
        return;
    }

    let Some(target_image) = config.target_image.clone() else {
        return;
    };
    let Some(image) = images.get(target_image.id()) else {
        return;
    };
    crate::terminals::append_debug_log(format!(
        "hud composite capture requested path={} size={}x{} format={:?}",
        config.path.display(),
        image.texture_descriptor.size.width,
        image.texture_descriptor.size.height,
        image.texture_descriptor.format,
    ));
    commands
        .spawn((
            Readback::texture(target_image),
            HudTextureReadbackMeta::from_image(config.path.clone(), image),
        ))
        .observe(handle_hud_composite_capture_complete);
    config.requested = true;
}

/// Requests capture of the raw Vello HUD texture once it becomes available.
///
/// The system waits for the delayed frame count, finds the first non-composited Vello canvas
/// material, and spawns a readback for its texture. If the source canvas does not exist yet, it logs
/// the wait and retries on future frames.
pub(crate) fn request_hud_texture_capture(
    mut commands: Commands,
    config: Option<ResMut<HudTextureCaptureConfig>>,
    images: Res<Assets<Image>>,
    vello_materials: Res<Assets<VelloCanvasMaterial>>,
    vello_canvases: Query<&MeshMaterial2d<VelloCanvasMaterial>, Without<HudCompositeLayerMarker>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let Some(mut config) = config else {
        return;
    };
    if config.completed {
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

    let mut requested = false;
    for material_handle in &vello_canvases {
        let Some(material) = vello_materials.get(material_handle.id()) else {
            continue;
        };
        let texture = material.texture.clone();
        let Some(image) = images.get(texture.id()) else {
            continue;
        };
        crate::terminals::append_debug_log(format!(
            "hud capture requested path={} size={}x{} format={:?}",
            config.path.display(),
            image.texture_descriptor.size.width,
            image.texture_descriptor.size.height,
            image.texture_descriptor.format
        ));
        commands
            .spawn((
                Readback::texture(texture),
                HudTextureReadbackMeta::from_image(config.path.clone(), image),
            ))
            .observe(handle_hud_texture_capture_complete);
        config.requested = true;
        requested = true;
        break;
    }
    if !requested {
        crate::terminals::append_debug_log("hud capture waiting for source canvas");
    }
}

/// Requests an ordinary screenshot of the primary window once the configured delay has elapsed.
///
/// This path uses Bevy's built-in screenshot component instead of GPU readback because it wants the
/// final window image rather than a specific intermediate texture.
pub(crate) fn request_window_capture(
    mut commands: Commands,
    config: Option<ResMut<WindowCaptureConfig>>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    let Some(mut config) = config else {
        return;
    };
    if config.completed {
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
    let path = config.path.clone();
    crate::terminals::append_debug_log(format!("window capture requested path={}", path.display()));
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    config.requested = true;
}

/// Completes the window-capture workflow once Bevy's screenshot system has finished writing the file.
///
/// The function waits until no `Capturing` component remains and the output file exists, then marks
/// the capture complete, logs success, and exits the application.
pub(crate) fn finalize_window_capture(
    config: Option<ResMut<WindowCaptureConfig>>,
    captures: Query<(), With<Capturing>>,
    mut exits: MessageWriter<AppExit>,
) {
    let Some(mut config) = config else {
        return;
    };
    if !config.requested || config.completed {
        return;
    }
    if !captures.is_empty() {
        return;
    }
    if !config.path.is_file() {
        return;
    }
    crate::terminals::append_debug_log(format!("window capture wrote {}", config.path.display()));
    config.completed = true;
    exits.write(AppExit::Success);
}

/// Handles completion of a raw HUD-texture readback.
///
/// The callback writes the PPM file, logs success or failure, marks the capture config completed, and
/// exits the app so scripted capture runs terminate automatically.
fn handle_hud_texture_capture_complete(
    event: On<ReadbackComplete>,
    metas: Query<&HudTextureReadbackMeta>,
    mut exits: MessageWriter<AppExit>,
    config: Option<ResMut<HudTextureCaptureConfig>>,
) {
    let Ok(meta) = metas.get(event.entity) else {
        return;
    };
    if let Err(error) = write_texture_dump(meta, &event.data) {
        crate::terminals::append_debug_log(format!(
            "hud capture write failed path={} error={error}",
            meta.path.display()
        ));
    } else {
        crate::terminals::append_debug_log(format!("hud capture wrote {}", meta.path.display()));
    }
    if let Some(mut config) = config {
        config.completed = true;
    }
    exits.write(AppExit::Success);
}

/// Handles completion of a composited-HUD readback.
///
/// This is the same basic flow as raw HUD-texture capture, but it updates the composite-capture
/// config instead of the raw HUD capture config.
fn handle_hud_composite_capture_complete(
    event: On<ReadbackComplete>,
    metas: Query<&HudTextureReadbackMeta>,
    mut exits: MessageWriter<AppExit>,
    config: Option<ResMut<HudCompositeCaptureConfig>>,
) {
    let Ok(meta) = metas.get(event.entity) else {
        return;
    };
    if let Err(error) = write_texture_dump(meta, &event.data) {
        crate::terminals::append_debug_log(format!(
            "hud composite capture write failed path={} error={error}",
            meta.path.display()
        ));
    } else {
        crate::terminals::append_debug_log(format!(
            "hud composite capture wrote {}",
            meta.path.display()
        ));
    }
    if let Some(mut config) = config {
        config.completed = true;
    }
    exits.write(AppExit::Success);
}

/// Serializes one HUD readback buffer into a PPM file on disk.
///
/// Format-aware byte conversion is delegated to [`texture_bytes_to_ppm`]; this helper is the thin I/O
/// layer that adds the destination path to any filesystem error.
fn write_texture_dump(meta: &HudTextureReadbackMeta, bytes: &[u8]) -> Result<(), String> {
    let ppm = texture_bytes_to_ppm(meta.width, meta.height, meta.format, bytes)?;
    fs::write(&meta.path, ppm)
        .map_err(|error| format!("failed to write {}: {error}", meta.path.display()))
}

/// Converts HUD readback bytes into a tightly packed binary PPM image.
///
/// The helper understands the small RGBA/BGRA format set used by HUD capture, compensates for GPU
/// row alignment padding, drops alpha, and reorders BGRA into RGB when needed.
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
        other => {
            return Err(format!("unsupported hud capture format: {other:?}"));
        }
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

/// Rounds a packed row byte count up to WGPU's 256-byte copy alignment.
///
/// The bit-mask formula is the standard power-of-two round-up used for GPU readback buffers.
fn align_copy_bytes_per_row(value: usize) -> usize {
    const ALIGNMENT: usize = 256;
    (value + (ALIGNMENT - 1)) & !(ALIGNMENT - 1)
}

#[cfg(test)]
mod tests;
