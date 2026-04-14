mod support;

use support::visual::{
    average_region_rgb, crop_region_rgb, run_offscreen_scenario_with_env,
    write_region_comparison_artifacts,
};

#[test]
fn message_box_modal_body_stays_stable_when_bloom_is_enabled() {
    let bloom_run = run_offscreen_scenario_with_env("message-box-bloom", &[]);
    let no_bloom_run = run_offscreen_scenario_with_env(
        "message-box-bloom",
        &[("NEOZEUS_AGENT_BLOOM_INTENSITY", "0")],
    );

    let body_avg = average_region_rgb(&bloom_run.data, bloom_run.width, 220, 170, 340, 300);
    let no_bloom_body_avg =
        average_region_rgb(&no_bloom_run.data, no_bloom_run.width, 220, 170, 340, 300);
    let (_, _, bloom_crop) = crop_region_rgb(&bloom_run.data, bloom_run.width, 200, 150, 380, 320);
    let (crop_width, crop_height, no_bloom_crop) =
        crop_region_rgb(&no_bloom_run.data, no_bloom_run.width, 200, 150, 380, 320);
    let _ = write_region_comparison_artifacts(
        &bloom_run.dir,
        "message-box-body",
        crop_width,
        crop_height,
        &bloom_crop,
        &no_bloom_crop,
    );

    let channel_delta = [
        (body_avg[0] - no_bloom_body_avg[0]).abs(),
        (body_avg[1] - no_bloom_body_avg[1]).abs(),
        (body_avg[2] - no_bloom_body_avg[2]).abs(),
    ];
    assert!(
        channel_delta.iter().all(|delta| *delta < 10.0),
        "message box body should remain stable above lower-surface bloom: bloom_avg={:?} no_bloom_avg={:?} delta={:?} bloom_frame={} no_bloom_frame={}",
        body_avg,
        no_bloom_body_avg,
        channel_delta,
        bloom_run.frame_path.display(),
        no_bloom_run.frame_path.display(),
    );
}

#[test]
fn context_bloom_changes_selected_row_roi_without_leaking_into_terminal_roi() {
    let bloom_run = run_offscreen_scenario_with_env(
        "agent-context-bloom",
        &[("NEOZEUS_AGENT_BLOOM_INTENSITY", "2")],
    );
    let no_bloom_run = run_offscreen_scenario_with_env(
        "agent-context-bloom",
        &[("NEOZEUS_AGENT_BLOOM_INTENSITY", "0")],
    );

    let left_panel_avg = average_region_rgb(&bloom_run.data, bloom_run.width, 400, 80, 520, 160);
    let no_bloom_left_panel_avg =
        average_region_rgb(&no_bloom_run.data, no_bloom_run.width, 400, 80, 520, 160);
    let terminal_roi_avg =
        average_region_rgb(&bloom_run.data, bloom_run.width, 980, 120, 1500, 320);
    let no_bloom_terminal_roi_avg =
        average_region_rgb(&no_bloom_run.data, no_bloom_run.width, 980, 120, 1500, 320);

    let (crop_width, crop_height, bloom_crop) =
        crop_region_rgb(&bloom_run.data, bloom_run.width, 940, 100, 1540, 340);
    let (_, _, no_bloom_crop) =
        crop_region_rgb(&no_bloom_run.data, no_bloom_run.width, 940, 100, 1540, 340);
    let _ = write_region_comparison_artifacts(
        &bloom_run.dir,
        "context-terminal-isolation",
        crop_width,
        crop_height,
        &bloom_crop,
        &no_bloom_crop,
    );

    let left_delta = [
        (left_panel_avg[0] - no_bloom_left_panel_avg[0]).abs(),
        (left_panel_avg[1] - no_bloom_left_panel_avg[1]).abs(),
        (left_panel_avg[2] - no_bloom_left_panel_avg[2]).abs(),
    ];
    let terminal_delta = [
        (terminal_roi_avg[0] - no_bloom_terminal_roi_avg[0]).abs(),
        (terminal_roi_avg[1] - no_bloom_terminal_roi_avg[1]).abs(),
        (terminal_roi_avg[2] - no_bloom_terminal_roi_avg[2]).abs(),
    ];

    assert!(
        left_delta.iter().copied().sum::<f32>() > 4.0,
        "context bloom should materially change the selected-row ROI: bloom_avg={:?} no_bloom_avg={:?} delta={:?}",
        left_panel_avg,
        no_bloom_left_panel_avg,
        left_delta,
    );
    assert!(
        terminal_delta.iter().all(|delta| *delta < 2.0),
        "context bloom should not leak into the terminal-view ROI: bloom_avg={:?} no_bloom_avg={:?} delta={:?} bloom_frame={} no_bloom_frame={}",
        terminal_roi_avg,
        no_bloom_terminal_roi_avg,
        terminal_delta,
        bloom_run.frame_path.display(),
        no_bloom_run.frame_path.display(),
    );
}
