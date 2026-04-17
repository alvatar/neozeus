use super::*;

/// Extracts a substring by character indices rather than byte indices.
///
/// The editor viewport logic uses this to window UTF-8 text safely.
fn slice_chars(text: &str, start_chars: usize, max_chars: usize) -> String {
    text.chars().skip(start_chars).take(max_chars).collect()
}

#[cfg(test)]
fn message_box_lines(text: &str) -> Vec<(usize, usize, &str)> {
    text.split('\n')
        .scan(0usize, |start, line| {
            let line_start = *start;
            let line_end = line_start + line.len();
            *start = line_end.saturating_add(1);
            Some((line_start, line_end, line))
        })
        .collect()
}

/// Builds the short status string describing the editor's current selection state.
///
/// A real region wins, then a bare mark, then the default "no mark" message.
pub(super) fn editor_selection_status(editor: &TextEditorState) -> String {
    editor
        .region_bounds()
        .map(|(start, end)| format!("Region {} chars", editor.text[start..end].chars().count()))
        .or_else(|| editor.mark.map(|_| "Mark set".to_owned()))
        .unwrap_or_else(|| "No mark".to_owned())
}

#[cfg(test)]
type WrappedEditorRow<'a> = crate::composer::WrappedTextRow<'a>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CursorVisualSpan<'a> {
    InvertedGlyph {
        leading_text: &'a str,
        glyph: &'a str,
        trailing_text: &'a str,
    },
    BoundaryBlock {
        leading_text: &'a str,
    },
}

fn byte_index_for_char(text: &str, chars: usize) -> usize {
    let mut byte_index = 0;
    for ch in text.chars().take(chars) {
        byte_index += ch.len_utf8();
    }
    byte_index
}

pub(super) fn cursor_visual_span(display_text: &str, cursor_col: usize) -> CursorVisualSpan<'_> {
    let display_len = display_text.chars().count();
    if cursor_col < display_len {
        let glyph_start = byte_index_for_char(display_text, cursor_col);
        let glyph_end = byte_index_for_char(display_text, cursor_col + 1);
        CursorVisualSpan::InvertedGlyph {
            leading_text: &display_text[..glyph_start],
            glyph: &display_text[glyph_start..glyph_end],
            trailing_text: &display_text[glyph_end..],
        }
    } else {
        CursorVisualSpan::BoundaryBlock {
            leading_text: display_text,
        }
    }
}

#[cfg(test)]
fn cursor_byte_for_line_column(text: &str, cursor_line: usize, cursor_col: usize) -> usize {
    let Some((line_start_byte, line_end_byte, line_text)) =
        message_box_lines(text).into_iter().nth(cursor_line)
    else {
        return text.len();
    };
    let bounded_cursor = cursor_col.min(line_text.chars().count());
    line_start_byte
        + byte_index_for_char(line_text, bounded_cursor).min(line_end_byte - line_start_byte)
}

pub(super) fn active_line_bounds(text: &str, cursor: usize) -> (usize, usize) {
    let line_start = text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let line_end = text[cursor..]
        .find('\n')
        .map(|offset| cursor + offset)
        .unwrap_or(text.len());
    (line_start, line_end)
}

pub(super) fn wrapped_row_is_active(
    row: &crate::composer::WrappedTextRow<'_>,
    active_line: (usize, usize),
) -> bool {
    (row.line_start_byte, row.line_end_byte) == active_line
}

#[cfg(test)]
pub(super) fn wrapped_editor_rows<'a>(
    text: &'a str,
    max_visible_cols: usize,
    cursor_line: usize,
    cursor_col: usize,
) -> (Vec<WrappedEditorRow<'a>>, usize) {
    crate::composer::wrapped_text_rows(
        text,
        max_visible_cols,
        cursor_byte_for_line_column(text, cursor_line, cursor_col),
    )
}

