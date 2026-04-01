use super::types::{
    TerminalCell, TerminalCellContent, TerminalCellStyle, TerminalCursor, TerminalCursorShape,
    TerminalSurface, TerminalUnderlineStyle,
};
use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    term::{cell::Flags, color::Colors, Term},
    vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Rgb},
};
use bevy_egui::egui;

/// Converts Alacritty's renderable terminal snapshot into NeoZeus's plain [`TerminalSurface`]
/// representation.
///
/// The conversion walks every visible cell, translates colors and cursor shape, handles inverse text,
/// collapses hidden/wide spacer cells into empty content, and records a width of 0/1/2 so the later
/// raster path can reason about wide characters without depending on Alacritty types.
pub(crate) fn build_surface(term: &Term<VoidListener>) -> TerminalSurface {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let renderable = term.renderable_content();
    let cols = term.columns();
    let rows = term.screen_lines();
    let mut surface = TerminalSurface::new(cols, rows);

    for indexed in renderable.display_iter {
        let x = indexed.point.column.0;
        let y_i32 = indexed.point.line.0;
        if y_i32 < 0 {
            continue;
        }
        let y = y_i32 as usize;
        if x >= cols || y >= rows {
            continue;
        }

        let mut fg = resolve_alacritty_color(indexed.cell.fg, renderable.colors, true);
        let mut bg = resolve_alacritty_color(indexed.cell.bg, renderable.colors, false);
        if indexed.cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }

        let content = if indexed.cell.flags.contains(Flags::HIDDEN)
            || indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            || indexed.cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            TerminalCellContent::Empty
        } else {
            TerminalCellContent::from_parts(indexed.cell.c, indexed.cell.zerowidth())
        };

        let width = if indexed.cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else if indexed
            .cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            0
        } else {
            1
        };

        surface.set_cell(
            x,
            y,
            TerminalCell {
                content,
                fg,
                bg,
                style: terminal_cell_style(
                    indexed.cell.flags,
                    indexed.cell.underline_color(),
                    renderable.colors,
                ),
                width,
            },
        );
    }

    surface.cursor = Some(TerminalCursor {
        x: renderable.cursor.point.column.0.min(cols.saturating_sub(1)),
        y: renderable.cursor.point.line.0.max(0) as usize,
        shape: map_cursor_shape(renderable.cursor.shape),
        visible: renderable.cursor.shape != CursorShape::Hidden,
        color: resolve_alacritty_color(
            AnsiColor::Named(NamedColor::Cursor),
            renderable.colors,
            true,
        ),
    });
    surface
}

/// Maps Alacritty cursor-shape variants onto NeoZeus's smaller cursor-shape enum.
///
/// Hidden and hollow-block cursors are both represented as block cursors here; visibility is carried
/// separately on the cursor record itself.
fn map_cursor_shape(shape: CursorShape) -> TerminalCursorShape {
    match shape {
        CursorShape::Underline => TerminalCursorShape::Underline,
        CursorShape::Beam => TerminalCursorShape::Beam,
        CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => {
            TerminalCursorShape::Block
        }
    }
}

/// Converts Alacritty style flags plus optional underline color into NeoZeus cell styling metadata.
fn terminal_cell_style(
    flags: Flags,
    underline_color: Option<AnsiColor>,
    colors: &Colors,
) -> TerminalCellStyle {
    TerminalCellStyle {
        bold: flags.intersects(Flags::BOLD | Flags::BOLD_ITALIC | Flags::DIM_BOLD),
        italic: flags.intersects(Flags::ITALIC | Flags::BOLD_ITALIC),
        dim: flags.intersects(Flags::DIM | Flags::DIM_BOLD),
        underline: terminal_underline_style(flags),
        strikeout: flags.contains(Flags::STRIKEOUT),
        underline_color: underline_color.map(|color| resolve_alacritty_color(color, colors, true)),
    }
}

/// Maps Alacritty's underline-variant flags to NeoZeus's underline-style enum.
fn terminal_underline_style(flags: Flags) -> TerminalUnderlineStyle {
    if flags.contains(Flags::UNDERCURL) {
        TerminalUnderlineStyle::Curly
    } else if flags.contains(Flags::DOTTED_UNDERLINE) {
        TerminalUnderlineStyle::Dotted
    } else if flags.contains(Flags::DASHED_UNDERLINE) {
        TerminalUnderlineStyle::Dashed
    } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
        TerminalUnderlineStyle::Double
    } else if flags.contains(Flags::UNDERLINE) {
        TerminalUnderlineStyle::Single
    } else {
        TerminalUnderlineStyle::None
    }
}

