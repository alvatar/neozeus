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
            let segment_end_byte_in_line =
                wrapped_segment_end(line_text, segment_start_byte_in_line, max_visible_cols);
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

fn wrapped_segment_end(line_text: &str, start_byte: usize, max_visible_cols: usize) -> usize {
    let (candidate_end, _) = byte_after_n_chars(line_text, start_byte, max_visible_cols);
    if candidate_end >= line_text.len() {
        return line_text.len();
    }

    let candidate = &line_text[start_byte..candidate_end];
    let next_char = line_text[candidate_end..]
        .chars()
        .next()
        .expect("non-final candidate should have next char");
    let candidate_ends_with_whitespace = candidate.chars().last().is_some_and(char::is_whitespace);
    if next_char.is_whitespace() || candidate_ends_with_whitespace {
        return candidate_end;
    }

    if let Some((last_whitespace_byte, whitespace_char)) = candidate
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
    {
        let break_end = start_byte + last_whitespace_byte + whitespace_char.len_utf8();
        if break_end > start_byte {
            return break_end;
        }
    }

    candidate_end
}

fn byte_after_n_chars(text: &str, start_byte: usize, count: usize) -> (usize, usize) {
    let mut end_byte = start_byte;
    let mut taken = 0;
    for ch in text[start_byte..].chars().take(count) {
        end_byte += ch.len_utf8();
        taken += 1;
    }
    (end_byte, taken)
}
