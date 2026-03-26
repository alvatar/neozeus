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
