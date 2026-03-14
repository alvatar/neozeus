use alacritty_terminal::{
    event::VoidListener,
    grid::{Dimensions, Scroll},
    term::{cell::Flags, color::Colors, Config as TermConfig, Term},
    vte::ansi::{self, Color as AnsiColor, CursorShape, NamedColor, Rgb},
};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    image::ImageSampler,
    input::{
        keyboard::KeyboardInput,
        mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
        ButtonState,
    },
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        settings::WgpuSettings,
        RenderPlugin,
    },
    sprite::Anchor,
    text::{Font, TextBounds, TextColor, TextFont},
    window::PrimaryWindow,
    winit::{EventLoopProxy, EventLoopProxyWrapper, WinitSettings, WinitUserEvent},
};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use bevy_vello::{
    prelude::{kurbo, peniko, VelloScene2d, VelloView},
    vello, VelloPlugin,
};
use cosmic_text::{
    fontdb, Attrs as CtAttrs, Buffer as CtBuffer, Color as CtColor, Family as CtFamily,
    FontSystem as CtFontSystem, Metrics as CtMetrics, Shaping as CtShaping,
    SwashCache as CtSwashCache,
};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    any::Any,
    collections::{BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

mod hud;
mod input;
mod scene;
mod terminals;

use hud::*;
use input::*;
use scene::*;
use terminals::*;

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 38;
const DEFAULT_BG: egui::Color32 = egui::Color32::from_rgb(10, 10, 10);
#[allow(
    dead_code,
    reason = "legacy per-cell terminal renderer kept temporarily"
)]
const BASE_CELL_ASPECT: f32 = 0.6;
const TERMINAL_MARGIN: f32 = 48.0;
#[allow(
    dead_code,
    reason = "legacy per-cell terminal renderer kept temporarily"
)]
const CURSOR_Z: f32 = 2.0;
#[allow(
    dead_code,
    reason = "legacy per-cell terminal renderer kept temporarily"
)]
const TEXT_Z: f32 = 1.0;
#[allow(
    dead_code,
    reason = "legacy per-cell terminal renderer kept temporarily"
)]
const BG_Z: f32 = 0.0;
const DEFAULT_CELL_HEIGHT_PX: u32 = 24;
const DEFAULT_CELL_WIDTH_PX: u32 = 14;
const GPU_NOT_FOUND_PANIC_FRAGMENT: &str = "Unable to find a GPU!";
const DEBUG_LOG_PATH: &str = "/tmp/neozeus-debug.log";
const DEBUG_TEXTURE_DUMP_PATH: &str = "/tmp/neozeus-texture.ppm";
const EVA_DEMO_Z: f32 = 20.0;