/// Resolves one Alacritty color reference into a concrete RGB color.
///
/// Explicit RGB values and indexed colors are handled directly; named colors first consult
/// Alacritty's current color table and only fall back to NeoZeus defaults when the table entry is
/// absent.
pub(crate) fn resolve_alacritty_color(
    color: AnsiColor,
    colors: &Colors,
    is_foreground: bool,
) -> egui::Color32 {
    let rgb = match color {
        AnsiColor::Spec(rgb) => rgb,
        AnsiColor::Indexed(index) => xterm_indexed_rgb(index),
        AnsiColor::Named(named) => match colors[named] {
            Some(rgb) => rgb,
            None => fallback_named_rgb(named, is_foreground),
        },
    };
    egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

/// Supplies NeoZeus fallback RGB values for named Alacritty colors when the live color table does
/// not define them.
///
/// Most names map to fixed hard-coded palette values. The `is_foreground` parameter is only needed
/// for the default foreground/background family later in the match so callers can preserve the usual
/// foreground/background distinction even in fallback mode.
fn fallback_named_rgb(named: NamedColor, is_foreground: bool) -> Rgb {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    match named {
        NamedColor::Black => Rgb { r: 0, g: 0, b: 0 },
        NamedColor::Red => Rgb {
            r: 204,
            g: 85,
            b: 85,
        },
        NamedColor::Green => Rgb {
            r: 85,
            g: 204,
            b: 85,
        },
        NamedColor::Yellow => Rgb {
            r: 205,
            g: 205,
            b: 85,
        },
        NamedColor::Blue => Rgb {
            r: 84,
            g: 85,
            b: 203,
        },
        NamedColor::Magenta => Rgb {
            r: 204,
            g: 85,
            b: 204,
        },
        NamedColor::Cyan => Rgb {
            r: 122,
            g: 202,
            b: 202,
        },
        NamedColor::White => Rgb {
            r: 204,
            g: 204,
            b: 204,
        },
        NamedColor::BrightBlack => Rgb {
            r: 85,
            g: 85,
            b: 85,
        },
        NamedColor::BrightRed => Rgb {
            r: 255,
            g: 85,
            b: 85,
        },
        NamedColor::BrightGreen => Rgb {
            r: 85,
            g: 255,
            b: 85,
        },
        NamedColor::BrightYellow => Rgb {
            r: 255,
            g: 255,
            b: 85,
        },
        NamedColor::BrightBlue => Rgb {
            r: 85,
            g: 85,
            b: 255,
        },
        NamedColor::BrightMagenta => Rgb {
            r: 255,
            g: 85,
            b: 255,
        },
        NamedColor::BrightCyan => Rgb {
            r: 85,
            g: 255,
            b: 255,
        },
        NamedColor::BrightWhite => Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        NamedColor::Foreground => Rgb {
            r: 102,
            g: 102,
            b: 102,
        },
        NamedColor::BrightForeground => Rgb {
            r: 128,
            g: 128,
            b: 128,
        },
        NamedColor::Background => Rgb {
            r: 24,
            g: 32,
            b: 30,
        },
        NamedColor::Cursor => Rgb {
            r: 82,
            g: 173,
            b: 112,
        },
        NamedColor::DimBlack => Rgb {
            r: 40,
            g: 40,
            b: 40,
        },
        NamedColor::DimRed => Rgb {
            r: 120,
            g: 50,
            b: 50,
        },
        NamedColor::DimGreen => Rgb {
            r: 50,
            g: 120,
            b: 50,
        },
        NamedColor::DimYellow => Rgb {
            r: 120,
            g: 120,
            b: 50,
        },
        NamedColor::DimBlue => Rgb {
            r: 50,
            g: 50,
            b: 120,
        },
        NamedColor::DimMagenta => Rgb {
            r: 120,
            g: 50,
            b: 120,
        },
        NamedColor::DimCyan => Rgb {
            r: 50,
            g: 120,
            b: 120,
        },
        NamedColor::DimWhite | NamedColor::DimForeground => {
            if is_foreground {
                Rgb {
                    r: 102,
                    g: 102,
                    b: 102,
                }
            } else {
                Rgb {
                    r: 24,
                    g: 32,
                    b: 30,
                }
            }
        }
    }
}

/// Maps an xterm 256-color palette index to its concrete RGB triple.
///
/// Indices `0..16` use the ANSI base palette, `16..232` use the 6×6×6 color cube, and the tail uses
/// the grayscale ramp.
pub(crate) fn xterm_indexed_rgb(index: u8) -> Rgb {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    const ANSI: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcc, 0x55, 0x55),
        (0x55, 0xcc, 0x55),
        (0xcd, 0xcd, 0x55),
        (0x54, 0x55, 0xcb),
        (0xcc, 0x55, 0xcc),
        (0x7a, 0xca, 0xca),
        (0xcc, 0xcc, 0xcc),
        (0x55, 0x55, 0x55),
        (0xff, 0x55, 0x55),
        (0x55, 0xff, 0x55),
        (0xff, 0xff, 0x55),
        (0x55, 0x55, 0xff),
        (0xff, 0x55, 0xff),
        (0x55, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];

    if index < 16 {
        let (r, g, b) = ANSI[index as usize];
        return Rgb { r, g, b };
    }

    if index < 232 {
        const RAMP6: [u8; 6] = [0, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
        let idx = index - 16;
        let blue = RAMP6[(idx % 6) as usize];
        let green = RAMP6[((idx / 6) % 6) as usize];
        let red = RAMP6[((idx / 36) % 6) as usize];
        return Rgb {
            r: red,
            g: green,
            b: blue,
        };
    }

    let grey = 0x08 + (index - 232) * 10;
    Rgb {
        r: grey,
        g: grey,
        b: grey,
    }
}

#[cfg(test)]
mod tests;
