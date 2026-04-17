use super::debug::append_debug_log;
use bevy::prelude::*;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use crate::shared::persistence::{
    first_non_empty_trimmed_line, load_text_file_or_default, mark_dirty_since,
    resolve_state_path_with, save_debounce_elapsed, write_file_atomically,
};

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

    pub(crate) fn clear_runtime_state(&mut self) {
        self.notes_by_agent_uid.clear();
        self.legacy_notes_by_session.clear();
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

    /// Test-only helper that mutates legacy session-keyed notes directly.
    ///
    /// Live runtime code must not write the legacy session-keyed map; only migration/restore paths
    /// may ingest it. The helper remains available to tests so legacy compatibility behavior can be
    /// exercised without exposing a general-purpose runtime write path.
    #[cfg(test)]
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
    resolve_state_path_with(
        xdg_state_home,
        home,
        xdg_config_home,
        "neozeus",
        TERMINAL_NOTES_FILENAME,
    )
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
pub(crate) fn load_terminal_notes_from(path: &Path) -> PersistedTerminalNotes {
    load_text_file_or_default(path, parse_terminal_notes, |path, error| {
        append_debug_log(format!(
            "terminal notes load failed {}: {error}",
            path.display()
        ));
    })
}

/// Parses the line-oriented notes persistence format into a per-session map.
///
/// The parser first validates the version header, then reads repeated `note name=...` blocks whose
/// bodies terminate at a lone `.` line. Leading `.` in note content is escaped by doubling it, so the
/// parser also has to undo that escaping on load.
fn parse_terminal_notes(text: &str) -> PersistedTerminalNotes {
    let version = first_non_empty_trimmed_line(text);
    if version.is_empty() {
        return PersistedTerminalNotes::default();
    }
    let mut lines = text.lines();
    for line in lines.by_ref() {
        if !line.trim().is_empty() {
            break;
        }
    }
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
    mark_dirty_since(&mut notes_state.dirty_since_secs, time);
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
    if !save_debounce_elapsed(
        notes_state.dirty_since_secs,
        time.elapsed_secs(),
        TERMINAL_NOTES_SAVE_DEBOUNCE_SECS,
    ) {
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
    if let Err(error) = write_file_atomically(path, &serialized) {
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
mod tests {
    use super::*;
    use crate::tests::temp_dir;
    use bevy::{
        ecs::system::RunSystemOnce,
        prelude::{Time, World},
    };
    use std::time::Duration;

    /// Appends one checkbox task parsed from raw text into the named session note.
    fn append_task_from_text(
        notes_state: &mut TerminalNotesState,
        session_name: &str,
        text: &str,
    ) -> bool {
        let Some(task_entry) = task_entry_from_text(text) else {
            return false;
        };
        let existing = notes_state
            .note_text(session_name)
            .map(str::trim_end)
            .unwrap_or_default();
        let updated = if existing.is_empty() {
            task_entry
        } else {
            format!("{existing}\n{task_entry}")
        };
        notes_state.set_note_text(session_name, &updated)
    }

    /// Prepends one checkbox task parsed from raw text into the named session note.
    fn prepend_task_from_text(
        notes_state: &mut TerminalNotesState,
        session_name: &str,
        text: &str,
    ) -> bool {
        let Some(task_entry) = task_entry_from_text(text) else {
            return false;
        };
        let existing = notes_state
            .note_text(session_name)
            .map(str::trim_end)
            .unwrap_or_default();
        let updated = if existing.is_empty() {
            task_entry
        } else {
            format!("{task_entry}\n{existing}")
        };
        notes_state.set_note_text(session_name, &updated)
    }

    /// Returns whether the named session currently has any non-blank note text.
    fn has_note_text(notes_state: &TerminalNotesState, session_name: &str) -> bool {
        notes_state
            .note_text(session_name)
            .is_some_and(|text| !text.trim().is_empty())
    }

    /// Verifies the text-to-task-entry formatter used when creating new checkbox tasks.
    ///
    /// A non-empty multi-line payload should become a Zeus-style `- [ ] ...` block, while all-whitespace
    /// input should be rejected.
    #[test]
    fn task_entry_from_text_matches_zeus_checkbox_format() {
        assert_eq!(
            task_entry_from_text("first line\n  detail line\nsecond detail"),
            Some("- [ ] first line\n  detail line\nsecond detail".to_owned())
        );
        assert_eq!(task_entry_from_text("  \n \t"), None);
    }

    /// Verifies the search-order logic for the terminal-notes persistence file.
    ///
    /// Notes follow the same state-home-first policy as session persistence, falling back through home
    /// state and then config paths.
    #[test]
    fn terminal_notes_path_prefers_state_home_then_home_state_then_config() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        assert_eq!(
            resolve_terminal_notes_path_with(
                Some("/tmp/state"),
                Some("/tmp/home"),
                Some("/tmp/config")
            ),
            Some(std::path::PathBuf::from("/tmp/state/neozeus/notes.v1"))
        );
        assert_eq!(
            resolve_terminal_notes_path_with(None, Some("/tmp/home"), Some("/tmp/config")),
            Some(std::path::PathBuf::from(
                "/tmp/home/.local/state/neozeus/notes.v1"
            ))
        );
        assert_eq!(
            resolve_terminal_notes_path_with(None, None, Some("/tmp/config")),
            Some(std::path::PathBuf::from("/tmp/config/neozeus/notes.v1"))
        );
    }

    /// Verifies that terminal notes serialize only stable agent-keyed entries.
    #[test]
    fn terminal_notes_serialize_drops_legacy_session_entries() {
        let notes = PersistedTerminalNotes {
            notes_by_agent_uid: std::collections::HashMap::from([(
                "agent-uid-a".to_owned(),
                "- [ ] first\n  detail".to_owned(),
            )]),
            legacy_notes_by_session: std::collections::HashMap::from([(
                "session-b".to_owned(),
                ".starts with dot".to_owned(),
            )]),
        };

        let serialized = serialize_terminal_notes(&notes);
        let reparsed = parse_terminal_notes(&serialized);

        assert_eq!(
            reparsed
                .notes_by_agent_uid
                .get("agent-uid-a")
                .map(String::as_str),
            Some("- [ ] first\n  detail")
        );
        assert!(reparsed.legacy_notes_by_session.is_empty());
    }

    /// Verifies that appending and prepending tasks preserve the expected Zeus task ordering.
    ///
    /// Prepending should place the new task block before existing tasks, appending should place it after,
    /// and the resulting stored note text should remain in checkbox-task format.
    #[test]
    fn terminal_notes_append_and_prepend_tasks_follow_zeus_ordering() {
        let mut notes_state = TerminalNotesState::default();
        assert!(append_task_from_text(
            &mut notes_state,
            "session-a",
            "second task"
        ));
        assert!(prepend_task_from_text(
            &mut notes_state,
            "session-a",
            "first task\n  detail",
        ));
        assert_eq!(
            notes_state.note_text("session-a"),
            Some("- [ ] first task\n  detail\n- [ ] second task")
        );
        assert!(has_note_text(&notes_state, "session-a"));
    }

    /// Verifies that clearing done tasks removes entire completed task blocks and leaves pending ones.
    ///
    /// The sample includes multi-line done blocks and trailing text so the helper's block-removal rules
    /// are exercised rather than just single-line checkbox deletion.
    #[test]
    fn clear_done_tasks_removes_done_task_blocks() {
        let (updated, removed) =
            clear_done_tasks("- [x] done one\n  detail\n- [ ] keep\n- [X] done two\n  more\ntrailing");
        assert_eq!(removed, 2);
        assert_eq!(updated, "- [ ] keep");
    }

    /// Verifies that extracting the next task returns the first pending task block and marks it done in
    /// the stored note text.
    ///
    /// The test confirms both outputs: the message payload sent to the terminal and the updated notes
    /// text with the first checkbox flipped to done.
    #[test]
    fn extract_next_task_marks_first_pending_block_done() {
        let extracted = extract_next_task("- [ ] first\n  detail\n- [x] done\n- [ ] second")
            .expect("pending task should be extracted");
        assert_eq!(extracted.0, "first\n  detail");
        assert_eq!(
            extracted.1,
            "- [x] first\n  detail\n- [x] done\n- [ ] second"
        );
    }

    /// Verifies the fallback behavior when notes do not contain explicit checkbox task headers.
    ///
    /// In that case the extractor should use the first non-empty line as the message and remove it from
    /// the stored text.
    #[test]
    fn extract_next_task_falls_back_to_first_non_empty_line_without_headers() {
        let extracted =
            extract_next_task("\nfirst line\nsecond line").expect("fallback task should be extracted");
        assert_eq!(extracted.0, "first line");
        assert_eq!(extracted.1, "\nsecond line");
    }

    /// Verifies the debounce behavior of the terminal-notes save system.
    ///
    /// Like session persistence, notes should not be written immediately after becoming dirty; they are
    /// expected to save only once the debounce window has elapsed.
    #[test]
    fn terminal_notes_save_waits_for_debounce_window() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let dir = temp_dir("neozeus-terminal-notes-save-debounce");
        let path = dir.join("notes.v1");
        let mut notes_state = TerminalNotesState {
            path: Some(path.clone()),
            dirty_since_secs: Some(0.0),
            ..Default::default()
        };
        assert!(append_task_from_text(
            &mut notes_state,
            "session-a",
            "first line"
        ));

        let mut world = World::default();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(100));
        world.insert_resource(time);
        world.insert_resource(notes_state);

        world.run_system_once(save_terminal_notes_if_dirty).unwrap();

        assert!(!path.exists(), "debounced save should not run yet");

        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(300));
        world.run_system_once(save_terminal_notes_if_dirty).unwrap();

        assert!(path.exists(), "save should run after debounce window");
        let saved = std::fs::read_to_string(&path).expect("failed to read notes file");
        let reparsed = parse_terminal_notes(&saved);
        assert!(reparsed.legacy_notes_by_session.is_empty());
    }

    #[test]
    fn remove_legacy_note_text_drops_session_keyed_entry() {
        let mut notes_state = TerminalNotesState::default();
        assert!(notes_state.set_note_text("session-a", "hello"));
        assert!(notes_state.remove_legacy_note_text("session-a"));
        assert_eq!(notes_state.note_text("session-a"), None);
        assert!(!notes_state.remove_legacy_note_text("session-a"));
    }
}
