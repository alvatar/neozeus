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

fn run_offscreen_scenario(scenario: &str) -> (u32, u32, Vec<u8>) {
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

    let output = Command::new(env!("CARGO_BIN_EXE_neozeus"))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_CACHE_HOME", &xdg_cache)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("XDG_RUNTIME_DIR", &xdg_runtime)
        .env("NEOZEUS_OUTPUT_MODE", "offscreen")
        .env("NEOZEUS_VERIFY_SCENARIO", scenario)
        .env("NEOZEUS_CAPTURE_FINAL_FRAME_PATH", &frame_path)
        .env("NEOZEUS_EXIT_AFTER_CAPTURE", "1")
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
fn selected_agent_context_box_darkens_the_bloom_region_in_final_frame() {
    let (context_width, context_height, context_frame) =
        run_offscreen_scenario("agent-context-bloom");
    let (agent_list_width, agent_list_height, agent_list_frame) =
        run_offscreen_scenario("agent-list-bloom");
    assert_eq!((context_width, context_height), (1920, 1200));
    assert_eq!(
        (agent_list_width, agent_list_height),
        (context_width, context_height)
    );

    // This region covers the deterministic context-box background area immediately to the right of
    // the selected agent row in the `agent-context-bloom` scenario. Without the box, the same area
    // remains visibly bloom-tinted; with the box, it should darken substantially.
    let context_avg = average_region_rgb(&context_frame, context_width, 309, 132, 435, 152);
    let agent_list_avg =
        average_region_rgb(&agent_list_frame, agent_list_width, 309, 132, 435, 152);
    let context_luma = context_avg[0] + context_avg[1] + context_avg[2];
    let agent_list_luma = agent_list_avg[0] + agent_list_avg[1] + agent_list_avg[2];

    assert!(
        context_luma < 15.0,
        "context box background should be near-opaque dark in the verified region: context_avg={:?}",
        context_avg,
    );
    assert!(
        context_luma < agent_list_luma * 0.45,
        "context box should materially darken the bloom region: context_avg={:?} agent_list_avg={:?}",
        context_avg,
        agent_list_avg,
    );
}
