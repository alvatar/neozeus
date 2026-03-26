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
    // Builds this value from environment variables.
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
    // Builds this value from environment variables.
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
    // Builds this value from environment variables.
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
    // Builds this value from image metadata.
    fn from_image(path: PathBuf, image: &Image) -> Self {
        Self {
            path,
            width: image.texture_descriptor.size.width,
            height: image.texture_descriptor.size.height,
            format: image.texture_descriptor.format,
        }
    }
}

// Implements composite capture target image.
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

// Requests HUD composite capture.
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

// Requests HUD texture capture.
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

// Requests window capture.
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

// Finalizes window capture.
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

// Handles HUD texture capture complete.
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

// Handles HUD composite capture complete.
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

// Writes texture dump.
fn write_texture_dump(meta: &HudTextureReadbackMeta, bytes: &[u8]) -> Result<(), String> {
    let ppm = texture_bytes_to_ppm(meta.width, meta.height, meta.format, bytes)?;
    fs::write(&meta.path, ppm)
        .map_err(|error| format!("failed to write {}: {error}", meta.path.display()))
}

// Implements texture bytes to PPM.
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

// Implements align copy bytes per row.
fn align_copy_bytes_per_row(value: usize) -> usize {
    const ALIGNMENT: usize = 256;
    (value + (ALIGNMENT - 1)) & !(ALIGNMENT - 1)
}

#[cfg(test)]
mod tests;
