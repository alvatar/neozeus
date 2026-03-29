use super::*;
use crate::{
    agents::{AgentCapabilities, AgentCatalog, AgentKind, AgentRuntimeIndex},
    tests::{insert_terminal_manager_resources, temp_dir, test_bridge},
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::{Time, World},
};
use std::{fs, time::Duration};

/// Verifies the search-order logic for the app-state persistence file.
#[test]
fn app_state_path_prefers_state_home_then_home_state_then_config() {
    assert_eq!(
        resolve_app_state_path_with(Some("/tmp/state"), Some("/tmp/home"), Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/state/neozeus/neozeus-state.v1"
        ))
    );
    assert_eq!(
        resolve_app_state_path_with(None, Some("/tmp/home"), Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/home/.local/state/neozeus/neozeus-state.v1"
        ))
    );
    assert_eq!(
        resolve_app_state_path_with(None, None, Some("/tmp/config")),
        Some(std::path::PathBuf::from(
            "/tmp/config/neozeus/neozeus-state.v1"
        ))
    );
}

/// Verifies that the current app-state persistence format round-trips losslessly.
#[test]
fn app_state_parse_and_serialize_roundtrip() {
    let persisted = PersistedAppState {
        agents: vec![
            PersistedAgentState {
                session_name: "neozeus-session-a".into(),
                label: Some("agent 1".into()),
                order_index: 0,
                last_focused: true,
            },
            PersistedAgentState {
                session_name: "neozeus-session-b".into(),
                label: None,
                order_index: 1,
                last_focused: false,
            },
        ],
    };

    let serialized = serialize_persisted_app_state(&persisted);
    assert_eq!(parse_persisted_app_state(&serialized), persisted);
}

/// Verifies that legacy terminal-session state migrates into the new app-state model on read.
#[test]
fn app_state_load_falls_back_to_legacy_terminal_sessions() {
    let dir = temp_dir("neozeus-app-state-fallback");
    let legacy_path = dir.join("terminals.v1");
    fs::write(
        &legacy_path,
        "version 1\nsession name=neozeus-session-a label=agent\\s1 creation_index=0 focused=1\n",
    )
    .unwrap();

    let persisted =
        map_legacy_sessions_to_app_state(&load_persisted_terminal_sessions_from(&legacy_path));
    assert_eq!(persisted.agents.len(), 1);
    assert_eq!(persisted.agents[0].session_name, "neozeus-session-a");
    assert_eq!(persisted.agents[0].label.as_deref(), Some("agent 1"));
    assert_eq!(persisted.agents[0].order_index, 0);
    assert!(persisted.agents[0].last_focused);
}

/// Verifies the reconciliation split between restored, pruned, and newly imported agent sessions.
#[test]
fn reconcile_persisted_agents_restores_prunes_and_imports() {
    let persisted = PersistedAppState {
        agents: vec![
            PersistedAgentState {
                session_name: "neozeus-session-a".into(),
                label: Some("one".into()),
                order_index: 0,
                last_focused: true,
            },
            PersistedAgentState {
                session_name: "neozeus-session-b".into(),
                label: None,
                order_index: 1,
                last_focused: false,
            },
        ],
    };

    let (restore, import, prune) = reconcile_persisted_agents(
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
    assert_eq!(import[0].order_index, 2);
}

/// Verifies that saving app state preserves user agent order, labels, and the focused session.
#[test]
fn saving_app_state_persists_agent_order_labels_and_focus() {
    let dir = temp_dir("neozeus-app-state-save");
    let path = dir.join("neozeus-state.v1");
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = crate::terminals::TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_two);

    let mut agent_catalog = AgentCatalog::default();
    let mut runtime_index = AgentRuntimeIndex::default();
    let alpha = agent_catalog
        .create_agent(
            Some("alpha".into()),
            AgentKind::Terminal,
            AgentCapabilities::terminal_defaults(),
        )
        .unwrap();
    let beta = agent_catalog
        .create_agent(
            Some("beta".into()),
            AgentKind::Terminal,
            AgentCapabilities::terminal_defaults(),
        )
        .unwrap();
    runtime_index.link_terminal(alpha, id_one, "neozeus-session-a".into(), None);
    runtime_index.link_terminal(beta, id_two, "neozeus-session-b".into(), None);
    agent_catalog.move_to_index(beta, 0);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(agent_catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(AppStatePersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world.run_system_once(save_app_state_if_dirty).unwrap();
    let serialized = fs::read_to_string(&path).expect("app state file missing");
    let persisted = parse_persisted_app_state(&serialized);
    assert_eq!(persisted.agents.len(), 2);
    assert_eq!(persisted.agents[0].session_name, "neozeus-session-b");
    assert_eq!(persisted.agents[0].label.as_deref(), Some("beta"));
    assert!(persisted.agents[0].last_focused);
    assert_eq!(persisted.agents[1].session_name, "neozeus-session-a");
    assert_eq!(persisted.agents[1].label.as_deref(), Some("alpha"));
    assert!(!persisted.agents[1].last_focused);
}

/// Verifies the debounce behavior of the app-state save system.
#[test]
fn app_state_save_waits_for_debounce_window() {
    let dir = temp_dir("neozeus-app-state-save-debounce");
    let path = dir.join("neozeus-state.v1");

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(100));
    world.insert_resource(time);
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(AppStatePersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world.run_system_once(save_app_state_if_dirty).unwrap();
    assert!(!path.exists());

    world
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_millis(300));
    world.run_system_once(save_app_state_if_dirty).unwrap();
    assert!(path.exists());
}
