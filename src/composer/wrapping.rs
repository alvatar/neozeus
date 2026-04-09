#[derive(Clone, Debug)]
pub(crate) struct WrappedTextRow<'a> {
    pub(crate) display_start_byte: usize,
    pub(crate) display_end_byte: usize,
    pub(crate) display_text: &'a str,
    pub(crate) segment_len: usize,
    pub(crate) cursor_col: Option<usize>,
}

/// Builds wrapped visual rows for the shared text-editor body and identifies the row containing the
/// cursor. Wrapping prefers a whitespace boundary only when the viewport edge would otherwise split
/// a word; long words still hard-wrap when no earlier boundary exists.
pub(crate) fn wrapped_text_rows<'a>(
    text: &'a str,
    max_visible_cols: usize,
    cursor: usize,
) -> (Vec<WrappedTextRow<'a>>, usize) {
    let max_visible_cols = max_visible_cols.max(1);
    wrapped_text_rows_measured(text, cursor, max_visible_cols as f32, |segment| {
        segment.chars().count() as f32
    })
}

pub(crate) fn wrapped_text_rows_measured<'a, F>(
    text: &'a str,
    cursor: usize,
    max_width: f32,
    mut measure: F,
) -> (Vec<WrappedTextRow<'a>>, usize)
where
    F: FnMut(&str) -> f32,
{
    let max_width = max_width.max(1.0);
    let mut rows = Vec::new();
    let mut cursor_row = 0;
    let mut cursor_assigned = false;

    for (line_start_byte, line_end_byte, line_text) in text_lines(text) {
        if line_text.is_empty() {
            let row_index = rows.len();
            let cursor_col =
                if !cursor_assigned && cursor >= line_start_byte && cursor <= line_end_byte {
                    cursor_assigned = true;
                    cursor_row = row_index;
                    Some(0)
                } else {
                    None
                };
            rows.push(WrappedTextRow {
                display_start_byte: line_start_byte,
                display_end_byte: line_end_byte,
                display_text: "",
                segment_len: 0,
                cursor_col,
            });
            continue;
        }

        let mut segment_start_byte_in_line = 0usize;
        while segment_start_byte_in_line < line_text.len() {
            let segment_end_byte_in_line = wrapped_segment_end_measured(
                line_text,
                segment_start_byte_in_line,
                max_width,
                &mut measure,
            );
            let display_text = &line_text[segment_start_byte_in_line..segment_end_byte_in_line];
            let segment_len = display_text.chars().count();
            let display_start_byte = line_start_byte + segment_start_byte_in_line;
            let display_end_byte = line_start_byte + segment_end_byte_in_line;
            let row_index = rows.len();
            let cursor_col =
                if !cursor_assigned && cursor >= display_start_byte && cursor <= display_end_byte {
                    cursor_assigned = true;
                    cursor_row = row_index;
                    Some(
                        text[display_start_byte..cursor.min(display_end_byte)]
                            .chars()
                            .count(),
                    )
                } else {
                    None
                };
            rows.push(WrappedTextRow {
                display_start_byte,
                display_end_byte,
                display_text,
                segment_len,
                cursor_col,
            });
            segment_start_byte_in_line = segment_end_byte_in_line;
        }
    }

    if rows.is_empty() {
        rows.push(WrappedTextRow {
            display_start_byte: 0,
            display_end_byte: 0,
            display_text: "",
            segment_len: 0,
            cursor_col: Some(0),
        });
    }

    (rows, cursor_row)
}

fn text_lines(text: &str) -> Vec<(usize, usize, &str)> {
    text.split('\n')
        .scan(0usize, |start, line| {
            let line_start = *start;
            let line_end = line_start + line.len();
            *start = line_end.saturating_add(1);
            Some((line_start, line_end, line))
        })
        .collect()
}

fn wrapped_segment_end_measured<F>(
    line_text: &str,
    start_byte: usize,
    max_width: f32,
    measure: &mut F,
) -> usize
where
    F: FnMut(&str) -> f32,
{
    let mut last_fit_end = start_byte;
    let mut last_whitespace_fit_end = None;
    let mut advanced = false;

    for (relative_start, ch) in line_text[start_byte..].char_indices() {
        let candidate_end = start_byte + relative_start + ch.len_utf8();
        let candidate = &line_text[start_byte..candidate_end];
        let fits = measure(candidate) <= max_width;
        if fits || !advanced {
            last_fit_end = candidate_end;
            advanced = true;
            if ch.is_whitespace() {
                last_whitespace_fit_end = Some(candidate_end);
            }
            continue;
        }
        break;
    }

    if last_fit_end >= line_text.len() {
        return line_text.len();
    }
    if let Some(last_whitespace_fit_end) = last_whitespace_fit_end {
        if last_whitespace_fit_end > start_byte {
            return last_whitespace_fit_end;
        }
    }
    last_fit_end.max(
        start_byte
            + line_text[start_byte..]
                .chars()
                .next()
                .map_or(0, char::len_utf8),
    )
}

#[cfg(test)]
mod tests {
    use super::{wrapped_text_rows_measured, WrappedTextRow};

    fn displays<'a>(rows: &'a [WrappedTextRow<'a>]) -> Vec<&'a str> {
        rows.iter().map(|row| row.display_text).collect()
    }

    #[test]
    fn measured_wrap_pushes_last_word_to_next_row_when_render_width_would_clip_it() {
        let (rows, cursor_row) = wrapped_text_rows_measured("fit wide", 8, 6.0, |segment| {
            segment.chars().count() as f32 * 0.9 + segment.matches('w').count() as f32 * 1.8
        });
        assert_eq!(displays(&rows), vec!["fit ", "wide"]);
        assert_eq!(cursor_row, 1);
        assert_eq!(rows[1].cursor_col, Some(4));
    }
}
