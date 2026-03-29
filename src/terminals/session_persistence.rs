#[cfg(test)]
use crate::shared::text_escape::quote_escaped_string;
use crate::shared::text_escape::{unquote_escaped_string, EXTENDED_QUOTED_STRING_ESCAPES};

use super::debug::append_debug_log;
use std::{env, fs, path::PathBuf};

#[cfg(test)]
use super::{
    daemon::is_persistent_session_name,
    registry::{TerminalFocusState, TerminalManager},
};
#[cfg(test)]
use crate::agents::{AgentCatalog, AgentRuntimeIndex};
#[cfg(test)]
use bevy::prelude::*;
#[cfg(test)]
use std::collections::BTreeSet;

const TERMINAL_SESSIONS_FILENAME: &str = "terminals.v1";
const TERMINAL_SESSIONS_VERSION_V1: &str = "version 1";
const TERMINAL_SESSIONS_VERSION_V2: &str = "version 2";
#[cfg(test)]
const TERMINAL_SESSIONS_SAVE_DEBOUNCE_SECS: f32 = 0.3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalSessionRecord {
    pub(crate) session_name: String,
    pub(crate) label: Option<String>,
    pub(crate) creation_index: u64,
    pub(crate) last_focused: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PersistedTerminalSessions {
    pub(crate) sessions: Vec<TerminalSessionRecord>,
}

#[cfg(test)]
#[derive(Resource, Default)]
pub(crate) struct TerminalSessionPersistenceState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
}

/// Resolves the terminal-session persistence file path from explicit directory inputs.
///
/// The precedence matches the notes persistence path: XDG state home first, then `~/.local/state`,
/// then XDG config as a fallback.
fn resolve_terminal_sessions_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
) -> Option<PathBuf> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    if let Some(xdg_state_home) = xdg_state_home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg_state_home)
                .join("neozeus")
                .join(TERMINAL_SESSIONS_FILENAME),
        );
    }

    if let Some(home) = home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(home)
                .join(".local/state/neozeus")
                .join(TERMINAL_SESSIONS_FILENAME),
        );
    }

    if let Some(xdg_config_home) = xdg_config_home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg_config_home)
                .join("neozeus")
                .join(TERMINAL_SESSIONS_FILENAME),
        );
    }

    None
}

/// Resolves the live terminal-session persistence path from the current environment.
///
/// This is the runtime wrapper around [`resolve_terminal_sessions_path_with`].
pub(crate) fn resolve_terminal_sessions_path() -> Option<PathBuf> {
    resolve_terminal_sessions_path_with(
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
}

/// Parses a quoted string from the version-2 session persistence format.
///
/// Returning `None` on malformed input lets the higher-level parser skip bad fields without
/// panicking.
fn parse_quoted_string(value: &str) -> Option<String> {
    unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
}

/// Parses the legacy version-1 terminal-session persistence format.
///
/// Version 1 is a compact single-line-per-session format with escaped spaces in labels. Unknown or
/// malformed fields are skipped so old files remain broadly recoverable.
fn parse_v1_terminal_sessions(text: &str) -> PersistedTerminalSessions {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let mut persisted = PersistedTerminalSessions::default();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(kind) = parts.next() else {
            continue;
        };
        if kind != "session" {
            continue;
        }

        let mut session_name = None;
        let mut label = None;
        let mut creation_index = None;
        let mut last_focused = None;
        for part in parts {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            match key {
                "name" => session_name = Some(value.to_owned()),
                "label" => {
                    if !value.is_empty() {
                        label = Some(value.replace("\\s", " "));
                    }
                }
                "creation_index" => creation_index = value.parse::<u64>().ok(),
                "focused" => last_focused = value.parse::<u8>().ok().map(|flag| flag != 0),
                _ => {}
            }
        }

        let (Some(session_name), Some(creation_index), Some(last_focused)) =
            (session_name, creation_index, last_focused)
        else {
            continue;
        };
        persisted.sessions.push(TerminalSessionRecord {
            session_name,
            label,
            creation_index,
            last_focused,
        });
    }
    persisted
}

