use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    terminals::{
        append_debug_log, load_persisted_terminal_sessions_from, resolve_terminal_sessions_path,
        TerminalFocusState,
    },
};
use bevy::prelude::*;
use std::{collections::BTreeSet, env, fs, path::PathBuf};

const APP_STATE_FILENAME: &str = "neozeus-state.v1";
const APP_STATE_VERSION_V1: &str = "neozeus state version 1";
const APP_STATE_SAVE_DEBOUNCE_SECS: f32 = 0.3;

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

#[derive(Resource, Default)]
pub(crate) struct AppStatePersistenceState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
}

/// Returns the persisted agents that should be attached at startup in effective display order.
pub(crate) fn ordered_reconciled_persisted_agents(
    restore: &[PersistedAgentState],
    import: &[PersistedAgentState],
) -> Vec<PersistedAgentState> {
    restore.iter().chain(import.iter()).cloned().collect()
}

/// Resolves the app-state persistence file path from explicit directory inputs.
fn resolve_app_state_path_with(
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

fn escape_persisted_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 4);
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
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

fn parse_persisted_app_state(text: &str) -> PersistedAppState {
    let version_line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    if version_line != APP_STATE_VERSION_V1 {
        append_debug_log(format!(
            "app state: unexpected version line `{version_line}`"
        ));
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

/// Serializes persisted app-state metadata into the current version-1 app-state format.
pub(crate) fn serialize_persisted_app_state(state: &PersistedAppState) -> String {
    let mut output = String::from(APP_STATE_VERSION_V1);
    output.push('\n');
    let mut ordered = state.agents.clone();
    ordered.sort_by_key(|record| record.order_index);
    for record in ordered {
        output.push_str("[agent]\n");
        output.push_str(&format!(
            "session_name=\"{}\"\n",
            escape_persisted_string(&record.session_name)
        ));
        if let Some(label) = record.label {
            output.push_str(&format!("label=\"{}\"\n", escape_persisted_string(&label)));
        }
        output.push_str(&format!("order_index={}\n", record.order_index));
        output.push_str(&format!("focused={}\n", u8::from(record.last_focused)));
        output.push_str("[/agent]\n");
    }
    output
}

fn map_legacy_sessions_to_app_state(
    legacy: &crate::terminals::PersistedTerminalSessions,
) -> PersistedAppState {
    PersistedAppState {
        agents: legacy
            .sessions
            .iter()
            .map(|record| PersistedAgentState {
                session_name: record.session_name.clone(),
                label: record.label.clone(),
                order_index: record.creation_index,
                last_focused: record.last_focused,
            })
            .collect(),
    }
}

/// Loads persisted app-state metadata from disk, falling back to legacy terminal-session state when
/// the new app-state file does not yet exist.
pub(crate) fn load_persisted_app_state_from(path: &PathBuf) -> PersistedAppState {
    match fs::read_to_string(path) {
        Ok(text) => {
            let version_line = text
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(str::trim)
                .unwrap_or_default();
            if version_line == APP_STATE_VERSION_V1 {
                parse_persisted_app_state(&text)
            } else {
                map_legacy_sessions_to_app_state(&load_persisted_terminal_sessions_from(path))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            resolve_terminal_sessions_path()
                .as_ref()
                .filter(|legacy_path| legacy_path.exists())
                .map(load_persisted_terminal_sessions_from)
                .as_ref()
                .map(map_legacy_sessions_to_app_state)
                .unwrap_or_default()
        }
        Err(error) => {
            append_debug_log(format!("app state load failed {}: {error}", path.display()));
            PersistedAppState::default()
        }
    }
}

/// Marks app-state persistence dirty, recording the first dirty timestamp if needed.
pub(crate) fn mark_app_state_dirty(
    persistence_state: &mut AppStatePersistenceState,
    time: Option<&Time>,
) {
    if persistence_state.dirty_since_secs.is_none() {
        persistence_state.dirty_since_secs = Some(time.map(Time::elapsed_secs).unwrap_or(0.0));
    }
}

fn build_persisted_app_state(
    focus_state: &TerminalFocusState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
) -> PersistedAppState {
    let agents = agent_catalog
        .order
        .iter()
        .enumerate()
        .filter_map(|(index, agent_id)| {
            let session_name = runtime_index.session_name(*agent_id)?;
            let terminal_id = runtime_index.primary_terminal(*agent_id);
            Some(PersistedAgentState {
                session_name: session_name.to_owned(),
                label: agent_catalog.label(*agent_id).map(str::to_owned),
                order_index: index as u64,
                last_focused: terminal_id
                    .is_some_and(|terminal_id| focus_state.active_id() == Some(terminal_id)),
            })
        })
        .collect();
    PersistedAppState { agents }
}

/// Writes the app-state persistence file once the debounce window has elapsed.
pub(crate) fn save_app_state_if_dirty(
    time: Res<Time>,
    focus_state: Res<TerminalFocusState>,
    agent_catalog: Res<AgentCatalog>,
    runtime_index: Res<AgentRuntimeIndex>,
    mut persistence_state: ResMut<AppStatePersistenceState>,
) {
    let Some(dirty_since) = persistence_state.dirty_since_secs else {
        return;
    };
    if time.elapsed_secs() - dirty_since < APP_STATE_SAVE_DEBOUNCE_SECS {
        return;
    }
    let Some(path) = persistence_state.path.as_ref() else {
        persistence_state.dirty_since_secs = None;
        return;
    };
    let persisted = build_persisted_app_state(&focus_state, &agent_catalog, &runtime_index);
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            append_debug_log(format!(
                "app state mkdir failed {}: {error}",
                parent.display()
            ));
            persistence_state.dirty_since_secs = None;
            return;
        }
    }
    let serialized = serialize_persisted_app_state(&persisted);
    if let Err(error) = fs::write(path, serialized) {
        append_debug_log(format!("app state save failed {}: {error}", path.display()));
    } else {
        append_debug_log(format!("app state saved {}", path.display()));
    }
    persistence_state.dirty_since_secs = None;
}

/// Reconciles persisted app-state agent metadata against the daemon's currently live sessions.
pub(crate) fn reconcile_persisted_agents(
    persisted: &PersistedAppState,
    live_sessions: &[String],
) -> (
    Vec<PersistedAgentState>,
    Vec<PersistedAgentState>,
    Vec<PersistedAgentState>,
) {
    let live = live_sessions.iter().cloned().collect::<BTreeSet<_>>();
    let persisted_names = persisted
        .agents
        .iter()
        .map(|record| record.session_name.clone())
        .collect::<BTreeSet<_>>();

    let restore = persisted
        .agents
        .iter()
        .filter(|record| live.contains(&record.session_name))
        .cloned()
        .collect::<Vec<_>>();
    let prune = persisted
        .agents
        .iter()
        .filter(|record| !live.contains(&record.session_name))
        .cloned()
        .collect::<Vec<_>>();

    let mut next_order_index = persisted
        .agents
        .iter()
        .map(|record| record.order_index)
        .max()
        .map(|max| max + 1)
        .unwrap_or(0);
    let mut import = live_sessions
        .iter()
        .filter(|name| name.starts_with(crate::terminals::PERSISTENT_SESSION_PREFIX))
        .filter(|name| !persisted_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    import.sort();
    let import = import
        .into_iter()
        .map(|session_name| {
            let record = PersistedAgentState {
                session_name,
                label: None,
                order_index: next_order_index,
                last_focused: false,
            };
            next_order_index += 1;
            record
        })
        .collect();

    (restore, import, prune)
}

#[cfg(test)]
mod tests;
