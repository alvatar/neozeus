use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCell {
    pub text: String,
    pub fg: Color32,
    pub bg: Color32,
    pub width: u8,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            text: String::new(),
            fg: Color32::from_rgb(220, 220, 220),
            bg: Color32::from_rgb(10, 10, 10),
            width: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCursorShape {
    Block,
    Underline,
    Beam,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCursor {
    pub x: usize,
    pub y: usize,
    pub shape: TerminalCursorShape,
    pub visible: bool,
    pub color: Color32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalSurface {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<TerminalCell>,
    pub cursor: Option<TerminalCursor>,
    pub title: Option<String>,
}

impl TerminalSurface {
    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![TerminalCell::default(); cols.saturating_mul(rows)],
            cursor: None,
            title: None,
        }
    }

    pub fn set_cell(&mut self, x: usize, y: usize, cell: TerminalCell) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        let index = y * self.cols + x;
        self.cells[index] = cell;
    }

    #[must_use]
    pub fn cell(&self, x: usize, y: usize) -> &TerminalCell {
        &self.cells[y * self.cols + x]
    }
}

pub fn paint_terminal(ui: &mut egui::Ui, surface: &TerminalSurface) {
    let available = ui.available_size();
    let desired = Vec2::new(available.x.max(64.0), available.y.max(64.0));
    let (response, painter) = ui.allocate_painter(desired, egui::Sense::click());
    let rect = response.rect;

    painter.rect_filled(rect, 0.0, Color32::from_rgb(10, 10, 10));

    if surface.cols == 0 || surface.rows == 0 {
        return;
    }

    let cell_w = rect.width() / surface.cols as f32;
    let cell_h = rect.height() / surface.rows as f32;
    let font_size = (cell_h * 0.82).max(8.0);
    let font = FontId::monospace(font_size);

    for y in 0..surface.rows {
        for x in 0..surface.cols {
            let cell = surface.cell(x, y);
            let min = Pos2::new(
                rect.left() + x as f32 * cell_w,
                rect.top() + y as f32 * cell_h,
            );
            let width = if cell.width <= 1 {
                cell_w
            } else {
                cell_w * f32::from(cell.width)
            };
            let cell_rect = Rect::from_min_size(min, Vec2::new(width, cell_h));
            painter.rect_filled(cell_rect, 0.0, cell.bg);

            if cell.width == 0 || cell.text.is_empty() {
                continue;
            }

            painter.text(
                Pos2::new(cell_rect.min.x + 1.0, cell_rect.min.y + 1.0),
                Align2::LEFT_TOP,
                cell.text.as_str(),
                font.clone(),
                cell.fg,
            );
        }
    }

    if let Some(cursor) = &surface.cursor {
        if cursor.visible && cursor.x < surface.cols && cursor.y < surface.rows {
            let min = Pos2::new(
                rect.left() + cursor.x as f32 * cell_w,
                rect.top() + cursor.y as f32 * cell_h,
            );
            let cursor_rect = Rect::from_min_size(min, Vec2::new(cell_w.max(1.0), cell_h.max(1.0)));
            match cursor.shape {
                TerminalCursorShape::Block => {
                    painter.rect_stroke(
                        cursor_rect.shrink(1.0),
                        0.0,
                        Stroke::new(1.5, cursor.color),
                        StrokeKind::Outside,
                    );
                }
                TerminalCursorShape::Underline => {
                    painter.line_segment(
                        [cursor_rect.left_bottom(), cursor_rect.right_bottom()],
                        Stroke::new(2.0, cursor.color),
                    );
                }
                TerminalCursorShape::Beam => {
                    painter.line_segment(
                        [cursor_rect.left_top(), cursor_rect.left_bottom()],
                        Stroke::new(2.0, cursor.color),
                    );
                }
            }
        }
    }
}
