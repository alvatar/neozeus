use super::super::ansi_surface::{resolve_alacritty_color, xterm_indexed_rgb};
use super::*;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

/// Verifies one representative xterm indexed-color cube entry so palette math regressions show up
/// quickly.
#[test]
fn indexed_color_has_expected_blue_cube_entry() {
    let rgb = xterm_indexed_rgb(21);
    assert_eq!((rgb.r, rgb.g, rgb.b), (0, 0, 255));
}

/// Verifies one representative named-color resolution for the terminal cursor color path.
#[test]
fn named_cursor_color_resolves() {
    let color = resolve_alacritty_color(
        AnsiColor::Named(NamedColor::Cursor),
        &Default::default(),
        true,
    );
    assert_eq!((color.r(), color.g(), color.b()), (82, 173, 112));
}

/// Verifies that multiline command payload normalization turns newline variants into carriage-return
/// PTY send sequences.
#[test]
fn send_command_payload_bytes_turn_multiline_text_into_enter_sequences() {
    assert_eq!(
        send_command_payload_bytes("echo hi\npwd"),
        b"echo hi\rpwd\r"
    );
    assert_eq!(
        send_command_payload_bytes("echo hi\r\npwd"),
        b"echo hi\rpwd\r"
    );
}
