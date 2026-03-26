use super::*;
use crate::tests::temp_dir;
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::{Time, World},
};
use std::time::Duration;

#[test]
fn task_entry_from_text_matches_zeus_checkbox_format() {
    assert_eq!(
        task_entry_from_text("first line\n  detail line\nsecond detail"),
        Some("- [ ] first line\n  detail line\nsecond detail".to_owned())
    );
    assert_eq!(task_entry_from_text("  \n \t"), None);
}

#[test]
fn terminal_notes_path_prefers_state_home_then_home_state_then_config() {
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

#[test]
fn terminal_notes_parse_and_serialize_roundtrip() {
    let mut notes = std::collections::HashMap::new();
    notes.insert("session-a".to_owned(), "- [ ] first\n  detail".to_owned());
    notes.insert("session-b".to_owned(), ".starts with dot".to_owned());

    let serialized = serialize_terminal_notes(&notes);
    let reparsed = parse_terminal_notes(&serialized);

    assert_eq!(reparsed, notes);
}

#[test]
fn terminal_notes_append_and_prepend_tasks_follow_zeus_ordering() {
    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.append_task_from_text("session-a", "second task"));
    assert!(notes_state.prepend_task_from_text("session-a", "first task\n  detail"));
    assert_eq!(
        notes_state.note_text("session-a"),
        Some("- [ ] first task\n  detail\n- [ ] second task")
    );
    assert!(notes_state.has_note_text("session-a"));
}

#[test]
fn clear_done_tasks_removes_done_task_blocks() {
    let (updated, removed) =
        clear_done_tasks("- [x] done one\n  detail\n- [ ] keep\n- [X] done two\n  more\ntrailing");
    assert_eq!(removed, 2);
    assert_eq!(updated, "- [ ] keep");
}

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

#[test]
fn extract_next_task_falls_back_to_first_non_empty_line_without_headers() {
    let extracted =
        extract_next_task("\nfirst line\nsecond line").expect("fallback task should be extracted");
    assert_eq!(extracted.0, "first line");
    assert_eq!(extracted.1, "\nsecond line");
}

#[test]
fn terminal_notes_save_waits_for_debounce_window() {
    let dir = temp_dir("neozeus-terminal-notes-save-debounce");
    let path = dir.join("notes.v1");
    let mut notes_state = TerminalNotesState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
        ..Default::default()
    };
    assert!(notes_state.append_task_from_text("session-a", "first line"));

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
    assert_eq!(
        reparsed.get("session-a").map(String::as_str),
        Some("- [ ] first line")
    );
}
