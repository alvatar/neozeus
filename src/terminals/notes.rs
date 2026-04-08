use super::debug::append_debug_log;
use bevy::prelude::*;
use std::{collections::HashMap, env, fs, path::PathBuf};

const TERMINAL_NOTES_FILENAME: &str = "notes.v1";
const TERMINAL_NOTES_VERSION_V1: &str = "version 1";
const TERMINAL_NOTES_VERSION_V2: &str = "version 2";
const TERMINAL_NOTES_SAVE_DEBOUNCE_SECS: f32 = 0.3;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PersistedTerminalNotes {
    pub(crate) notes_by_agent_uid: HashMap<String, String>,
    pub(crate) legacy_notes_by_session: HashMap<String, String>,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalNotesState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
    notes_by_agent_uid: HashMap<String, String>,
    legacy_notes_by_session: HashMap<String, String>,
}

impl TerminalNotesState {
    /// Replaces the entire notes map with freshly loaded persisted data and clears dirty state.
    ///
    /// Loading is treated as authoritative state replacement, not as a merge, because the persisted
    /// file is the single source of truth for note text at startup.
    pub(crate) fn load(&mut self, notes: PersistedTerminalNotes) {
        self.notes_by_agent_uid = notes.notes_by_agent_uid;
        self.legacy_notes_by_session = notes.legacy_notes_by_session;
        self.dirty_since_secs = None;
    }

    /// Returns the stored note text for one session, if any.
    ///
    /// The returned slice borrows directly from the internal map so callers can inspect notes without
    /// allocating.
    pub(crate) fn note_text(&self, session_name: &str) -> Option<&str> {
        self.legacy_notes_by_session
            .get(session_name)
            .map(String::as_str)
    }

    pub(crate) fn remove_legacy_note_text(&mut self, session_name: &str) -> bool {
        self.legacy_notes_by_session.remove(session_name).is_some()
    }

    pub(crate) fn note_text_by_agent_uid(&self, agent_uid: &str) -> Option<&str> {
        self.notes_by_agent_uid.get(agent_uid).map(String::as_str)
    }

    /// Sets or clears the note text for one session and reports whether anything actually changed.
    ///
    /// Trailing whitespace is trimmed before storage, and a fully blank result removes the entry from
    /// the map altogether. Existing strings are edited in place when possible to avoid replacing the
    /// allocation unnecessarily.
    pub(crate) fn set_note_text(&mut self, session_name: &str, text: &str) -> bool {
        set_note_text_in_map(&mut self.legacy_notes_by_session, session_name, text)
    }

    pub(crate) fn set_note_text_by_agent_uid(&mut self, agent_uid: &str, text: &str) -> bool {
        set_note_text_in_map(&mut self.notes_by_agent_uid, agent_uid, text)
    }

    pub(crate) fn remove_note_text_by_agent_uid(&mut self, agent_uid: &str) -> bool {
        self.notes_by_agent_uid.remove(agent_uid).is_some()
    }
}

fn set_note_text_in_map(notes: &mut HashMap<String, String>, key: &str, text: &str) -> bool {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return notes.remove(key).is_some();
    }

    match notes.get_mut(key) {
        Some(existing) if existing == trimmed => false,
        Some(existing) => {
            existing.clear();
            existing.push_str(trimmed);
            true
        }
        None => {
            notes.insert(key.to_owned(), trimmed.to_owned());
            true
        }
    }
}

/// Resolves the notes persistence path from explicit directory inputs.
///
/// The precedence mirrors the rest of NeoZeus persistence: XDG state home first, then the legacy
/// `~/.local/state` path, then XDG config as a final fallback.
fn resolve_terminal_notes_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
) -> Option<PathBuf> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
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

