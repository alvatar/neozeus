use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub(crate) struct OffscreenScenarioRun {
    pub(crate) dir: PathBuf,
    pub(crate) frame_path: PathBuf,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct OffscreenProcessRun {
    pub(crate) dir: PathBuf,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

/// Creates a fresh unique temp directory under the process temp root.
pub(crate) fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir should create");
    dir
}

/// Reads a binary `P6` ppm file and returns `(width, height, rgb_bytes)`.
pub(crate) fn read_binary_ppm(path: &Path) -> (u32, u32, Vec<u8>) {
    let bytes = fs::read(path).expect("ppm should read");
    assert!(bytes.starts_with(b"P6\n"), "expected binary ppm header");

    let mut idx = 3usize;
    let mut tokens = Vec::new();
    while tokens.len() < 3 {
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx < bytes.len() && bytes[idx] == b'#' {
            while idx < bytes.len() && bytes[idx] != b'\n' {
                idx += 1;
            }
            continue;
        }
        let start = idx;
        while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        tokens.push(String::from_utf8(bytes[start..idx].to_vec()).expect("ppm token utf8"));
    }

    let width = tokens[0].parse::<u32>().expect("ppm width");
    let height = tokens[1].parse::<u32>().expect("ppm height");
    assert_eq!(tokens[2].parse::<u32>().expect("ppm max value"), 255);

    assert!(
        idx < bytes.len() && bytes[idx].is_ascii_whitespace(),
        "ppm header should terminate with one whitespace separator"
    );
    idx += 1;
    (width, height, bytes[idx..].to_vec())
}

/// Writes one binary `P6` ppm file from packed RGB bytes.
pub(crate) fn write_binary_ppm(path: &Path, width: u32, height: u32, data: &[u8]) {
    let mut bytes = format!("P6\n{} {}\n255\n", width, height).into_bytes();
    bytes.extend_from_slice(data);
    fs::write(path, bytes).expect("ppm should write");
}

/// Extracts one RGB crop from a packed full-frame RGB image.
pub(crate) fn crop_region_rgb(
    data: &[u8],
    width: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) -> (u32, u32, Vec<u8>) {
    let crop_width = x1 - x0;
    let crop_height = y1 - y0;
    let mut crop = Vec::with_capacity((crop_width * crop_height * 3) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let offset = ((y * width + x) * 3) as usize;
            crop.extend_from_slice(&data[offset..offset + 3]);
        }
    }
    (crop_width, crop_height, crop)
}

/// Writes the absolute per-channel RGB difference between two equal-sized frames.
pub(crate) fn write_absolute_diff_ppm(
    path: &Path,
    width: u32,
    height: u32,
    left: &[u8],
    right: &[u8],
) {
    let diff = left
        .iter()
        .zip(right)
        .map(|(l, r)| u8::abs_diff(*l, *r))
        .collect::<Vec<_>>();
    write_binary_ppm(path, width, height, &diff);
}

/// Writes one standard left/right/diff artifact bundle under the provided directory.
pub(crate) fn write_region_comparison_artifacts(
    dir: &Path,
    prefix: &str,
    width: u32,
    height: u32,
    left: &[u8],
    right: &[u8],
) -> (PathBuf, PathBuf, PathBuf) {
    let left_path = dir.join(format!("{prefix}-left.ppm"));
    let right_path = dir.join(format!("{prefix}-right.ppm"));
    let diff_path = dir.join(format!("{prefix}-diff.ppm"));
    write_binary_ppm(&left_path, width, height, left);
    write_binary_ppm(&right_path, width, height, right);
    write_absolute_diff_ppm(&diff_path, width, height, left, right);
    (left_path, right_path, diff_path)
}

/// Computes the average RGB values over one half-open image region.
pub(crate) fn average_region_rgb(
    data: &[u8],
    width: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) -> [f32; 3] {
    let mut sums = [0u64; 3];
    let mut count = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let offset = ((y * width + x) * 3) as usize;
            sums[0] += u64::from(data[offset]);
            sums[1] += u64::from(data[offset + 1]);
            sums[2] += u64::from(data[offset + 2]);
            count += 1;
        }
    }
    [
        sums[0] as f32 / count as f32,
        sums[1] as f32 / count as f32,
        sums[2] as f32 / count as f32,
    ]
}

/// Counts pixels whose max RGB channel is strictly above the provided threshold.
pub(crate) fn count_nonblack_pixels(
    data: &[u8],
    width: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    threshold: u8,
) -> usize {
    let mut count = 0usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let offset = ((y * width + x) * 3) as usize;
            let max_channel = data[offset..offset + 3]
                .iter()
                .copied()
                .max()
                .expect("rgb triple should exist");
            if max_channel > threshold {
                count += 1;
            }
        }
    }
    count
}

fn run_offscreen_process_in_dir(
    dir: PathBuf,
    scenario: &str,
    extra_env: &[(String, String)],
) -> OffscreenProcessRun {
    let home = dir.join("home");
    let xdg_config = dir.join("xdg-config");
    let xdg_cache = dir.join("xdg-cache");
    let xdg_state = dir.join("xdg-state");
    let xdg_runtime = dir.join("xdg-runtime");
    for entry in [&home, &xdg_config, &xdg_cache, &xdg_state, &xdg_runtime] {
        fs::create_dir_all(entry).expect("runtime dir should create");
    }

    let mut command = Command::new(env!("CARGO_BIN_EXE_neozeus"));
    command
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_CACHE_HOME", &xdg_cache)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("XDG_RUNTIME_DIR", &xdg_runtime)
        .env("NEOZEUS_OUTPUT_MODE", "offscreen")
        .env("NEOZEUS_VERIFY_SCENARIO", scenario);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let output = command
        .output()
        .expect("neozeus offscreen run should spawn");
    assert!(
        output.status.success(),
        "neozeus offscreen run failed for scenario {scenario}:\nstdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    OffscreenProcessRun {
        dir,
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

/// Runs one isolated offscreen NeoZeus process with caller-provided environment overrides.
pub(crate) fn run_offscreen_process_with_owned_env(
    scenario: &str,
    extra_env: &[(String, String)],
) -> OffscreenProcessRun {
    run_offscreen_process_in_dir(
        unique_temp_dir(&format!("neozeus-{scenario}-test")),
        scenario,
        extra_env,
    )
}

/// Runs one built-in offscreen verification scenario in an isolated runtime root and captures the final frame.
pub(crate) fn run_offscreen_scenario_with_env(
    scenario: &str,
    extra_env: &[(&str, &str)],
) -> OffscreenScenarioRun {
    let mut owned_env = extra_env
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect::<Vec<_>>();
    let dir = unique_temp_dir(&format!("neozeus-{scenario}-frame"));
    let frame_path = dir.join("final-frame.ppm");
    owned_env.push((
        "NEOZEUS_CAPTURE_FINAL_FRAME_PATH".to_owned(),
        frame_path.display().to_string(),
    ));
    owned_env.push(("NEOZEUS_EXIT_AFTER_CAPTURE".to_owned(), "1".to_owned()));
    let _process = run_offscreen_process_in_dir(dir.clone(), scenario, &owned_env);
    let (width, height, data) = read_binary_ppm(&frame_path);
    OffscreenScenarioRun {
        dir,
        frame_path,
        width,
        height,
        data,
    }
}
