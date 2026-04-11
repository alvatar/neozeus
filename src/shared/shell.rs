/// Quotes one string for safe embedding in a POSIX shell command line.
///
/// Simple ASCII path/session-like values are left unquoted for readability; everything else is
/// single-quoted with embedded single quotes escaped using the standard `'\''` sequence.
pub fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::shell_quote;

    #[test]
    fn shell_quote_leaves_simple_session_like_values_unquoted() {
        assert_eq!(shell_quote("neozeus-session-1"), "neozeus-session-1");
        assert_eq!(shell_quote("/tmp/demo:path"), "/tmp/demo:path");
    }

    #[test]
    fn shell_quote_quotes_empty_and_spaced_values() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("two words"), "'two words'");
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("it's fine"), "'it'\\''s fine'");
    }
}
