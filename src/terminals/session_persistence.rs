use crate::{
    hud::AgentDirectory,
    terminals::{append_debug_log, is_persistent_session_name, TerminalManager},
};
use bevy::prelude::*;
use std::{collections::BTreeSet, env, fs, path::PathBuf};

const TERMINAL_SESSIONS_FILENAME: &str = "terminals.v1";
const TERMINAL_SESSIONS_VERSION: &str = "version 1";
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

#[derive(Resource, Default)]
pub(crate) struct TerminalSessionPersistenceState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReconciledTerminalSessions {
    pub(crate) restore: Vec<TerminalSessionRecord>,
    pub(crate) import: Vec<TerminalSessionRecord>,
    pub(crate) prune: Vec<TerminalSessionRecord>,
}

impl ReconciledTerminalSessions {
    pub(crate) fn ordered_sessions(&self) -> Vec<TerminalSessionRecord> {
        self.restore
            .iter()
            .chain(self.import.iter())
            .cloned()
            .collect()
    }
}

pub(crate) fn resolve_terminal_sessions_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
) -> Option<PathBuf> {
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

pub(crate) fn resolve_terminal_sessions_path() -> Option<PathBuf> {
    resolve_terminal_sessions_path_with(
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
}

pub(crate) fn parse_persisted_terminal_sessions(text: &str) -> PersistedTerminalSessions {
    let mut persisted = PersistedTerminalSessions::default();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 {
            if line != TERMINAL_SESSIONS_VERSION {
                append_debug_log(format!(
                    "terminal sessions: unexpected version line `{line}`"
                ));
                return PersistedTerminalSessions::default();
            }
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

pub(crate) fn serialize_persisted_terminal_sessions(
    sessions: &PersistedTerminalSessions,
) -> String {
    let mut output = String::from(TERMINAL_SESSIONS_VERSION);
    output.push('\n');
    let mut ordered = sessions.sessions.clone();
    ordered.sort_by_key(|record| record.creation_index);
    for record in ordered {
        output.push_str(&format!(
            "session name={} label={} creation_index={} focused={}\n",
            record.session_name,
            record
                .label
                .as_deref()
                .map(|label| label.replace(' ', "\\s"))
                .unwrap_or_default(),
            record.creation_index,
            u8::from(record.last_focused),
        ));
    }
    output
}

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

pub(crate) fn mark_terminal_sessions_dirty(
    persistence_state: &mut TerminalSessionPersistenceState,
    time: Option<&Time>,
) {
    if persistence_state.dirty_since_secs.is_none() {
        persistence_state.dirty_since_secs = Some(time.map(Time::elapsed_secs).unwrap_or(0.0));
    }
}

pub(crate) fn build_persisted_terminal_sessions(
    terminal_manager: &TerminalManager,
    focus_state: &crate::terminals::TerminalFocusState,
    agent_directory: &AgentDirectory,
) -> PersistedTerminalSessions {
    let sessions = terminal_manager
        .terminal_ids()
        .iter()
        .enumerate()
        .filter_map(|(index, id)| {
            let terminal = terminal_manager.get(*id)?;
            Some(TerminalSessionRecord {
                session_name: terminal.session_name.clone(),
                label: agent_directory.labels.get(id).cloned(),
                creation_index: index as u64,
                last_focused: focus_state.active_id() == Some(*id),
            })
        })
        .collect();
    PersistedTerminalSessions { sessions }
}

pub(crate) fn save_terminal_sessions_if_dirty(
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<crate::terminals::TerminalFocusState>,
    agent_directory: Res<AgentDirectory>,
    mut persistence_state: ResMut<TerminalSessionPersistenceState>,
) {
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

    let persisted =
        build_persisted_terminal_sessions(&terminal_manager, &focus_state, &agent_directory);
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

pub(crate) fn reconcile_terminal_sessions(
    persisted: &PersistedTerminalSessions,
    live_sessions: &[String],
) -> ReconciledTerminalSessions {
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

    ReconciledTerminalSessions {
        restore,
        import,
        prune,
    }
}
