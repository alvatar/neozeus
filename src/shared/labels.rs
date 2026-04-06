pub fn uppercase_display_label_text(text: &str) -> String {
    text.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::uppercase_display_label_text;

    #[test]
    fn uppercase_display_label_text_matches_agent_label_behavior() {
        assert_eq!(uppercase_display_label_text("build"), "BUILD");
        assert_eq!(uppercase_display_label_text("Beta build"), "BETA BUILD");
        assert_eq!(uppercase_display_label_text("ßeta"), "SSETA");
    }
}
