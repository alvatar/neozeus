use crate::terminals::append_debug_log;
use bevy::prelude::*;
use std::{collections::HashMap, env, fs, path::PathBuf};

const TERMINAL_NOTES_FILENAME: &str = "notes.v1";
const TERMINAL_NOTES_VERSION: &str = "version 1";
const TERMINAL_NOTES_SAVE_DEBOUNCE_SECS: f32 = 0.3;

#[derive(Resource, Default)]
pub(crate) struct TerminalNotesState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
    notes_by_session: HashMap<String, String>,
}

impl TerminalNotesState {
    pub(crate) fn load(&mut self, notes_by_session: HashMap<String, String>) {
        self.notes_by_session = notes_by_session;
        self.dirty_since_secs = None;
    }

    pub(crate) fn note_text(&self, session_name: &str) -> Option<&str> {
        self.notes_by_session.get(session_name).map(String::as_str)
    }

    pub(crate) fn has_note_text(&self, session_name: &str) -> bool {
        self.note_text(session_name)
            .is_some_and(|text| !text.trim().is_empty())
    }

    pub(crate) fn append_task_from_text(&mut self, session_name: &str, text: &str) -> bool {
        let Some(task_entry) = task_entry_from_text(text) else {
            return false;
        };
        let existing = self
            .notes_by_session
            .get(session_name)
            .map(|text| text.trim_end().to_owned())
            .unwrap_or_default();
        let updated = if existing.is_empty() {
            task_entry
        } else {
            format!("{existing}\n{task_entry}")
        };
        self.notes_by_session
            .insert(session_name.to_owned(), updated);
        true
    }

    pub(crate) fn prepend_task_from_text(&mut self, session_name: &str, text: &str) -> bool {
        let Some(task_entry) = task_entry_from_text(text) else {
            return false;
        };
        let existing = self
            .notes_by_session
            .get(session_name)
            .map(|text| text.trim_end().to_owned())
            .unwrap_or_default();
        let updated = if existing.is_empty() {
            task_entry
        } else {
            format!("{task_entry}\n{existing}")
        };
        self.notes_by_session
            .insert(session_name.to_owned(), updated);
        true
    }
}

pub(crate) fn resolve_terminal_notes_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(xdg_state_home) = xdg_state_home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg_state_home)
                .join("neozeus")
                .join(TERMINAL_NOTES_FILENAME),
        );
    }

    if let Some(home) = home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(home)
                .join(".local/state/neozeus")
                .join(TERMINAL_NOTES_FILENAME),
        );
    }

    if let Some(xdg_config_home) = xdg_config_home.filter(|value| !value.is_empty()) {
        return Some(
            PathBuf::from(xdg_config_home)
                .join("neozeus")
                .join(TERMINAL_NOTES_FILENAME),
        );
    }

    None
}

pub(crate) fn resolve_terminal_notes_path() -> Option<PathBuf> {
    resolve_terminal_notes_path_with(
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
}

pub(crate) fn load_terminal_notes_from(path: &PathBuf) -> HashMap<String, String> {
    match fs::read_to_string(path) {
        Ok(text) => parse_terminal_notes(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(error) => {
            append_debug_log(format!(
                "terminal notes load failed {}: {error}",
                path.display()
            ));
            HashMap::new()
        }
    }
}

pub(crate) fn parse_terminal_notes(text: &str) -> HashMap<String, String> {
    let mut lines = text.lines();
    let Some(version) = lines.next() else {
        return HashMap::new();
    };
    if version.trim() != TERMINAL_NOTES_VERSION {
        append_debug_log(format!(
            "terminal notes: unexpected version line `{version}`"
        ));
        return HashMap::new();
    }

    let mut notes = HashMap::new();
    while let Some(line) = lines.next() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let Some(name) = line.strip_prefix("note name=") else {
            continue;
        };
        let session_name = name.replace("\\s", " ");
        let mut note_lines = Vec::new();
        for note_line in lines.by_ref() {
            if note_line == "." {
                break;
            }
            note_lines.push(
                note_line
                    .strip_prefix("..")
                    .map(|line| format!(".{line}"))
                    .unwrap_or_else(|| note_line.to_owned()),
            );
        }
        let note_text = note_lines.join("\n").trim().to_owned();
        if !session_name.is_empty() && !note_text.is_empty() {
            notes.insert(session_name, note_text);
        }
    }

    notes
}

pub(crate) fn serialize_terminal_notes(notes_by_session: &HashMap<String, String>) -> String {
    let mut sessions = notes_by_session
        .iter()
        .filter_map(|(session_name, note_text)| {
            let trimmed = note_text.trim();
            (!session_name.is_empty() && !trimmed.is_empty())
                .then_some((session_name.as_str(), trimmed))
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| left.0.cmp(right.0));

    let mut output = String::from(TERMINAL_NOTES_VERSION);
    output.push('\n');
    for (session_name, note_text) in sessions {
        output.push_str("note name=");
        output.push_str(&session_name.replace(' ', "\\s"));
        output.push('\n');
        for line in note_text.lines() {
            if line.starts_with('.') {
                output.push('.');
            }
            output.push_str(line);
            output.push('\n');
        }
        output.push_str(".\n");
    }
    output
}

pub(crate) fn mark_terminal_notes_dirty(notes_state: &mut TerminalNotesState, time: Option<&Time>) {
    if notes_state.dirty_since_secs.is_none() {
        notes_state.dirty_since_secs = Some(time.map(Time::elapsed_secs).unwrap_or(0.0));
    }
}

pub(crate) fn save_terminal_notes_if_dirty(
    time: Res<Time>,
    mut notes_state: ResMut<TerminalNotesState>,
) {
    let Some(dirty_since) = notes_state.dirty_since_secs else {
        return;
    };
    if time.elapsed_secs() - dirty_since < TERMINAL_NOTES_SAVE_DEBOUNCE_SECS {
        return;
    }
    let Some(path) = notes_state.path.as_ref() else {
        notes_state.dirty_since_secs = None;
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            append_debug_log(format!(
                "terminal notes mkdir failed {}: {error}",
                parent.display()
            ));
            notes_state.dirty_since_secs = None;
            return;
        }
    }

    let serialized = serialize_terminal_notes(&notes_state.notes_by_session);
    if let Err(error) = fs::write(path, serialized) {
        append_debug_log(format!(
            "terminal notes save failed {}: {error}",
            path.display()
        ));
    } else {
        append_debug_log(format!("terminal notes saved {}", path.display()));
    }
    notes_state.dirty_since_secs = None;
}

pub(crate) fn task_entry_from_text(text: &str) -> Option<String> {
    let clean = text.trim();
    if clean.is_empty() {
        return None;
    }

    let mut lines = clean.lines();
    let first = lines.next()?.trim();
    let mut task_entry = format!("- [ ] {first}");
    for line in lines {
        task_entry.push('\n');
        task_entry.push_str(line);
    }
    Some(task_entry)
}
