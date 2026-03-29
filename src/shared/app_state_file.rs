use std::{env, path::PathBuf};

pub(crate) const APP_STATE_FILENAME: &str = "neozeus-state.v1";
pub(crate) const APP_STATE_VERSION_V1: &str = "neozeus state version 1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PersistedAgentState {
    pub(crate) session_name: String,
    pub(crate) label: Option<String>,
    pub(crate) order_index: u64,
    pub(crate) last_focused: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PersistedAppState {
    pub(crate) agents: Vec<PersistedAgentState>,
}

/// Resolves the app-state persistence file path from explicit directory inputs.
pub(crate) fn resolve_app_state_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(xdg_state_home) = xdg_state_home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg_state_home)
                .join("neozeus")
                .join(APP_STATE_FILENAME),
        );
    }
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(home)
                .join(".local/state/neozeus")
                .join(APP_STATE_FILENAME),
        );
    }
    if let Some(xdg_config_home) = xdg_config_home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg_config_home)
                .join("neozeus")
                .join(APP_STATE_FILENAME),
        );
    }
    None
}

/// Resolves the live app-state persistence path from the current environment.
pub(crate) fn resolve_app_state_path() -> Option<PathBuf> {
    resolve_app_state_path_with(
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
}

fn parse_quoted_string(value: &str) -> Option<String> {
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
            'r' => parsed.push('\r'),
            't' => parsed.push('\t'),
            _ => return None,
        }
    }
    Some(parsed)
}

/// Parses version-1 persisted app-state text.
pub(crate) fn parse_persisted_app_state(text: &str) -> PersistedAppState {
    let version_line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    if version_line != APP_STATE_VERSION_V1 {
        return PersistedAppState::default();
    }

    let mut persisted = PersistedAppState::default();
    let mut session_name = None;
    let mut label = None;
    let mut order_index = None;
    let mut last_focused = None;
    let mut in_agent = false;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 {
            continue;
        }
        match line {
            "[agent]" => {
                in_agent = true;
                session_name = None;
                label = None;
                order_index = None;
                last_focused = None;
            }
            "[/agent]" => {
                if in_agent {
                    if let (Some(session_name), Some(order_index), Some(last_focused)) =
                        (session_name.take(), order_index.take(), last_focused.take())
                    {
                        persisted.agents.push(PersistedAgentState {
                            session_name,
                            label: label.take(),
                            order_index,
                            last_focused,
                        });
                    }
                }
                in_agent = false;
            }
            _ if !in_agent => {}
            _ => {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key {
                    "session_name" => session_name = parse_quoted_string(value),
                    "label" => label = parse_quoted_string(value),
                    "order_index" => order_index = value.parse::<u64>().ok(),
                    "focused" => last_focused = value.parse::<u8>().ok().map(|flag| flag != 0),
                    _ => {}
                }
            }
        }
    }

    persisted
}
