use super::*;
use crate::{
    agents::{AgentCapabilities, AgentCatalog, AgentKind, AgentRuntimeIndex},
    tests::{insert_terminal_manager_resources, temp_dir, test_bridge},
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::{Time, World},
};
use std::fs;
use std::time::Duration;

/// Verifies the search-order logic for the terminal-session persistence file.
///
/// The path resolver should prefer XDG state home first, then `~/.local/state`, and only then fall
/// back to the config directory path.
#[test]
fn terminal_sessions_path_prefers_state_home_then_home_state_then_config() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
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

/// Verifies that the current terminal-session persistence format round-trips losslessly.
///
/// The test serializes a representative structure containing labels, creation order, and focus state,
/// then reparses it and expects the original structure back unchanged.
#[test]
fn terminal_sessions_parse_and_serialize_roundtrip() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
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

/// Verifies that version-2 persistence handles quoted labels with escapes correctly.
///
/// The test uses a label containing quotes, backslashes, and a newline so both escaping and unescaping
/// paths are exercised.
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

/// Verifies backward compatibility with the legacy version-1 terminal-session format.
///
/// Old persisted files should still parse, including the historical escaped-space encoding for
/// labels.
#[test]
fn terminal_sessions_v1_parser_remains_backward_compatible() {
    let persisted = parse_persisted_terminal_sessions(
        "version 1\nsession name=neozeus-session-a label=agent\\s1 creation_index=0 focused=1\n",
    );
    assert_eq!(persisted.sessions.len(), 1);
    assert_eq!(persisted.sessions[0].label.as_deref(), Some("agent 1"));
}

/// Verifies that unsupported persistence versions are rejected by falling back to an empty default.
///
/// This protects the app from partially misparsing unknown future formats as if they were current
/// data.
#[test]
fn malformed_terminal_sessions_version_falls_back_to_default() {
    assert_eq!(
        parse_persisted_terminal_sessions(
            "version 99\nsession name=a creation_index=0 focused=1\n"
        ),
        PersistedTerminalSessions::default()
    );
}

/// Verifies the reconciliation split between restored, pruned, and newly imported sessions.
///
/// The fixture mixes one persisted-live match, one stale persisted session, one fresh live session,
/// and one verifier session that should be ignored. The resulting buckets and imported creation index
/// are then asserted explicitly.
#[test]
fn reconcile_terminal_sessions_restores_prunes_and_imports() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
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

    let (restore, import, prune) = reconcile_terminal_sessions(
        &persisted,
        &[
            "neozeus-session-a".into(),
            "neozeus-session-c".into(),
            "neozeus-verifier-x".into(),
        ],
    );

    assert_eq!(restore.len(), 1);
    assert_eq!(restore[0].session_name, "neozeus-session-a");
    assert_eq!(prune.len(), 1);
    assert_eq!(prune[0].session_name, "neozeus-session-b");
    assert_eq!(import.len(), 1);
    assert_eq!(import[0].session_name, "neozeus-session-c");
    assert_eq!(import[0].creation_index, 2);
}

/// Verifies that saving terminal sessions preserves creation order, labels, and the focused session.
///
/// The test builds a small manager, focuses the second session, adds one agent label, runs the save
/// system, and then reparses the written file to ensure those semantics survived serialization.
#[test]
fn saving_terminal_sessions_persists_focus_order_and_labels() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let dir = temp_dir("neozeus-terminal-sessions-save");
    let path = dir.join("terminals.v1");
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_two);

    let mut agent_catalog = AgentCatalog::default();
    let mut runtime_index = AgentRuntimeIndex::default();
    let agent_id = agent_catalog.create_agent(
        Some("oracle one".into()),
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );
    runtime_index.link_terminal(agent_id, id_one, "neozeus-session-a".into(), None);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(agent_catalog);
    world.insert_resource(runtime_index);
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

/// Verifies the debounce behavior of the session-persistence save system.
///
/// A dirty resource should not be written immediately; the first run before the debounce window ends
/// must do nothing, while a later run after enough simulated time has elapsed must create the file.
#[test]
fn terminal_sessions_save_waits_for_debounce_window() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
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
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
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