/// Resolves the live notes persistence path from the current process environment.
///
/// This is the runtime wrapper around [`resolve_terminal_notes_path_with`] used during startup.
pub(crate) fn resolve_terminal_notes_path() -> Option<PathBuf> {
    resolve_terminal_notes_path_with(
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
}

/// Loads the notes map from disk, treating missing files as an empty note store.
///
/// Read failures other than `NotFound` are logged and also fall back to an empty map, because notes
/// persistence should not block application startup.
pub(crate) fn load_terminal_notes_from(path: &PathBuf) -> PersistedTerminalNotes {
    match fs::read_to_string(path) {
        Ok(text) => parse_terminal_notes(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            PersistedTerminalNotes::default()
        }
        Err(error) => {
            append_debug_log(format!(
                "terminal notes load failed {}: {error}",
                path.display()
            ));
            PersistedTerminalNotes::default()
        }
    }
}

/// Parses the line-oriented notes persistence format into a per-session map.
///
/// The parser first validates the version header, then reads repeated `note name=...` blocks whose
/// bodies terminate at a lone `.` line. Leading `.` in note content is escaped by doubling it, so the
/// parser also has to undo that escaping on load.
fn parse_terminal_notes(text: &str) -> PersistedTerminalNotes {
    let mut lines = text.lines();
    let Some(version) = lines.next() else {
        return PersistedTerminalNotes::default();
    };
    let version = version.trim();
    if version != TERMINAL_NOTES_VERSION_V1 && version != TERMINAL_NOTES_VERSION_V2 {
        append_debug_log(format!(
            "terminal notes: unexpected version line `{version}`"
        ));
        return PersistedTerminalNotes::default();
    }

    let mut notes = PersistedTerminalNotes::default();
    while let Some(line) = lines.next() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let (agent_uid, session_name) = if let Some(raw) = line.strip_prefix("note agent_uid=") {
            (Some(raw.replace("\\s", " ")), None)
        } else if let Some(raw) = line.strip_prefix("note name=") {
            (None, Some(raw.replace("\\s", " ")))
        } else {
            continue;
        };
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
        if let Some(agent_uid) = agent_uid.filter(|value| !value.is_empty()) {
            if !note_text.is_empty() {
                notes.notes_by_agent_uid.insert(agent_uid, note_text);
            }
        } else if let Some(session_name) = session_name.filter(|value| !value.is_empty()) {
            if !note_text.is_empty() {
                notes
                    .legacy_notes_by_session
                    .insert(session_name, note_text);
            }
        }
    }

    notes
}

/// Serializes the notes map into the line-oriented persistence format.
///
/// Sessions are sorted by name for deterministic output. Empty session names and blank note payloads
/// are skipped, and note lines beginning with `.` are escaped by prefixing an extra dot so block
/// terminators remain unambiguous.
fn serialize_terminal_notes(notes: &PersistedTerminalNotes) -> String {
    let mut agent_notes = notes
        .notes_by_agent_uid
        .iter()
        .filter_map(|(agent_uid, note_text)| {
            let trimmed = note_text.trim();
            (!agent_uid.is_empty() && !trimmed.is_empty()).then_some((agent_uid.as_str(), trimmed))
        })
        .collect::<Vec<_>>();
    agent_notes.sort_by(|left, right| left.0.cmp(right.0));

    let mut legacy_notes = notes
        .legacy_notes_by_session
        .iter()
        .filter_map(|(session_name, note_text)| {
            let trimmed = note_text.trim();
            (!session_name.is_empty() && !trimmed.is_empty())
                .then_some((session_name.as_str(), trimmed))
        })
        .collect::<Vec<_>>();
    legacy_notes.sort_by(|left, right| left.0.cmp(right.0));

    let mut output = String::from(TERMINAL_NOTES_VERSION_V2);
    output.push('\n');
    for (agent_uid, note_text) in agent_notes {
        output.push_str("note agent_uid=");
        output.push_str(&agent_uid.replace(' ', "\\s"));
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
    let _ = legacy_notes;
    output
}

/// Marks the notes store dirty, recording the first dirty timestamp if it was previously clean.
///
/// The first-write-wins timestamp is what powers the later debounce logic in
/// [`save_terminal_notes_if_dirty`].
pub(crate) fn mark_terminal_notes_dirty(notes_state: &mut TerminalNotesState, time: Option<&Time>) {
    if notes_state.dirty_since_secs.is_none() {
        notes_state.dirty_since_secs = Some(time.map(Time::elapsed_secs).unwrap_or(0.0));
    }
}

/// Writes the notes file once the dirty debounce window has elapsed.
///
/// The system exits early while clean or still inside the debounce window, creates parent
/// directories on demand, writes the serialized note map, logs success or failure, and finally clears
/// the dirty marker either way so repeated failing saves do not loop forever.
pub(crate) fn save_terminal_notes_if_dirty(
    time: Res<Time>,
    mut notes_state: ResMut<TerminalNotesState>,
) {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
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

    let serialized = serialize_terminal_notes(&PersistedTerminalNotes {
        notes_by_agent_uid: notes_state.notes_by_agent_uid.clone(),
        legacy_notes_by_session: notes_state.legacy_notes_by_session.clone(),
    });
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TaskHeader<'a> {
    unchecked: bool,
    suffix: &'a str,
}

/// Parses one line as a Zeus-style checkbox task header.
///
/// The parser accepts `- [ ] ...` as unchecked and `- [x] ...`/`- [X] ...` as checked, returning the
/// remainder of the line as the task suffix so callers can reconstruct or rewrite the line.
fn parse_task_header(line: &str) -> Option<TaskHeader<'_>> {
    // Process the input incrementally so each transformation stays local and malformed data fails at the narrowest point.
    let trimmed = line.trim_start();
    let dash = trimmed.strip_prefix('-')?;
    let after_dash = dash.trim_start();
    let bracketed = after_dash.strip_prefix('[')?;
    let bracket_end = bracketed.find(']')?;
    let inner = &bracketed[..bracket_end];
    let inner_trimmed = inner.trim();
    let unchecked = if inner_trimmed.is_empty() {
        true
    } else if inner_trimmed.eq_ignore_ascii_case("x") {
        false
    } else {
        return None;
    };

    Some(TaskHeader {
        unchecked,
        suffix: &bracketed[bracket_end + 1..],
    })
}

