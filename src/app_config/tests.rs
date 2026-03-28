use super::*;
use crate::{
    app::{primary_window_config_for_with_config, AppOutputConfig, OutputMode},
    tests::temp_dir,
};

/// Verifies that the NeoZeus TOML parser populates terminal font and window metadata fields.
#[test]
fn parses_neozeus_toml_config() {
    let config = parse_neozeus_config(
        r#"
        [terminal]
        font_path = "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"
        font_size_px = 16.0
        baseline_offset_px = -0.5

        [window]
        title = "NeoZeus"
        app_id = "neozeus-dev"
        "#,
    )
    .expect("config should parse");

    assert_eq!(
        resolve_terminal_font_path(&config),
        Some(std::path::PathBuf::from(
            "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"
        ))
    );
    assert_eq!(config.terminal_font_size_px(), Some(16.0));
    assert_eq!(config.terminal_baseline_offset_px(), Some(-0.5));
    assert_eq!(config.window_title(), Some("NeoZeus"));
    assert_eq!(config.window_app_id(), Some("neozeus-dev"));
}

/// Verifies the config-file discovery precedence: explicit path, XDG config home, HOME fallback,
/// then `neozeus.toml` in the cwd.
#[test]
fn neozeus_config_path_resolution_prefers_explicit_then_xdg_then_home_then_cwd() {
    let dir = temp_dir("neozeus-config-resolution");
    let explicit = dir.join("explicit.toml");
    let xdg = dir.join("xdg/neozeus/config.toml");
    let home = dir.join("home/.config/neozeus/config.toml");
    let cwd = dir.join("cwd/neozeus.toml");
    for path in [&explicit, &xdg, &home, &cwd] {
        std::fs::create_dir_all(path.parent().unwrap()).expect("config dir should exist");
        std::fs::write(path, "").expect("config file should exist");
    }

    assert_eq!(
        resolve_neozeus_config_path_with(
            Some(explicit.as_os_str()),
            Some(dir.join("xdg").as_os_str()),
            Some(dir.join("home").as_os_str()),
            Some(&dir.join("cwd")),
        ),
        Some(explicit.clone())
    );
    std::fs::remove_file(&explicit).expect("explicit config should be removable");
    assert_eq!(
        resolve_neozeus_config_path_with(
            None,
            Some(dir.join("xdg").as_os_str()),
            Some(dir.join("home").as_os_str()),
            Some(&dir.join("cwd")),
        ),
        Some(xdg.clone())
    );
    std::fs::remove_file(&xdg).expect("xdg config should be removable");
    assert_eq!(
        resolve_neozeus_config_path_with(
            None,
            None,
            Some(dir.join("home").as_os_str()),
            Some(&dir.join("cwd")),
        ),
        Some(home.clone())
    );
    std::fs::remove_file(&home).expect("home config should be removable");
    assert_eq!(
        resolve_neozeus_config_path_with(None, None, None, Some(&dir.join("cwd"))),
        Some(cwd)
    );
}

/// Verifies that loaded NeoZeus config overrides are threaded through primary-window construction.
#[test]
fn primary_window_config_can_use_loaded_toml_overrides() {
    let dir = temp_dir("neozeus-config-load");
    let path = dir.join("neozeus.toml");
    std::fs::write(
        &path,
        r#"
        [terminal]
        font_path = "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"

        [window]
        title = "NeoZeus Configured"
        app_id = "neozeus-configured"
        "#,
    )
    .expect("config file should be written");
    let config = load_neozeus_config_from(&path).expect("config should load");

    let window = primary_window_config_for_with_config(
        &AppOutputConfig {
            mode: OutputMode::Desktop,
            width: 1600,
            height: 1000,
            scale_factor_override: None,
        },
        &config,
    );

    assert_eq!(window.title, "NeoZeus Configured");
    assert_eq!(window.name.as_deref(), Some("neozeus-configured"));
    assert_eq!(
        resolve_terminal_font_path(&config),
        Some(std::path::PathBuf::from(
            "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"
        ))
    );
}
