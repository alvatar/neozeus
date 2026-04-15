use super::super::bootstrap::primary_window_config_for;
use super::*;
use crate::hud::{AgentListBloomAdditiveCameraMarker, HudCompositeCameraMarker, HudCompositeLayerId};
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
    let overlay_composite = world
        .spawn((HudCompositeCameraMarker {
            id: HudCompositeLayerId::Overlay,
        },))
        .id();
    let modal_composite = world
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
    assert!(world.get::<RenderTarget>(overlay_composite).is_some());
    assert!(world.get::<RenderTarget>(modal_composite).is_some());

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
        world.get::<RenderTarget>(overlay_composite),
        Some(RenderTarget::Window(_))
    ));
    assert!(matches!(
        world.get::<RenderTarget>(modal_composite),
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
    world.spawn((HudCompositeCameraMarker { id: HudCompositeLayerId::Main },));
    world.spawn((HudCompositeCameraMarker { id: HudCompositeLayerId::Overlay },));
    world.spawn((HudCompositeCameraMarker { id: HudCompositeLayerId::Modal },));
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
