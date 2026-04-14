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

    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    (width, height, bytes[idx..].to_vec())
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

/// Runs one built-in offscreen verification scenario in an isolated runtime root.
pub(crate) fn run_offscreen_scenario_with_env(
    scenario: &str,
    extra_env: &[(&str, &str)],
) -> OffscreenScenarioRun {
    let dir = unique_temp_dir(&format!("neozeus-{scenario}-test"));
    let home = dir.join("home");
    let xdg_config = dir.join("xdg-config");
    let xdg_cache = dir.join("xdg-cache");
    let xdg_state = dir.join("xdg-state");
    let xdg_runtime = dir.join("xdg-runtime");
    for entry in [&home, &xdg_config, &xdg_cache, &xdg_state, &xdg_runtime] {
        fs::create_dir_all(entry).expect("runtime dir should create");
    }
    let frame_path = dir.join("final-frame.ppm");

    let mut command = Command::new(env!("CARGO_BIN_EXE_neozeus"));
    command
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_CACHE_HOME", &xdg_cache)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("XDG_RUNTIME_DIR", &xdg_runtime)
        .env("NEOZEUS_OUTPUT_MODE", "offscreen")
        .env("NEOZEUS_VERIFY_SCENARIO", scenario)
        .env("NEOZEUS_CAPTURE_FINAL_FRAME_PATH", &frame_path)
        .env("NEOZEUS_EXIT_AFTER_CAPTURE", "1");
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

    let (width, height, data) = read_binary_ppm(&frame_path);
    OffscreenScenarioRun {
        dir,
        frame_path,
        width,
        height,
        data,
    }
}

/// Runs one built-in offscreen verification scenario with default environment.
pub(crate) fn run_offscreen_scenario(scenario: &str) -> OffscreenScenarioRun {
    run_offscreen_scenario_with_env(scenario, &[])
}
