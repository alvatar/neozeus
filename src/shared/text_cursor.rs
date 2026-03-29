/// Returns the previous UTF-8 character boundary before `index`, if any.
pub fn previous_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return None;
    }
    text[..index]
        .char_indices()
        .last()
        .map(|(offset, _)| offset)
}

/// Returns the next UTF-8 character boundary after `index`, if any.
pub fn next_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index >= text.len() {
        return None;
    }
    text[index..].chars().next().map(|ch| index + ch.len_utf8())
}

/// Moves backward to the start of the preceding word according to `is_word_char`.
pub fn word_backward_boundary(
    text: &str,
    cursor: usize,
    is_word_char: impl Fn(char) -> bool,
) -> usize {
    let mut current = cursor;
    while let Some((previous, ch)) = previous_char(text, current) {
        if is_word_char(ch) {
            break;
        }
        current = previous;
    }
    while let Some((previous, ch)) = previous_char(text, current) {
        if !is_word_char(ch) {
            break;
        }
        current = previous;
    }
    current
}

/// Moves forward to the end of the next word according to `is_word_char`.
pub fn word_forward_boundary(
    text: &str,
    cursor: usize,
    is_word_char: impl Fn(char) -> bool,
) -> usize {
    let mut current = cursor;
    while let Some((next, ch)) = next_char(text, current) {
        if is_word_char(ch) {
            break;
        }
        current = next;
    }
    while let Some((next, ch)) = next_char(text, current) {
        if !is_word_char(ch) {
            break;
        }
        current = next;
    }
    current
}

fn previous_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last()
}

fn next_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| (cursor + ch.len_utf8(), ch))
}

#[cfg(test)]
mod tests {
    use super::{
        next_char_boundary, previous_char_boundary, word_backward_boundary, word_forward_boundary,
    };

    #[test]
    fn char_boundaries_follow_utf8_codepoint_edges() {
        let text = "aéβ";
        assert_eq!(previous_char_boundary(text, text.len()), Some(3));
        assert_eq!(previous_char_boundary(text, 3), Some(1));
        assert_eq!(previous_char_boundary(text, 1), Some(0));
        assert_eq!(previous_char_boundary(text, 0), None);
        assert_eq!(next_char_boundary(text, 0), Some(1));
        assert_eq!(next_char_boundary(text, 1), Some(3));
        assert_eq!(next_char_boundary(text, 3), Some(5));
        assert_eq!(next_char_boundary(text, 5), None);
    }

    #[test]
    fn word_boundaries_support_whitespace_delimited_fields() {
        let text = "  foo bar";
        let is_word_char = |ch: char| !ch.is_whitespace();
        assert_eq!(word_backward_boundary(text, 9, is_word_char), 6);
        assert_eq!(word_backward_boundary(text, 6, is_word_char), 2);
        assert_eq!(word_forward_boundary(text, 2, is_word_char), 5);
        assert_eq!(word_forward_boundary(text, 5, is_word_char), 9);
    }

    #[test]
    fn word_boundaries_support_editor_word_char_policy() {
        let text = " ::foo_bar baz";
        let is_word_char = |ch: char| ch.is_alphanumeric() || ch == '_';
        assert_eq!(word_backward_boundary(text, 10, is_word_char), 3);
        assert_eq!(word_forward_boundary(text, 0, is_word_char), 10);
        assert_eq!(word_forward_boundary(text, 10, is_word_char), 14);
    }
}
