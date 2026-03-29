/// Configures which backslash escape sequences are valid for one quoted-string format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QuotedStringEscapeRules {
    pub(crate) carriage_return: bool,
    pub(crate) tab: bool,
}

/// Quoted-string escape rules for the stricter conversation persistence format.
pub(crate) const BASIC_QUOTED_STRING_ESCAPES: QuotedStringEscapeRules = QuotedStringEscapeRules {
    carriage_return: false,
    tab: false,
};

/// Quoted-string escape rules for app/session persistence formats that support `\r` and `\t`.
pub(crate) const EXTENDED_QUOTED_STRING_ESCAPES: QuotedStringEscapeRules =
    QuotedStringEscapeRules {
        carriage_return: true,
        tab: true,
    };

/// Escapes raw text for use as the inner content of a quoted persisted string.
pub(crate) fn escape_quoted_string_content(value: &str, rules: QuotedStringEscapeRules) -> String {
    let mut escaped = String::with_capacity(value.len() + 4);
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' if rules.carriage_return => escaped.push_str("\\r"),
            '\t' if rules.tab => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Quotes and escapes one persisted string field.
pub(crate) fn quote_escaped_string(value: &str, rules: QuotedStringEscapeRules) -> String {
    format!("\"{}\"", escape_quoted_string_content(value, rules))
}

/// Parses one quoted persisted string field according to the supplied escape rules.
pub(crate) fn unquote_escaped_string(
    value: &str,
    rules: QuotedStringEscapeRules,
) -> Option<String> {
    let trimmed = value.trim();
    let inner = trimmed.strip_prefix('"')?.strip_suffix('"')?;
    let mut parsed = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            parsed.push(ch);
            continue;
        }
        match chars.next()? {
            '\\' => parsed.push('\\'),
            '"' => parsed.push('"'),
            'n' => parsed.push('\n'),
            'r' if rules.carriage_return => parsed.push('\r'),
            't' if rules.tab => parsed.push('\t'),
            _ => return None,
        }
    }
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::{
        quote_escaped_string, unquote_escaped_string, BASIC_QUOTED_STRING_ESCAPES,
        EXTENDED_QUOTED_STRING_ESCAPES,
    };

    #[test]
    fn quoted_string_roundtrips_extended_escape_set() {
        let original = "a\\b\n\r\t\"z";
        let quoted = quote_escaped_string(original, EXTENDED_QUOTED_STRING_ESCAPES);
        assert_eq!(
            unquote_escaped_string(&quoted, EXTENDED_QUOTED_STRING_ESCAPES),
            Some(original.to_owned())
        );
    }

    #[test]
    fn quoted_string_roundtrips_basic_escape_set() {
        let original = "a\\b\n\"z";
        let quoted = quote_escaped_string(original, BASIC_QUOTED_STRING_ESCAPES);
        assert_eq!(
            unquote_escaped_string(&quoted, BASIC_QUOTED_STRING_ESCAPES),
            Some(original.to_owned())
        );
    }

    #[test]
    fn quoted_string_rejects_escape_sequences_not_allowed_by_rules() {
        assert_eq!(
            unquote_escaped_string("\"bad\\r\"", BASIC_QUOTED_STRING_ESCAPES),
            None
        );
        assert_eq!(
            unquote_escaped_string("\"bad\\t\"", BASIC_QUOTED_STRING_ESCAPES),
            None
        );
    }

    #[test]
    fn quoted_string_rejects_malformed_escape_sequences() {
        assert_eq!(
            unquote_escaped_string("\"bad\\x\"", EXTENDED_QUOTED_STRING_ESCAPES),
            None
        );
        assert_eq!(
            unquote_escaped_string("\"dangling\\\"", EXTENDED_QUOTED_STRING_ESCAPES),
            None
        );
    }
}