/// Parses the structured version-2 terminal-session persistence format.
///
/// Version 2 stores each session inside explicit `[session] ... [/session]` blocks and uses quoted
/// strings for names/labels, which avoids the escaping limitations of version 1.
fn parse_v2_terminal_sessions(text: &str) -> PersistedTerminalSessions {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let mut persisted = PersistedTerminalSessions::default();
    let mut session_name = None;
    let mut label = None;
    let mut creation_index = None;
    let mut last_focused = None;
    let mut in_session = false;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 {
            continue;
        }

        match line {
            "[session]" => {
                in_session = true;
                session_name = None;
                label = None;
                creation_index = None;
                last_focused = None;
            }
            "[/session]" => {
                if in_session {
                    if let (Some(session_name), Some(creation_index), Some(last_focused)) = (
                        session_name.take(),
                        creation_index.take(),
                        last_focused.take(),
                    ) {
                        persisted.sessions.push(TerminalSessionRecord {
                            session_name,
                            label: label.take(),
                            creation_index,
                            last_focused,
                        });
                    }
                }
                in_session = false;
            }
            _ if !in_session => {}
            _ => {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key {
                    "name" => session_name = parse_quoted_string(value),
                    "label" => label = parse_quoted_string(value),
                    "creation_index" => creation_index = value.parse::<u64>().ok(),
                    "focused" => last_focused = value.parse::<u8>().ok().map(|flag| flag != 0),
                    _ => {}
                }
            }
        }
    }

    persisted
}

/// Dispatches parsing to the correct persistence-format reader based on the version header.
///
/// Unknown versions are logged and treated as an empty persistence file rather than as a hard error,
/// which keeps startup resilient to corrupted or future-version files.
fn parse_persisted_terminal_sessions(text: &str) -> PersistedTerminalSessions {
    let version_line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    match version_line {
        TERMINAL_SESSIONS_VERSION_V1 => parse_v1_terminal_sessions(text),
        TERMINAL_SESSIONS_VERSION_V2 => parse_v2_terminal_sessions(text),
        line => {
            append_debug_log(format!(
                "terminal sessions: unexpected version line `{line}`"
            ));
            PersistedTerminalSessions::default()
        }
    }
}

/// Serializes terminal-session metadata into the current version-2 persistence format.
///
/// Sessions are emitted in creation-order order, with names and optional labels escaped through
/// [`escape_persisted_string`]. Version 2 is always written even though version 1 remains readable.
#[cfg(test)]
pub(crate) fn serialize_persisted_terminal_sessions(
    sessions: &PersistedTerminalSessions,
) -> String {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let mut output = String::from(TERMINAL_SESSIONS_VERSION_V2);
    output.push('\n');
    let mut ordered = sessions.sessions.clone();
    ordered.sort_by_key(|record| record.creation_index);
    for record in ordered {
        output.push_str("[session]\n");
        output.push_str(&format!(
            "name={}\n",
            quote_escaped_string(&record.session_name, EXTENDED_QUOTED_STRING_ESCAPES)
        ));
        if let Some(label) = record.label {
            output.push_str(&format!(
                "label={}\n",
                quote_escaped_string(&label, EXTENDED_QUOTED_STRING_ESCAPES)
            ));
        }
        output.push_str(&format!("creation_index={}\n", record.creation_index));
        output.push_str(&format!("focused={}\n", u8::from(record.last_focused)));
        output.push_str("[/session]\n");
    }
    output
}

/// Loads persisted terminal-session metadata from disk, defaulting to an empty set on failure.
///
/// Missing files are normal and return the default empty structure. Other read failures are logged
/// and also degrade to the default so startup can continue.
pub(crate) fn load_persisted_terminal_sessions_from(path: &PathBuf) -> PersistedTerminalSessions {
    match fs::read_to_string(path) {
        Ok(text) => parse_persisted_terminal_sessions(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            PersistedTerminalSessions::default()
        }
        Err(error) => {
            append_debug_log(format!(
                "terminal sessions load failed {}: {error}",
                path.display()
            ));
            PersistedTerminalSessions::default()
        }
    }
}

