fn uppercase_label_text(text: &str) -> String {
    text.to_uppercase()
}

pub fn uppercase_agent_label_text(text: &str) -> String {
    uppercase_label_text(text)
}

pub fn uppercase_owned_tmux_display_name_text(text: &str) -> String {
    uppercase_label_text(text)
}

#[cfg(test)]
mod tests {
    use super::{uppercase_agent_label_text, uppercase_owned_tmux_display_name_text};

    #[test]
    fn agent_and_owned_tmux_uppercase_rules_match() {
        for input in ["build", "Beta build", "ßeta"] {
            assert_eq!(
                uppercase_owned_tmux_display_name_text(input),
                uppercase_agent_label_text(input)
            );
        }
    }
}
