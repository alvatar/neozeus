mod support;

use std::{fs, path::Path};

use support::visual::{average_region_rgb, read_binary_ppm, unique_temp_dir};

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
    fs::write(
        &path,
        b"P6\n2 1\n255\n\x01\x02\x03\x04\x05\x06",
    )
    .unwrap();

    let (width, height, data) = read_binary_ppm(&path);
    assert_eq!((width, height), (2, 1));
    assert_eq!(data, vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn average_region_rgb_reports_expected_channel_means() {
    let width = 2;
    let data = vec![10, 20, 30, 50, 60, 70];
    assert_eq!(average_region_rgb(&data, width, 0, 0, 2, 1), [30.0, 40.0, 50.0]);
}

#[test]
fn unique_temp_dir_paths_stay_under_system_temp_root() {
    let dir = unique_temp_dir("neozeus-temp-root");
    assert!(Path::new(&std::env::temp_dir()).starts_with(std::env::temp_dir()));
    assert!(dir.starts_with(std::env::temp_dir()));
}