/// Removes all completed checkbox task blocks from note text.
///
/// A "done task block" means a checked header line plus any immediately following non-header detail
/// lines. The function returns both the updated text and the number of removed task blocks.
pub(crate) fn clear_done_tasks(text: &str) -> (String, usize) {
    // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return (String::new(), 0);
    }

    let mut kept = Vec::new();
    let mut removed = 0;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        match parse_task_header(line) {
            Some(header) if !header.unchecked => {
                removed += 1;
                index += 1;
                while index < lines.len() && parse_task_header(lines[index]).is_none() {
                    index += 1;
                }
            }
            _ => {
                kept.push(line);
                index += 1;
            }
        }
    }

    (kept.join("\n").trim_end().to_owned(), removed)
}

/// Extracts the next actionable task message from note text and returns the updated note text.
///
/// If checkbox tasks exist, the first unchecked task block is chosen, its message body is extracted,
/// and its checkbox is flipped to done in the returned note text. If there are no checkbox headers at
/// all, the function falls back to the first non-empty line and removes it from the notes instead.
pub(crate) fn extract_next_task(task_text: &str) -> Option<(String, String)> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let mut lines = task_text.lines().map(str::to_owned).collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    let pending_index = lines
        .iter()
        .position(|line| parse_task_header(line).is_some_and(|header| header.unchecked));

    if let Some(pending_index) = pending_index {
        let header = parse_task_header(&lines[pending_index])?;
        let mut end = lines.len();
        for (index, line) in lines.iter().enumerate().skip(pending_index + 1) {
            if parse_task_header(line).is_some() {
                end = index;
                break;
            }
        }

        let mut message_lines = Vec::new();
        let first_content = header.suffix.trim_start();
        if !first_content.is_empty() {
            message_lines.push(first_content.to_owned());
        }
        message_lines.extend(
            lines[pending_index + 1..end]
                .iter()
                .map(|line| line.trim_end().to_owned()),
        );
        let message = message_lines.join("\n").trim().to_owned();
        if message.is_empty() {
            return None;
        }

        if let Some(bracket_start) = lines[pending_index].find('[') {
            if let Some(bracket_end) = lines[pending_index][bracket_start..].find(']') {
                let bracket_end = bracket_start + bracket_end;
                let prefix = &lines[pending_index][..bracket_start];
                let suffix = &lines[pending_index][bracket_end + 1..];
                lines[pending_index] = format!("{prefix}[x]{suffix}");
            }
        }

        return Some((message, lines.join("\n").trim_end().to_owned()));
    }

    if lines.iter().any(|line| parse_task_header(line).is_some()) {
        return None;
    }

    let first_non_empty = lines.iter().position(|line| !line.trim().is_empty())?;
    let message = lines[first_non_empty].trim().to_owned();
    if message.is_empty() {
        return None;
    }
    lines.remove(first_non_empty);
    Some((message, lines.join("\n").trim_end().to_owned()))
}

/// Converts arbitrary text into a normalized unchecked task block.
///
/// The first non-empty trimmed line becomes the checkbox header, while remaining lines are preserved
/// as task detail lines verbatim. Fully blank input is rejected.
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

#[cfg(test)]
mod tests;
