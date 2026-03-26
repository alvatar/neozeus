use super::*;
use crate::tests::{insert_terminal_manager_resources, temp_dir, test_bridge};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::{Time, World},
};
use std::fs;
use std::time::Duration;

// Verifies that terminal sessions path prefers state home then home state then config.
#[test]
fn terminal_sessions_path_prefers_state_home_then_home_state_then_config() {
    assert_eq!(
        resolve_terminal_sessions_path_with(
            Some("/tmp/state"),
            Some("/tmp/home"),
            Some("/tmp/config")
        ),
        Some(std::path::PathBuf::from("/tmp/state/neozeus/terminals.v1"))
    );
    assert_eq!(
        resolve_terminal_sessions_path_with(None, Some("/tmp/home"), Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/home/.local/state/neozeus/terminals.v1"
        ))
    );
    assert_eq!(
        resolve_terminal_sessions_path_with(None, None, Some("/tmp/config")),
        Some(std::path::PathBuf::from("/tmp/config/neozeus/terminals.v1"))
    );
}

// Verifies that terminal sessions parse and serialize roundtrip.
#[test]
fn terminal_sessions_parse_and_serialize_roundtrip() {
    let persisted = PersistedTerminalSessions {
        sessions: vec![
            TerminalSessionRecord {
                session_name: "neozeus-session-a".into(),
                label: Some("agent 1".into()),
                creation_index: 0,
                last_focused: true,
            },
            TerminalSessionRecord {
                session_name: "neozeus-session-b".into(),
                label: None,
                creation_index: 1,
                last_focused: false,
            },
        ],
    };

    let serialized = serialize_persisted_terminal_sessions(&persisted);
    assert_eq!(parse_persisted_terminal_sessions(&serialized), persisted);
}

// Verifies that terminal sessions v2 quoted labels roundtrip.
#[test]
fn terminal_sessions_v2_quoted_labels_roundtrip() {
    let persisted = PersistedTerminalSessions {
        sessions: vec![TerminalSessionRecord {
            session_name: "neozeus-session-a".into(),
            label: Some("agent = \"alpha\"\\beta\nnext".into()),
            creation_index: 0,
            last_focused: true,
        }],
    };

    let serialized = serialize_persisted_terminal_sessions(&persisted);
    assert!(serialized.contains("version 2"));
    assert_eq!(parse_persisted_terminal_sessions(&serialized), persisted);
}

// Verifies that terminal sessions v1 parser remains backward compatible.
#[test]
fn terminal_sessions_v1_parser_remains_backward_compatible() {
    let persisted = parse_persisted_terminal_sessions(
        "version 1\nsession name=neozeus-session-a label=agent\\s1 creation_index=0 focused=1\n",
    );
    assert_eq!(persisted.sessions.len(), 1);
    assert_eq!(persisted.sessions[0].label.as_deref(), Some("agent 1"));
}

// Verifies that malformed terminal sessions version falls back to default.
#[test]
fn malformed_terminal_sessions_version_falls_back_to_default() {
    assert_eq!(
        parse_persisted_terminal_sessions(
            "version 99\nsession name=a creation_index=0 focused=1\n"
        ),
        PersistedTerminalSessions::default()
    );
}

// Verifies that reconcile terminal sessions restores prunes and imports.
#[test]
fn reconcile_terminal_sessions_restores_prunes_and_imports() {
    let persisted = PersistedTerminalSessions {
        sessions: vec![
            TerminalSessionRecord {
                session_name: "neozeus-session-a".into(),
                label: Some("one".into()),
                creation_index: 0,
                last_focused: true,
            },
            TerminalSessionRecord {
                session_name: "neozeus-session-b".into(),
                label: None,
                creation_index: 1,
                last_focused: false,
            },
        ],
    };

    let reconciled = reconcile_terminal_sessions(
        &persisted,
        &[
            "neozeus-session-a".into(),
            "neozeus-session-c".into(),
            "neozeus-verifier-x".into(),
        ],
    );

    assert_eq!(reconciled.restore.len(), 1);
    assert_eq!(reconciled.restore[0].session_name, "neozeus-session-a");
    assert_eq!(reconciled.prune.len(), 1);
    assert_eq!(reconciled.prune[0].session_name, "neozeus-session-b");
    assert_eq!(reconciled.import.len(), 1);
    assert_eq!(reconciled.import[0].session_name, "neozeus-session-c");
    assert_eq!(reconciled.import[0].creation_index, 2);
}

// Verifies that saving terminal sessions persists focus order and labels.
#[test]
fn saving_terminal_sessions_persists_focus_order_and_labels() {
    let dir = temp_dir("neozeus-terminal-sessions-save");
    let path = dir.join("terminals.v1");
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_two);

    let mut directory = AgentDirectory::default();
    directory.labels.insert(id_one, "oracle one".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(directory);
    world.insert_resource(TerminalSessionPersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world
        .run_system_once(save_terminal_sessions_if_dirty)
        .unwrap();
    let serialized = fs::read_to_string(&path).expect("terminal sessions file missing");
    let persisted = parse_persisted_terminal_sessions(&serialized);
    assert_eq!(persisted.sessions.len(), 2);
    assert_eq!(persisted.sessions[0].session_name, "neozeus-session-a");
    assert_eq!(persisted.sessions[0].label.as_deref(), Some("oracle one"));
    assert!(!persisted.sessions[0].last_focused);
    assert_eq!(persisted.sessions[1].session_name, "neozeus-session-b");
    assert!(persisted.sessions[1].last_focused);
}

// Verifies that terminal sessions save waits for debounce window.
#[test]
fn terminal_sessions_save_waits_for_debounce_window() {
    let dir = temp_dir("neozeus-terminal-sessions-save-debounce");
    let path = dir.join("terminals.v1");
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal_with_session(bridge, "neozeus-session-a".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(100));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(AgentDirectory::default());
    world.insert_resource(TerminalSessionPersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world
        .run_system_once(save_terminal_sessions_if_dirty)
        .unwrap();
    assert!(!path.exists(), "debounced save should not run yet");

    world
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_millis(300));
    world
        .run_system_once(save_terminal_sessions_if_dirty)
        .unwrap();
    assert!(path.exists(), "save should run after debounce window");
}
