mod support;

use std::{fs, path::Path};

use support::visual::{
    average_region_rgb, count_nonblack_pixels, crop_region_rgb, read_binary_ppm,
    run_offscreen_process_with_owned_env, unique_temp_dir, write_absolute_diff_ppm,
    write_binary_ppm, write_region_comparison_artifacts,
};

#[test]
fn support_exports_cover_offscreen_run_contract() {
    let run = support::visual::OffscreenScenarioRun {
        dir: unique_temp_dir("neozeus-visual-contract"),
        frame_path: std::env::temp_dir().join("frame.ppm"),
        width: 1,
        height: 1,
        data: vec![0, 0, 0],
    };
    assert!(run.dir.starts_with(std::env::temp_dir()));
    let _ = support::visual::run_offscreen_scenario_with_env
        as fn(&str, &[(&str, &str)]) -> support::visual::OffscreenScenarioRun;
    let _ = run_offscreen_process_with_owned_env
        as fn(&str, &[(String, String)]) -> support::visual::OffscreenProcessRun;
}

#[test]
fn unique_temp_dir_creates_distinct_existing_directories() {
    let first = unique_temp_dir("neozeus-visual-support");
    let second = unique_temp_dir("neozeus-visual-support");
    assert_ne!(first, second);
    assert!(first.exists());
    assert!(second.exists());
}

#[test]
fn read_binary_ppm_parses_header_and_pixels() {
    let dir = unique_temp_dir("neozeus-ppm-parse");
    let path = dir.join("sample.ppm");
    fs::write(&path, b"P6\n2 1\n255\n\x01\x02\x03\x04\x05\x06").unwrap();

    let (width, height, data) = read_binary_ppm(&path);
    assert_eq!((width, height), (2, 1));
    assert_eq!(data, vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn average_region_rgb_reports_expected_channel_means() {
    let width = 2;
    let data = vec![10, 20, 30, 50, 60, 70];
    assert_eq!(
        average_region_rgb(&data, width, 0, 0, 2, 1),
        [30.0, 40.0, 50.0]
    );
    assert_eq!(count_nonblack_pixels(&data, width, 0, 0, 2, 1, 40), 1);
}

#[test]
fn diff_and_crop_helpers_write_expected_ppm_artifacts() {
    let dir = unique_temp_dir("neozeus-visual-artifacts");
    let left = vec![0, 0, 0, 10, 20, 30, 40, 50, 60, 70, 80, 90];
    let right = vec![0, 0, 0, 0, 10, 20, 40, 50, 60, 60, 70, 80];
    let diff_path = dir.join("diff.ppm");
    write_absolute_diff_ppm(&diff_path, 2, 2, &left, &right);
    let (width, height, data) = read_binary_ppm(&diff_path);
    assert_eq!((width, height), (2, 2));
    assert_eq!(data[3..6], [10, 10, 10]);

    let crop_path = dir.join("crop.ppm");
    let (crop_width, crop_height, crop) = crop_region_rgb(&left, 2, 1, 0, 2, 2);
    write_binary_ppm(&crop_path, crop_width, crop_height, &crop);
    let (written_width, written_height, written_crop) = read_binary_ppm(&crop_path);
    assert_eq!((written_width, written_height), (1, 2));
    assert_eq!(written_crop, vec![10, 20, 30, 70, 80, 90]);

    let artifacts_dir = unique_temp_dir("neozeus-region-artifacts");
    let (left_path, right_path, diff_bundle_path) =
        write_region_comparison_artifacts(&artifacts_dir, "bundle", 1, 2, &crop, &written_crop);
    assert!(left_path.is_file());
    assert!(right_path.is_file());
    assert!(diff_bundle_path.is_file());
}

#[test]
fn offscreen_process_helper_exposes_output_buffers() {
    let run = support::visual::OffscreenProcessRun {
        dir: unique_temp_dir("neozeus-process-run"),
        stdout: b"stdout".to_vec(),
        stderr: b"stderr".to_vec(),
    };
    assert_eq!(run.stdout, b"stdout");
    assert_eq!(run.stderr, b"stderr");
}

#[test]
fn unique_temp_dir_paths_stay_under_system_temp_root() {
    let dir = unique_temp_dir("neozeus-temp-root");
    assert!(Path::new(&std::env::temp_dir()).starts_with(std::env::temp_dir()));
    assert!(dir.starts_with(std::env::temp_dir()));
}
