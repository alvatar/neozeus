use super::raster::CachedTerminalGlyph;

pub(crate) fn is_box_drawing(ch: char) -> bool {
    matches!(
        ch,
        '─' | '━' | '│' | '┃' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼'
    )
}

pub(crate) fn rasterize_box_drawing(
    ch: char,
    cell_width: u32,
    cell_height: u32,
) -> Option<CachedTerminalGlyph> {
    if !is_box_drawing(ch) {
        return None;
    }
    let width = cell_width.max(1);
    let height = cell_height.max(1);
    let mut pixels = vec![0; (width * height * 4) as usize];
    let stroke = stroke_width(ch, width, height);
    let mid_x = width / 2;
    let mid_y = height / 2;

    match ch {
        '─' | '━' => draw_h(&mut pixels, width, height, stroke, mid_y, 0, width - 1),
        '│' | '┃' => draw_v(&mut pixels, width, height, stroke, mid_x, 0, height - 1),
        '┌' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, mid_x, width - 1);
            draw_v(&mut pixels, width, height, stroke, mid_x, mid_y, height - 1);
        }
        '┐' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, 0, mid_x);
            draw_v(&mut pixels, width, height, stroke, mid_x, mid_y, height - 1);
        }
        '└' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, mid_x, width - 1);
            draw_v(&mut pixels, width, height, stroke, mid_x, 0, mid_y);
        }
        '┘' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, 0, mid_x);
            draw_v(&mut pixels, width, height, stroke, mid_x, 0, mid_y);
        }
        '├' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, mid_x, width - 1);
            draw_v(&mut pixels, width, height, stroke, mid_x, 0, height - 1);
        }
        '┤' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, 0, mid_x);
            draw_v(&mut pixels, width, height, stroke, mid_x, 0, height - 1);
        }
        '┬' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, 0, width - 1);
            draw_v(&mut pixels, width, height, stroke, mid_x, mid_y, height - 1);
        }
        '┴' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, 0, width - 1);
            draw_v(&mut pixels, width, height, stroke, mid_x, 0, mid_y);
        }
        '┼' => {
            draw_h(&mut pixels, width, height, stroke, mid_y, 0, width - 1);
            draw_v(&mut pixels, width, height, stroke, mid_x, 0, height - 1);
        }
        _ => return None,
    }

    Some(CachedTerminalGlyph {
        width,
        height,
        pixels,
        preserve_color: false,
    })
}

fn draw_h(buffer: &mut [u8], width: u32, height: u32, stroke: u32, y: u32, x0: u32, x1: u32) {
    for yy in y.saturating_sub(stroke / 2)..=(y + stroke / 2).min(height - 1) {
        for xx in x0.min(width - 1)..=x1.min(width - 1) {
            set_white(buffer, width, xx, yy);
        }
    }
}

fn draw_v(buffer: &mut [u8], width: u32, height: u32, stroke: u32, x: u32, y0: u32, y1: u32) {
    for xx in x.saturating_sub(stroke / 2)..=(x + stroke / 2).min(width - 1) {
        for yy in y0.min(height - 1)..=y1.min(height - 1) {
            set_white(buffer, width, xx, yy);
        }
    }
}

fn stroke_width(ch: char, cell_width: u32, cell_height: u32) -> u32 {
    let base = (cell_width.min(cell_height) / 8).max(1);
    if matches!(ch, '━' | '┃') {
        base.max(2)
    } else {
        base
    }
}

fn set_white(buffer: &mut [u8], width: u32, x: u32, y: u32) {
    let idx = ((y * width + x) * 4) as usize;
    buffer[idx] = 255;
    buffer[idx + 1] = 255;
    buffer[idx + 2] = 255;
    buffer[idx + 3] = 255;
}
