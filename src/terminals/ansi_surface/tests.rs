use super::*;
use crate::terminals::types::{TerminalDimensions, TerminalUnderlineStyle};
use alacritty_terminal::{event::VoidListener, term::Config};

/// Builds a surface by feeding ANSI bytes through the normal Alacritty parser path.
fn surface_from_ansi(bytes: &[u8], cols: usize, rows: usize) -> TerminalSurface {
    let dimensions = TerminalDimensions { cols, rows };
    let config = Config {
        scrolling_history: 128,
        ..Config::default()
    };
    let mut terminal =
        alacritty_terminal::term::Term::<VoidListener>::new(config, &dimensions, VoidListener);
    let mut parser = alacritty_terminal::vte::ansi::Processor::<
        alacritty_terminal::vte::ansi::StdSyncHandler,
    >::new();
    parser.advance(&mut terminal, bytes);
    build_surface(&terminal)
}

/// Verifies the ANSI surface bridge preserves the text styling flags NeoZeus needs to render.
#[test]
fn build_surface_preserves_cell_style_flags() {
    let surface = surface_from_ansi(
        b"\x1b[1mA\x1b[0m\x1b[3mB\x1b[0m\x1b[2mC\x1b[0m\x1b[9mD\x1b[0m\x1b[4mE\x1b[0m\x1b[4:2mF\x1b[0m\x1b[4:3mG\x1b[0m\x1b[4:4mH\x1b[0m\x1b[4:5mI\x1b[0m",
        16,
        2,
    );

    assert!(surface.cell(0, 0).style.bold);
    assert!(surface.cell(1, 0).style.italic);
    assert!(surface.cell(2, 0).style.dim);
    assert!(surface.cell(3, 0).style.strikeout);
    assert_eq!(
        surface.cell(4, 0).style.underline,
        TerminalUnderlineStyle::Single
    );
    assert_eq!(
        surface.cell(5, 0).style.underline,
        TerminalUnderlineStyle::Double
    );
    assert_eq!(
        surface.cell(6, 0).style.underline,
        TerminalUnderlineStyle::Curly
    );
    assert_eq!(
        surface.cell(7, 0).style.underline,
        TerminalUnderlineStyle::Dotted
    );
    assert_eq!(
        surface.cell(8, 0).style.underline,
        TerminalUnderlineStyle::Dashed
    );
}

/// Verifies the ANSI bridge preserves underline color and truecolor background data.
#[test]
fn build_surface_preserves_underline_color_and_background() {
    let surface = surface_from_ansi(
        b"\x1b[58:2::1:2:3m\x1b[4mX\x1b[0m\x1b[48;2;10;20;30mY\x1b[0m",
        8,
        2,
    );

    assert_eq!(
        surface.cell(0, 0).style.underline_color,
        Some(egui::Color32::from_rgb(1, 2, 3))
    );
    assert_eq!(
        surface.cell(0, 0).style.underline,
        TerminalUnderlineStyle::Single
    );
    assert_eq!(surface.cell(1, 0).bg, egui::Color32::from_rgb(10, 20, 30));
}

#[test]
fn surface_from_ansi_text_auto_size_preserves_multiline_rows() {
    let surface = surface_from_ansi_text_auto_size("line one\nline two\n");

    assert_eq!(surface.rows, 3);
    assert_eq!(surface.cols, 8);
    assert_eq!(surface.cell(0, 0).content.to_owned_string(), "l");
    assert_eq!(surface.cell(5, 1).content.to_owned_string(), "t");
}