/// Draws the wrapped modal text editor body, including visible lines, selection, and cursor.
pub(super) fn draw_text_editor_body(
    painter: &mut HudPainter,
    window: &Window,
    editor: &TextEditorState,
    body_rect: HudRect,
    focused: bool,
) {
    painter.fill_rect(body_rect, HudColors::MESSAGE_BOX, 6.0);
    painter.stroke_rect(
        body_rect,
        if focused {
            HudColors::TEXT_MUTED
        } else {
            HudColors::BUTTON_BORDER
        },
        4.0,
    );

    let line_height = 24.0;
    let text_size = 18.0;
    let content_x = body_rect.x + 18.0;
    let content_y = body_rect.y + 16.0;
    let max_visible_lines = ((body_rect.h - 24.0) / line_height).floor().max(1.0) as usize;
    let selection = editor.region_bounds();
    let active_line = active_line_bounds(&editor.text, editor.cursor);
    let text_width = (body_rect.w - 36.0).max(1.0);
    let (rows, cursor_row) =
        wrapped_text_rows_measured(&editor.text, editor.cursor, text_width, |segment| {
            painter.text_size(segment, text_size).x
        });
    let start_row = cursor_row.saturating_sub(max_visible_lines.saturating_sub(1));
    let end_row = (start_row + max_visible_lines).min(rows.len());

    painter.scene.push_clip_layer(
        Fill::NonZero,
        Affine::IDENTITY,
        &hud_rect_to_scene(window, body_rect),
    );

    for (visible_index, row) in rows[start_row..end_row].iter().enumerate() {
        let y = content_y + visible_index as f32 * line_height;

        if wrapped_row_is_active(row, active_line) {
            painter.fill_rect(
                HudRect {
                    x: body_rect.x + 8.0,
                    y: y - 3.0,
                    w: body_rect.w - 16.0,
                    h: line_height,
                },
                HudColors::ROW_HOVERED,
                4.0,
            );
        }

        if let Some((selection_start, selection_end)) = selection {
            let row_selection_start = selection_start.max(row.display_start_byte);
            let row_selection_end = selection_end.min(row.display_end_byte);
            if row_selection_start < row_selection_end {
                let local_start = editor.text[row.display_start_byte..row_selection_start]
                    .chars()
                    .count();
                let local_end = editor.text[row.display_start_byte..row_selection_end]
                    .chars()
                    .count();
                let before_selection = slice_chars(row.display_text, 0, local_start);
                let before_selection_end = slice_chars(row.display_text, 0, local_end);
                let selection_x = content_x + painter.text_size(&before_selection, text_size).x;
                let selection_end_x =
                    content_x + painter.text_size(&before_selection_end, text_size).x;
                painter.fill_rect(
                    HudRect {
                        x: selection_x,
                        y: y - 2.0,
                        w: (selection_end_x - selection_x).max(6.0),
                        h: line_height - 4.0,
                    },
                    HudColors::ROW_FOCUSED,
                    3.0,
                );
            }
        }

        if focused {
            if let Some(visible_cursor_col) = row.cursor_col {
                match cursor_visual_span(row.display_text, visible_cursor_col) {
                    CursorVisualSpan::InvertedGlyph {
                        leading_text,
                        glyph,
                        trailing_text,
                    } => {
                        if !leading_text.is_empty() {
                            painter.label(
                                Vec2::new(content_x, y),
                                leading_text,
                                text_size,
                                HudColors::TEXT,
                                VelloTextAnchor::TopLeft,
                            );
                        }
                        let cursor_x = content_x + painter.text_size(leading_text, text_size).x;
                        let glyph_width = painter.text_size(glyph, text_size).x.max(10.0);
                        painter.fill_rect(
                            HudRect {
                                x: cursor_x - 1.0,
                                y: y - 1.0,
                                w: glyph_width + 2.0,
                                h: 22.0,
                            },
                            HudColors::TEXT,
                            2.0,
                        );
                        painter.label(
                            Vec2::new(cursor_x, y),
                            glyph,
                            text_size,
                            HudColors::MESSAGE_BOX,
                            VelloTextAnchor::TopLeft,
                        );
                        if !trailing_text.is_empty() {
                            painter.label(
                                Vec2::new(cursor_x + glyph_width, y),
                                trailing_text,
                                text_size,
                                HudColors::TEXT,
                                VelloTextAnchor::TopLeft,
                            );
                        }
                    }
                    CursorVisualSpan::BoundaryBlock { leading_text } => {
                        if !leading_text.is_empty() {
                            painter.label(
                                Vec2::new(content_x, y),
                                leading_text,
                                text_size,
                                HudColors::TEXT,
                                VelloTextAnchor::TopLeft,
                            );
                        }
                        let cursor_x = content_x + painter.text_size(leading_text, text_size).x;
                        painter.fill_rect(
                            HudRect {
                                x: cursor_x - 1.0,
                                y: y - 1.0,
                                w: 11.0,
                                h: 22.0,
                            },
                            HudColors::TEXT,
                            2.0,
                        );
                    }
                }
            } else if !row.display_text.is_empty() {
                painter.label(
                    Vec2::new(content_x, y),
                    row.display_text,
                    text_size,
                    HudColors::TEXT,
                    VelloTextAnchor::TopLeft,
                );
            }
        } else if !row.display_text.is_empty() {
            painter.label(
                Vec2::new(content_x, y),
                row.display_text,
                text_size,
                HudColors::TEXT,
                VelloTextAnchor::TopLeft,
            );
        }
    }

    painter.scene.pop_layer();
}

