use bevy::render::render_resource::TextureFormat;
use std::{fs, path::Path};

pub fn write_texture_dump_to_path(
    path: &Path,
    width: u32,
    height: u32,
    format: TextureFormat,
    bytes: &[u8],
    context: &str,
) -> Result<(), String> {
    let ppm = texture_bytes_to_ppm(width, height, format, bytes, context)?;
    fs::write(path, ppm).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

pub fn texture_bytes_to_ppm(
    width: u32,
    height: u32,
    format: TextureFormat,
    bytes: &[u8],
    context: &str,
) -> Result<Vec<u8>, String> {
    let pixel_size = match format {
        TextureFormat::Rgba8Unorm
        | TextureFormat::Rgba8UnormSrgb
        | TextureFormat::Bgra8Unorm
        | TextureFormat::Bgra8UnormSrgb => 4usize,
        other => return Err(format!("unsupported {context} format: {other:?}")),
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

pub fn align_copy_bytes_per_row(value: usize) -> usize {
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
        let ppm =
            texture_bytes_to_ppm(width, height, TextureFormat::Rgba8Unorm, &bytes, "test").unwrap();
        assert_eq!(&ppm[..11], b"P6\n2 2\n255\n");
        assert_eq!(
            &ppm[11..],
            &[225, 129, 10, 25, 215, 189, 0, 0, 0, 255, 255, 255]
        );
    }

    #[test]
    fn texture_dump_swaps_bgra_channels() {
        let bytes = [10u8, 129, 225, 255];
        let ppm = texture_bytes_to_ppm(1, 1, TextureFormat::Bgra8Unorm, &bytes, "test").unwrap();
        assert_eq!(&ppm[11..], &[225, 129, 10]);
    }

    #[test]
    fn texture_dump_reports_short_buffers() {
        let error = texture_bytes_to_ppm(2, 2, TextureFormat::Rgba8Unorm, &[0; 4], "test")
            .expect_err("short buffer should error");
        assert!(error.contains("short readback buffer"));
    }

    #[test]
    fn texture_dump_reports_unsupported_formats() {
        let error = texture_bytes_to_ppm(1, 1, TextureFormat::R8Unorm, &[0], "hud capture")
            .expect_err("unsupported format should error");
        assert!(error.contains("unsupported hud capture format"));
    }
}
