use super::request_hud_composite_capture;
use crate::{
    hud::{HudCompositeBloomCameraMarker, HudCompositeCameraMarker, HudCompositeModalCameraMarker},
    shared::{
        capture::ArmedCaptureRequestState,
        readback::{align_copy_bytes_per_row, texture_bytes_to_ppm},
    },
};
use bevy::{
    camera::RenderTarget,
    ecs::system::RunSystemOnce,
    prelude::*,
    render::render_resource::TextureFormat,
    window::{PrimaryWindow, RequestRedraw},
};

/// Checks that HUD texture dump generation ignores per-row GPU padding for RGBA data.
///
/// The readback helper is fed a padded 2×2 buffer and must emit a tightly packed PPM payload. The
/// assertions verify both the header and the exact RGB byte order of the two logical rows.
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
        "hud capture",
    )
    .unwrap();
    assert_eq!(&ppm[..11], b"P6\n2 2\n255\n");
    assert_eq!(
        &ppm[11..],
        &[225, 129, 10, 25, 215, 189, 0, 0, 0, 255, 255, 255]
    );
}

/// Checks that BGRA HUD readback bytes are converted into RGB order before being written to PPM.
///
/// The helper should treat the incoming bytes as BGRA, drop alpha, and swap channels so the output
/// image contains the expected red-green-blue ordering.
#[test]
fn texture_dump_swaps_bgra_channels() {
    let bytes = [10u8, 129, 225, 255];
    let ppm = texture_bytes_to_ppm(1, 1, TextureFormat::Bgra8Unorm, &bytes, "hud capture").unwrap();
    assert_eq!(&ppm[11..], &[225, 129, 10]);
}

#[test]
fn composite_capture_retargets_bloom_camera_to_shared_capture_image() {
    let mut world = World::default();
    world.insert_resource(super::HudCompositeCaptureConfig {
        path: "/tmp/hud-composite-test.ppm".into(),
        request: ArmedCaptureRequestState::new(0),
        target_image: None,
    });
    world.insert_resource(Assets::<Image>::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));
    let main_camera = world.spawn((HudCompositeCameraMarker,)).id();
    let bloom_camera = world.spawn((HudCompositeBloomCameraMarker,)).id();
    let modal_camera = world.spawn((HudCompositeModalCameraMarker,)).id();

    world
        .run_system_once(request_hud_composite_capture)
        .unwrap();

    let Some(RenderTarget::Image(main_target)) = world.get::<RenderTarget>(main_camera) else {
        panic!("main compositor camera should target capture image");
    };
    let Some(RenderTarget::Image(bloom_target)) = world.get::<RenderTarget>(bloom_camera) else {
        panic!("bloom compositor camera should target capture image");
    };
    let Some(RenderTarget::Image(modal_target)) = world.get::<RenderTarget>(modal_camera) else {
        panic!("modal compositor camera should target capture image");
    };
    assert_eq!(main_target.handle, bloom_target.handle);
    assert_eq!(main_target.handle, modal_target.handle);
}