fn main() {
    let _ = fs::write(DEBUG_LOG_PATH, "");
    append_debug_log("app start");
    match build_app() {
        Ok(mut app) => {
            let _ = app.run();
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        blend_rgba_in_place, ctrl_sequence, find_kitty_config_path, format_startup_panic,
        initialize_terminal_text_renderer, is_emoji_like, is_private_use_like,
        keyboard_input_to_terminal_command, parse_kitty_config_file, rasterize_terminal_glyph,
        resolve_alacritty_color, resolve_terminal_font_report, xterm_indexed_rgb,
        CachedTerminalGlyph, KittyFontConfig, TerminalCommand, TerminalFontRole, TerminalFontState,
        TerminalGlyphCacheKey, TerminalTextRenderer,
    };
    use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
    use bevy::{
        input::{
            keyboard::{Key, KeyboardInput},
            ButtonState,
        },
        prelude::*,
    };
    use std::{
        collections::BTreeSet,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn pressed_text(key_code: KeyCode, text: Option<&str>) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key: Key::Character(text.unwrap_or("").into()),
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    #[test]
    fn ctrl_sequence_maps_common_shortcuts() {
        assert_eq!(ctrl_sequence(KeyCode::KeyC), Some("\u{3}"));
        assert_eq!(ctrl_sequence(KeyCode::KeyL), Some("\u{c}"));
        assert_eq!(ctrl_sequence(KeyCode::Enter), None);
    }

    #[test]
    fn plain_text_uses_text_payload() {
        let keys = ButtonInput::<KeyCode>::default();
        let event = pressed_text(KeyCode::KeyA, Some("a"));
        let command = keyboard_input_to_terminal_command(&event, &keys);
        match command {
            Some(TerminalCommand::InputText(text)) => assert_eq!(text, "a"),
            _ => panic!("expected text input command"),
        }
    }

    #[test]
    fn indexed_color_has_expected_blue_cube_entry() {
        let rgb = xterm_indexed_rgb(21);
        assert_eq!((rgb.r, rgb.g, rgb.b), (0, 0, 255));
    }

    #[test]
    fn alpha_blend_preserves_transparent_glyph_background() {
        let mut pixel = [0, 0, 0, 0];
        blend_rgba_in_place(&mut pixel, [255, 255, 255, 0]);
        assert_eq!(pixel, [0, 0, 0, 0]);

        blend_rgba_in_place(&mut pixel, [255, 255, 255, 128]);
        assert_eq!(pixel[3], 128);
    }

    #[test]
    fn named_cursor_color_resolves() {
        let color = resolve_alacritty_color(
            AnsiColor::Named(NamedColor::Cursor),
            &Default::default(),
            true,
        );
        assert_eq!((color.r(), color.g(), color.b()), (82, 173, 112));
    }

    #[test]
    fn parses_font_family_from_included_kitty_config() {
        let dir = temp_dir("neozeus-kitty-font-test");
        let main = dir.join("kitty.conf");
        let included = dir.join("fonts.conf");
        fs::write(&included, "font_family JetBrains Mono Nerd Font\n")
            .expect("failed to write include config");
        fs::write(&main, "include fonts.conf\n").expect("failed to write main config");

        let mut visited = BTreeSet::new();
        let mut config = KittyFontConfig::default();
        parse_kitty_config_file(&main, &mut visited, &mut config)
            .expect("failed to parse kitty config");

        assert_eq!(
            config.font_family.as_deref(),
            Some("JetBrains Mono Nerd Font")
        );
    }

    #[test]
    fn current_host_has_no_user_kitty_config() {
        assert_eq!(find_kitty_config_path(), None);
    }

    #[test]
    fn resolves_effective_terminal_font_stack_on_host() {
        let report = resolve_terminal_font_report().expect("failed to resolve terminal fonts");
        assert_eq!(report.requested_family, "monospace");
        assert_eq!(report.primary.family, "Adwaita Mono");
        assert!(report.primary.path.is_file());
        assert!(report
            .fallbacks
            .iter()
            .any(|face| face.family.contains("Nerd Font")));
    }

    #[test]
    fn detects_special_font_ranges() {
        assert!(is_private_use_like('\u{e0b0}'));
        assert!(is_emoji_like('🚀'));
        assert!(!is_private_use_like('a'));
    }

    #[test]
    fn standalone_text_renderer_rasterizes_ascii_glyph() {
        let report = resolve_terminal_font_report().expect("failed to resolve terminal fonts");
        let mut renderer = TerminalTextRenderer::default();
        initialize_terminal_text_renderer(&report, &mut renderer)
            .expect("failed to initialize terminal text renderer");
        let font_state = TerminalFontState {
            report: Some(Ok(report)),
            primary_font: None,
            private_use_font: None,
            emoji_font: None,
        };
        let glyph = rasterize_terminal_glyph(
            &TerminalGlyphCacheKey {
                text: "A".into(),
                font_role: TerminalFontRole::Primary,
                width_cells: 1,
                cell_width: 14,
                cell_height: 24,
            },
            TerminalFontRole::Primary,
            false,
            &mut renderer,
            &font_state,
        );
        assert_glyph_has_visible_pixels(&glyph);
    }

    fn assert_glyph_has_visible_pixels(glyph: &CachedTerminalGlyph) {
        let non_zero_alpha = glyph
            .pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .count();
        assert!(
            non_zero_alpha > 0,
            "glyph rasterized to fully transparent image"
        );
    }

    #[test]
    fn formats_missing_gpu_startup_panics_as_user_facing_errors() {
        let error = format_startup_panic(&"Unable to find a GPU! renderer init failed")
            .expect("missing gpu panic should be formatted");
        assert!(error.contains("could not find a usable graphics adapter"));
        assert!(format_startup_panic(&"some other panic").is_none());
    }
}
