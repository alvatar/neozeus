mod support;

use support::visual::{
    average_region_rgb, crop_region_rgb, run_offscreen_scenario_with_env,
    write_region_comparison_artifacts,
};

#[test]
fn selected_agent_context_box_renders_above_agent_bloom_in_overlap_region() {
    let context_run = run_offscreen_scenario_with_env("agent-context-bloom", &[]);
    let no_bloom_run = run_offscreen_scenario_with_env(
        "agent-context-bloom",
        &[("NEOZEUS_AGENT_BLOOM_INTENSITY", "0")],
    );
    assert_eq!((context_run.width, context_run.height), (1920, 1200));
    assert_eq!(
        (no_bloom_run.width, no_bloom_run.height),
        (context_run.width, context_run.height)
    );

    // This stripe sits just inside the left edge of the context box, in a blank area that should
    // stay black regardless of whether agent bloom is enabled. If bloom paints above the box, the
    // stripe becomes orange instead of matching the no-bloom interior.
    let overlap_avg = average_region_rgb(&context_run.data, context_run.width, 290, 130, 298, 176);
    let no_bloom_avg =
        average_region_rgb(&no_bloom_run.data, no_bloom_run.width, 290, 130, 298, 176);
    let (crop_width, crop_height, context_crop) =
        crop_region_rgb(&context_run.data, context_run.width, 280, 120, 320, 190);
    let (_, _, no_bloom_crop) =
        crop_region_rgb(&no_bloom_run.data, no_bloom_run.width, 280, 120, 320, 190);
    let _ = write_region_comparison_artifacts(
        &context_run.dir,
        "overlap",
        crop_width,
        crop_height,
        &context_crop,
        &no_bloom_crop,
    );
    let channel_delta = [
        (overlap_avg[0] - no_bloom_avg[0]).abs(),
        (overlap_avg[1] - no_bloom_avg[1]).abs(),
        (overlap_avg[2] - no_bloom_avg[2]).abs(),
    ];

    assert!(
        channel_delta.iter().all(|delta| *delta < 8.0),
        "context box overlap stripe should match the no-bloom box interior, but bloom leaked on top: overlap_avg={:?} no_bloom_avg={:?} delta={:?} context_frame={} no_bloom_frame={}",
        overlap_avg,
        no_bloom_avg,
        channel_delta,
        context_run.frame_path.display(),
        no_bloom_run.frame_path.display(),
    );
}

#[test]
fn selected_agent_context_box_body_stays_stable_when_bloom_is_enabled() {
    let context_run = run_offscreen_scenario_with_env("agent-context-bloom", &[]);
    let no_bloom_run = run_offscreen_scenario_with_env(
        "agent-context-bloom",
        &[("NEOZEUS_AGENT_BLOOM_INTENSITY", "0")],
    );

    let body_avg = average_region_rgb(&context_run.data, context_run.width, 320, 145, 390, 180);
    let no_bloom_body_avg =
        average_region_rgb(&no_bloom_run.data, no_bloom_run.width, 320, 145, 390, 180);
    let channel_delta = [
        (body_avg[0] - no_bloom_body_avg[0]).abs(),
        (body_avg[1] - no_bloom_body_avg[1]).abs(),
        (body_avg[2] - no_bloom_body_avg[2]).abs(),
    ];

    assert!(
        channel_delta.iter().all(|delta| *delta < 10.0),
        "context box body should remain stable under bloom: body_avg={:?} no_bloom_body_avg={:?} delta={:?} context_frame={} no_bloom_frame={}",
        body_avg,
        no_bloom_body_avg,
        channel_delta,
        context_run.frame_path.display(),
        no_bloom_run.frame_path.display(),
    );
}