/// Builds the persistence snapshot that should be written for the current terminal state.
///
/// The snapshot uses terminal creation order, current focus state, and the agent label directory to
/// produce the compact persisted record list.
#[cfg(test)]
fn build_persisted_terminal_sessions(
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
) -> PersistedTerminalSessions {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let sessions = terminal_manager
        .terminal_ids()
        .iter()
        .enumerate()
        .filter_map(|(index, id)| {
            let terminal = terminal_manager.get(*id)?;
            Some(TerminalSessionRecord {
                session_name: terminal.session_name.clone(),
                label: runtime_index
                    .agent_for_terminal(*id)
                    .and_then(|agent_id| agent_catalog.label(agent_id))
                    .map(str::to_owned),
                creation_index: index as u64,
                last_focused: focus_state.active_id() == Some(*id),
            })
        })
        .collect();
    PersistedTerminalSessions { sessions }
}

/// Writes the terminal-session persistence file once the debounce window has elapsed.
///
/// The system exits early while clean or still debouncing, builds the current persistence snapshot,
/// ensures the parent directory exists, writes the serialized file, logs success/failure, and clears
/// the dirty marker either way.
#[cfg(test)]
pub(crate) fn save_terminal_sessions_if_dirty(
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    agent_catalog: Res<AgentCatalog>,
    runtime_index: Res<AgentRuntimeIndex>,
    mut persistence_state: ResMut<TerminalSessionPersistenceState>,
) {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let Some(dirty_since) = persistence_state.dirty_since_secs else {
        return;
    };
    if time.elapsed_secs() - dirty_since < TERMINAL_SESSIONS_SAVE_DEBOUNCE_SECS {
        return;
    }
    let Some(path) = persistence_state.path.as_ref() else {
        persistence_state.dirty_since_secs = None;
        return;
    };

    let persisted = build_persisted_terminal_sessions(
        &terminal_manager,
        &focus_state,
        &agent_catalog,
        &runtime_index,
    );
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            append_debug_log(format!(
                "terminal sessions mkdir failed {}: {error}",
                parent.display()
            ));
            persistence_state.dirty_since_secs = None;
            return;
        }
    }

    let serialized = serialize_persisted_terminal_sessions(&persisted);
    if let Err(error) = fs::write(path, serialized) {
        append_debug_log(format!(
            "terminal sessions save failed {}: {error}",
            path.display()
        ));
    } else {
        append_debug_log(format!("terminal sessions saved {}", path.display()));
    }
    persistence_state.dirty_since_secs = None;
}

/// Reconciles persisted terminal-session metadata against the daemon's currently live sessions.
///
/// The result is split into three buckets: sessions to restore, stale persisted sessions to prune,
/// and live persistent sessions to import. Imported sessions are assigned fresh creation indices after
/// the highest persisted index so restored ordering remains stable.
#[cfg(test)]
pub(crate) fn reconcile_terminal_sessions(
    persisted: &PersistedTerminalSessions,
    live_sessions: &[String],
) -> (
    Vec<TerminalSessionRecord>,
    Vec<TerminalSessionRecord>,
    Vec<TerminalSessionRecord>,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let live = live_sessions.iter().cloned().collect::<BTreeSet<_>>();
    let persisted_names = persisted
        .sessions
        .iter()
        .map(|record| record.session_name.clone())
        .collect::<BTreeSet<_>>();

    let restore = persisted
        .sessions
        .iter()
        .filter(|record| live.contains(&record.session_name))
        .cloned()
        .collect::<Vec<_>>();
    let prune = persisted
        .sessions
        .iter()
        .filter(|record| !live.contains(&record.session_name))
        .cloned()
        .collect::<Vec<_>>();

    let mut next_creation_index = persisted
        .sessions
        .iter()
        .map(|record| record.creation_index)
        .max()
        .map(|max| max + 1)
        .unwrap_or(0);
    let mut import = live_sessions
        .iter()
        .filter(|name| is_persistent_session_name(name))
        .filter(|name| !persisted_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    import.sort();
    let import = import
        .into_iter()
        .map(|session_name| {
            let record = TerminalSessionRecord {
                session_name,
                label: None,
                creation_index: next_creation_index,
                last_focused: false,
            };
            next_creation_index += 1;
            record
        })
        .collect();

    (restore, import, prune)
}

#[cfg(test)]
mod tests;
