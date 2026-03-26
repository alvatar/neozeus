use crate::terminals::{
    TerminalCell, TerminalCellContent, TerminalCursor, TerminalCursorShape, TerminalSurface,
};
use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    term::{cell::Flags, color::Colors, Term},
    vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Rgb},
};
use bevy_egui::egui;

// Builds surface.
pub(crate) fn build_surface(term: &Term<VoidListener>) -> TerminalSurface {
    let content = term.renderable_content();
    let cols = term.columns();
    let rows = term.screen_lines();
    let mut surface = TerminalSurface::new(cols, rows);

    for indexed in content.display_iter {
        let x = indexed.point.column.0;
        let y_i32 = indexed.point.line.0;
        if y_i32 < 0 {
            continue;
        }
        let y = y_i32 as usize;
        if x >= cols || y >= rows {
            continue;
        }

        let mut fg = resolve_alacritty_color(indexed.cell.fg, content.colors, true);
        let mut bg = resolve_alacritty_color(indexed.cell.bg, content.colors, false);
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
                width,
            },
        );
    }

    surface.cursor = Some(TerminalCursor {
        x: content.cursor.point.column.0.min(cols.saturating_sub(1)),
        y: content.cursor.point.line.0.max(0) as usize,
        shape: map_cursor_shape(content.cursor.shape),
        visible: content.cursor.shape != CursorShape::Hidden,
        color: resolve_alacritty_color(AnsiColor::Named(NamedColor::Cursor), content.colors, true),
    });
    surface
}

// Maps cursor shape.
fn map_cursor_shape(shape: CursorShape) -> TerminalCursorShape {
    match shape {
        CursorShape::Underline => TerminalCursorShape::Underline,
        CursorShape::Beam => TerminalCursorShape::Beam,
        CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => {
            TerminalCursorShape::Block
        }
    }
}

// Resolves Alacritty color.
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

// Implements fallback named RGB.
fn fallback_named_rgb(named: NamedColor, is_foreground: bool) -> Rgb {
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
        NamedColor::Foreground | NamedColor::BrightForeground => Rgb {
            r: 190,
            g: 190,
            b: 190,
        },
        NamedColor::Background => Rgb {
            r: 10,
            g: 10,
            b: 10,
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
                    r: 120,
                    g: 120,
                    b: 120,
                }
            } else {
                Rgb {
                    r: 10,
                    g: 10,
                    b: 10,
                }
            }
        }
    }
}

// Implements xterm indexed RGB.
pub(crate) fn xterm_indexed_rgb(index: u8) -> Rgb {
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