/// Draws a simple row of modal action buttons from precomputed rect/label pairs.
///
/// This helper is shared by both the message box and the task dialog so they keep identical button
/// chrome.
pub(super) fn draw_dialog_button_row<I, L>(
    painter: &mut HudPainter,
    buttons: I,
) where
    I: IntoIterator<Item = (HudRect, L, bool)>,
    L: Into<String>,
{
    for (rect, label, focused) in buttons {
        let label = label.into();
        painter.fill_rect(rect, HudColors::BUTTON, 0.0);
        painter.stroke_rect(
            rect,
            if focused {
                HudColors::TEXT
            } else {
                HudColors::BUTTON_BORDER
            },
            0.0,
        );
        painter.label(
            Vec2::new(rect.x + 10.0, rect.y + 6.0),
            &label,
            14.0,
            if focused {
                HudColors::TEXT
            } else {
                HudColors::TEXT_MUTED
            },
            VelloTextAnchor::TopLeft,
        );
    }
}

pub(super) fn single_line_field_viewport(
    text: &str,
    cursor: usize,
    max_visible_cols: usize,
) -> (usize, usize, String) {
    let safe_cursor = cursor.min(text.len());
    let cursor_col = text[..safe_cursor].chars().count();
    let start_col = cursor_col.saturating_sub(max_visible_cols.saturating_sub(1));
    let visible_cursor_col = cursor_col.saturating_sub(start_col);
    let display_text = slice_chars(text, start_col, max_visible_cols);
    (start_col, visible_cursor_col, display_text)
}

/// Draws one single-line dialog field with optional focus cursor.
pub(super) fn draw_single_line_dialog_field(
    painter: &mut HudPainter,
    window: &Window,
    field: &TextFieldState,
    rect: HudRect,
    focused: bool,
) {
    painter.fill_rect(rect, HudColors::BUTTON, 4.0);
    painter.stroke_rect(
        rect,
        if focused {
            HudColors::TEXT
        } else {
            HudColors::BUTTON_BORDER
        },
        4.0,
    );

    let content_rect = HudRect {
        x: rect.x + 10.0,
        y: rect.y + 5.0,
        w: (rect.w - 20.0).max(1.0),
        h: (rect.h - 10.0).max(1.0),
    };
    let max_visible_cols = ((content_rect.w - 4.0) / 9.0).floor().max(4.0) as usize;
    let (_start_col, visible_cursor_col, display_text) =
        single_line_field_viewport(&field.text, field.cursor, max_visible_cols);

    painter.scene.push_clip_layer(
        Fill::NonZero,
        Affine::IDENTITY,
        &hud_rect_to_scene(window, content_rect),
    );
    if !display_text.is_empty() {
        painter.label(
            Vec2::new(content_rect.x, rect.y + 7.0),
            &display_text,
            15.0,
            HudColors::TEXT,
            VelloTextAnchor::TopLeft,
        );
    }
    if focused {
        let before_cursor = slice_chars(&display_text, 0, visible_cursor_col);
        let cursor_x = content_rect.x + painter.text_size(&before_cursor, 15.0).x;
        painter.fill_rect(
            HudRect {
                x: cursor_x,
                y: rect.y + 6.0,
                w: 2.0,
                h: rect.h - 12.0,
            },
            HudColors::TEXT,
            0.0,
        );
    }
    painter.scene.pop_layer();
}
