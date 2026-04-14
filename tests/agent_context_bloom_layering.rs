use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir should create");
    dir
}

fn read_binary_ppm(path: &Path) -> (u32, u32, Vec<u8>) {
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

fn run_offscreen_scenario_with_env(
    scenario: &str,
    extra_env: &[(&str, &str)],
) -> (u32, u32, Vec<u8>) {
    let smoke_dir = unique_temp_dir(&format!("neozeus-{scenario}-test"));
    let home = smoke_dir.join("home");
    let xdg_config = smoke_dir.join("xdg-config");
    let xdg_cache = smoke_dir.join("xdg-cache");
    let xdg_state = smoke_dir.join("xdg-state");
    let xdg_runtime = smoke_dir.join("xdg-runtime");
    for dir in [&home, &xdg_config, &xdg_cache, &xdg_state, &xdg_runtime] {
        fs::create_dir_all(dir).expect("runtime dir should create");
    }
    let frame_path = smoke_dir.join("final-frame.ppm");

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

    read_binary_ppm(&frame_path)
}

fn run_offscreen_scenario(scenario: &str) -> (u32, u32, Vec<u8>) {
    run_offscreen_scenario_with_env(scenario, &[])
}

fn average_region_rgb(data: &[u8], width: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> [f32; 3] {
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

#[test]
fn selected_agent_context_box_renders_above_agent_bloom_in_overlap_region() {
    let (context_width, context_height, context_frame) =
        run_offscreen_scenario("agent-context-bloom");
    let (no_bloom_width, no_bloom_height, no_bloom_frame) = run_offscreen_scenario_with_env(
        "agent-context-bloom",
        &[("NEOZEUS_AGENT_BLOOM_INTENSITY", "0")],
    );
    assert_eq!((context_width, context_height), (1920, 1200));
    assert_eq!(
        (no_bloom_width, no_bloom_height),
        (context_width, context_height)
    );

    // This stripe sits just inside the left edge of the context box, in a blank area that should
    // stay black regardless of whether agent bloom is enabled. If the bloom pass is composited above
    // the context box, the stripe turns orange even though the box should fully occlude it.
    let overlap_avg = average_region_rgb(&context_frame, context_width, 290, 130, 298, 176);
    let no_bloom_avg = average_region_rgb(&no_bloom_frame, no_bloom_width, 290, 130, 298, 176);
    let channel_delta = [
        (overlap_avg[0] - no_bloom_avg[0]).abs(),
        (overlap_avg[1] - no_bloom_avg[1]).abs(),
        (overlap_avg[2] - no_bloom_avg[2]).abs(),
    ];

    assert!(
        channel_delta.iter().all(|delta| *delta < 8.0),
        "context box overlap stripe should match the no-bloom box interior, but bloom leaked on top: overlap_avg={:?} no_bloom_avg={:?} delta={:?}",
        overlap_avg,
        no_bloom_avg,
        channel_delta,
    );
}
