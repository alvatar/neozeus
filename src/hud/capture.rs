use std::{env, fs, path::PathBuf};

use bevy::{
    app::AppExit,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::TextureFormat,
        view::screenshot::{save_to_disk, Capturing, Screenshot},
    },
    sprite_render::MeshMaterial2d,
    window::RequestRedraw,
};
use bevy_vello::render::VelloCanvasMaterial;

#[derive(Resource, Clone, Debug)]
pub(crate) struct HudTextureCaptureConfig {
    path: PathBuf,
    frames_until_capture: u32,
    requested: bool,
    completed: bool,
}

impl HudTextureCaptureConfig {
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

#[derive(Component, Clone, Debug)]
struct HudTextureReadbackMeta {
    path: PathBuf,
    width: u32,
    height: u32,
    format: TextureFormat,
}

impl HudTextureReadbackMeta {
    fn from_image(path: PathBuf, image: &Image) -> Self {
        Self {
            path,
            width: image.texture_descriptor.size.width,
            height: image.texture_descriptor.size.height,
            format: image.texture_descriptor.format,
        }
    }
}

pub(crate) fn request_hud_texture_capture(
    mut commands: Commands,
    config: Option<ResMut<HudTextureCaptureConfig>>,
    images: Res<Assets<Image>>,
    vello_materials: Res<Assets<VelloCanvasMaterial>>,
    vello_canvases: Query<&MeshMaterial2d<VelloCanvasMaterial>>,
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
        break;
    }
}

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

fn write_texture_dump(meta: &HudTextureReadbackMeta, bytes: &[u8]) -> Result<(), String> {
    let ppm = texture_bytes_to_ppm(meta.width, meta.height, meta.format, bytes)?;
    fs::write(&meta.path, ppm)
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

fn align_copy_bytes_per_row(value: usize) -> usize {
    const ALIGNMENT: usize = 256;
    (value + (ALIGNMENT - 1)) & !(ALIGNMENT - 1)
}

#[cfg(test)]
mod tests {
    use super::{align_copy_bytes_per_row, texture_bytes_to_ppm};
    use bevy::render::render_resource::TextureFormat;

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

    #[test]
    fn texture_dump_swaps_bgra_channels() {
        let bytes = [10u8, 129, 225, 255];
        let ppm = texture_bytes_to_ppm(1, 1, TextureFormat::Bgra8Unorm, &bytes).unwrap();
        assert_eq!(&ppm[11..], &[225, 129, 10]);
    }
}
