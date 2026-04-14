use super::*;
use crate::{
    agents::{AgentCatalog, AgentKind, AgentMetadata, AgentRecoverySpec, AgentRuntimeIndex},
    shared::app_state_file::{PersistedAgentKind, PersistedAgentRecoverySpec},
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

/// Verifies that the canonical recovery snapshot format round-trips its minimal recoverable-agent
/// payload losslessly.
#[test]
fn app_state_parse_and_serialize_roundtrip() {
    let persisted = PersistedAppState {
        agents: vec![PersistedAgentState {
            agent_uid: Some("agent-uid-a".into()),
            runtime_session_name: None,
            label: Some("agent 1\nrow\rand\ttabs\\slash".into()),
            kind: PersistedAgentKind::Claude,
            recovery: Some(PersistedAgentRecoverySpec::Claude {
                session_id: "claude-session-a".into(),
                cwd: "/tmp/demo".into(),
                model: Some("sonnet".into()),
                profile: None,
            }),
            clone_source_session_path: None,
            aegis_enabled: false,
            aegis_prompt_text: None,
            paused: false,
            order_index: 0,
            last_focused: false,
        }],
    };

    let serialized = serialize_persisted_app_state(&persisted);
    assert!(serialized.starts_with(crate::shared::app_state_file::APP_STATE_VERSION_V4));
    assert_eq!(parse_persisted_app_state(&serialized), persisted);
}

#[test]
fn canonical_snapshot_serializer_roundtrips_paused_agents() {
    let persisted = PersistedAppState {
        agents: vec![PersistedAgentState {
            agent_uid: Some("agent-uid-a".into()),
            runtime_session_name: Some("neozeus-session-a".into()),
            label: Some("ALPHA".into()),
            kind: PersistedAgentKind::Terminal,
            recovery: None,
            clone_source_session_path: None,
            aegis_enabled: false,
            aegis_prompt_text: None,
            paused: true,
            order_index: 0,
            last_focused: false,
        }],
    };

    let serialized = serialize_persisted_app_state(&persisted);
    assert!(serialized.contains("paused=1"));
    assert_eq!(parse_persisted_app_state(&serialized), persisted);
}

#[test]
fn canonical_snapshot_serializer_includes_runtime_focus_clone_and_aegis_fields() {
    let persisted = PersistedAppState {
        agents: vec![PersistedAgentState {
            agent_uid: Some("agent-uid-a".into()),
            runtime_session_name: Some("neozeus-session-a".into()),
            label: Some("ALPHA".into()),
            kind: PersistedAgentKind::Pi,
            recovery: Some(PersistedAgentRecoverySpec::Pi {
                session_path: "/tmp/pi-alpha.jsonl".into(),
                cwd: Some("/tmp/demo".into()),
                is_workdir: true,
                workdir_slug: Some("alpha-wt".into()),
            }),
            clone_source_session_path: Some("/tmp/pi-alpha.jsonl".into()),
            aegis_enabled: true,
            aegis_prompt_text: Some("continue cleanly".into()),
            paused: false,
            order_index: 0,
            last_focused: true,
        }],
    };

    let serialized = serialize_persisted_app_state(&persisted);
    assert!(serialized.contains("runtime_session_name=\"neozeus-session-a\""));
    assert!(serialized.contains("clone_source_session_path=\"/tmp/pi-alpha.jsonl\""));
    assert!(serialized.contains("aegis_enabled=1"));
    assert!(serialized.contains("aegis_prompt_text=\"continue cleanly\""));
    assert!(serialized.contains("focused=1"));
    assert!(serialized.contains("recovery_mode=\"pi\""));
}

/// Verifies that older app-state files without explicit kind metadata default to `pi`.
#[test]
fn app_state_parse_defaults_missing_kind_to_pi() {
    let parsed = parse_persisted_app_state(
        "neozeus state version 1\n[agent]\nsession_name=\"neozeus-session-a\"\norder_index=0\nfocused=1\n[/agent]\n",
    );

    assert_eq!(parsed.agents.len(), 1);
    assert_eq!(parsed.agents[0].kind, PersistedAgentKind::Pi);
    assert_eq!(parsed.agents[0].agent_uid, None);
    assert_eq!(parsed.agents[0].clone_source_session_path, None);
    assert_eq!(parsed.agents[0].recovery, None);
    assert!(!parsed.agents[0].aegis_enabled);
    assert_eq!(parsed.agents[0].aegis_prompt_text, None);
}

#[test]
fn parse_persisted_app_state_flushes_complete_final_agent_block_at_eof() {
    let parsed = parse_persisted_app_state(
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"terminal\"\norder_index=0\nfocused=1\n",
    );

    assert_eq!(parsed.agents.len(), 1);
    assert_eq!(parsed.agents[0].agent_uid.as_deref(), Some("agent-uid-1"));
    assert_eq!(
        parsed.agents[0].runtime_session_name.as_deref(),
        Some("neozeus-session-a")
    );
    assert_eq!(parsed.agents[0].label.as_deref(), Some("ALPHA"));
    assert_eq!(parsed.agents[0].kind, PersistedAgentKind::Terminal);
    assert!(parsed.agents[0].last_focused);
}

#[test]
fn parse_persisted_app_state_flushes_complete_open_block_before_next_agent_header() {
    let parsed = parse_persisted_app_state(
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-a\"\norder_index=0\n[agent]\nagent_uid=\"agent-uid-2\"\nruntime_session_name=\"neozeus-session-b\"\norder_index=1\n[/agent]\n",
    );

    assert_eq!(parsed.agents.len(), 2);
    assert_eq!(parsed.agents[0].agent_uid.as_deref(), Some("agent-uid-1"));
    assert_eq!(parsed.agents[1].agent_uid.as_deref(), Some("agent-uid-2"));
    assert_eq!(parsed.agents[0].order_index, 0);
    assert_eq!(parsed.agents[1].order_index, 1);
}

#[test]
fn parse_persisted_app_state_drops_incomplete_truncated_agent_blocks() {
    let parsed = parse_persisted_app_state(
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-a\"\norder_index=0\n[/agent]\n[agent]\nagent_uid=\"agent-uid-2\"\nruntime_session_name=\"neozeus-session-b\"\n",
    );

    assert_eq!(parsed.agents.len(), 1);
    assert_eq!(parsed.agents[0].agent_uid.as_deref(), Some("agent-uid-1"));
}

#[test]
fn parse_persisted_app_state_ignores_unknown_sections_and_fields() {
    let parsed = parse_persisted_app_state(
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-a\"\nunknown_field=\"ignored\"\norder_index=0\n[/agent]\n[garbage]\nthis is not key value\n[/garbage]\n[agent]\nagent_uid=\"agent-uid-2\"\nruntime_session_name=\"neozeus-session-b\"\norder_index=1\n[/agent]\n",
    );

    assert_eq!(parsed.agents.len(), 2);
    assert_eq!(parsed.agents[0].agent_uid.as_deref(), Some("agent-uid-1"));
    assert_eq!(parsed.agents[1].agent_uid.as_deref(), Some("agent-uid-2"));
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
    assert_eq!(persisted.agents[0].agent_uid, None);
    assert_eq!(
        persisted.agents[0].runtime_session_name.as_deref(),
        Some("neozeus-session-a")
    );
    assert_eq!(persisted.agents[0].label.as_deref(), Some("agent 1"));
    assert_eq!(persisted.agents[0].kind, PersistedAgentKind::Pi);
    assert_eq!(persisted.agents[0].clone_source_session_path, None);
    assert_eq!(persisted.agents[0].recovery, None);
    assert!(!persisted.agents[0].aegis_enabled);
    assert_eq!(persisted.agents[0].aegis_prompt_text, None);
    assert_eq!(persisted.agents[0].order_index, 0);
    assert!(persisted.agents[0].last_focused);
}

/// Verifies the reconciliation split between restored, pruned, and newly imported agent sessions.
#[test]
fn reconcile_persisted_agents_restores_prunes_and_imports() {
    let persisted = PersistedAppState {
        agents: vec![
            PersistedAgentState {
                agent_uid: Some("agent-uid-a".into()),
                runtime_session_name: Some("neozeus-session-a".into()),
                label: Some("one".into()),
                kind: PersistedAgentKind::Pi,
                recovery: Some(PersistedAgentRecoverySpec::Pi {
                    session_path: "/tmp/pi-session-a.jsonl".into(),
                    cwd: Some("/tmp/demo".into()),
                    is_workdir: true,
                    workdir_slug: None,
                }),
                clone_source_session_path: Some("/tmp/pi-session-a.jsonl".into()),
                aegis_enabled: true,
                aegis_prompt_text: Some("prompt a".into()),
                paused: false,
                order_index: 0,
                last_focused: true,
            },
            PersistedAgentState {
                agent_uid: Some("agent-uid-b".into()),
                runtime_session_name: Some("neozeus-session-b".into()),
                label: None,
                kind: PersistedAgentKind::Terminal,
                recovery: None,
                clone_source_session_path: None,
                aegis_enabled: false,
                aegis_prompt_text: None,
                paused: false,
                order_index: 1,
                last_focused: false,
            },
        ],
    };

    let live_sessions = vec![
        crate::terminals::DaemonSessionInfo {
            session_id: "neozeus-session-a".into(),
            runtime: crate::terminals::TerminalRuntimeState::default(),
            revision: 0,
            created_order: 0,
            metadata: crate::shared::daemon_wire::DaemonSessionMetadata {
                agent_uid: Some("agent-uid-a".into()),
                agent_label: None,
                agent_kind: None,
            },
        },
        crate::terminals::DaemonSessionInfo {
            session_id: "neozeus-session-c".into(),
            runtime: crate::terminals::TerminalRuntimeState::default(),
            revision: 0,
            created_order: 1,
            metadata: crate::shared::daemon_wire::DaemonSessionMetadata::default(),
        },
        crate::terminals::DaemonSessionInfo {
            session_id: "neozeus-verifier-x".into(),
            runtime: crate::terminals::TerminalRuntimeState::default(),
            revision: 0,
            created_order: 2,
            metadata: crate::shared::daemon_wire::DaemonSessionMetadata::default(),
        },
    ];

    let (restore, prune, import) = reconcile_persisted_agents(&persisted, &live_sessions);

    assert_eq!(restore.len(), 1);
    assert_eq!(
        restore[0].runtime_session_name.as_deref(),
        Some("neozeus-session-a")
    );
    assert_eq!(restore[0].agent_uid.as_deref(), Some("agent-uid-a"));
    assert_eq!(
        restore[0].clone_source_session_path.as_deref(),
        Some("/tmp/pi-session-a.jsonl")
    );
    assert!(matches!(
        restore[0].recovery,
        Some(PersistedAgentRecoverySpec::Pi {
            is_workdir: true,
            ..
        })
    ));
    assert!(restore[0].aegis_enabled);
    assert_eq!(restore[0].aegis_prompt_text.as_deref(), Some("prompt a"));
    assert_eq!(prune.len(), 1);
    assert_eq!(
        prune[0].runtime_session_name.as_deref(),
        Some("neozeus-session-b")
    );
    assert_eq!(import, vec!["neozeus-session-c".to_owned()]);
}

#[test]
fn reconcile_persisted_agents_prefers_agent_uid_over_stale_runtime_session_name() {
    let persisted = PersistedAppState {
        agents: vec![PersistedAgentState {
            agent_uid: Some("agent-uid-a".into()),
            runtime_session_name: Some("neozeus-session-stale".into()),
            label: Some("alpha".into()),
            kind: PersistedAgentKind::Pi,
            recovery: None,
            clone_source_session_path: None,
            aegis_enabled: true,
            aegis_prompt_text: Some("keep going".into()),
            paused: false,
            order_index: 0,
            last_focused: true,
        }],
    };
    let live_sessions = vec![crate::terminals::DaemonSessionInfo {
        session_id: "neozeus-session-live".into(),
        runtime: crate::terminals::TerminalRuntimeState::default(),
        revision: 0,
        created_order: 0,
        metadata: crate::shared::daemon_wire::DaemonSessionMetadata {
            agent_uid: Some("agent-uid-a".into()),
            agent_label: Some("ALPHA".into()),
            agent_kind: None,
        },
    }];

    let (restore, prune, import) = reconcile_persisted_agents(&persisted, &live_sessions);

    assert_eq!(restore.len(), 1);
    assert_eq!(
        restore[0].runtime_session_name.as_deref(),
        Some("neozeus-session-live")
    );
    assert!(prune.is_empty());
    assert!(import.is_empty());
}

/// Verifies that the canonical snapshot persists both recoverable and live-only agents truthfully.
#[test]
fn saving_app_state_persists_runtime_focus_clone_and_aegis_truth() {
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
    let alpha = agent_catalog.create_agent_with_metadata(
        Some("alpha".into()),
        AgentKind::Claude,
        AgentKind::Claude.capabilities(),
        AgentMetadata {
            clone_source_session_path: None,
            recovery: Some(AgentRecoverySpec::Claude {
                session_id: "claude-session-alpha".into(),
                cwd: "/tmp/alpha".into(),
                model: Some("sonnet".into()),
                profile: None,
            }),
        },
    );
    let beta = agent_catalog.create_agent(
        Some("beta".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let alpha_uid = agent_catalog.uid(alpha).unwrap().to_owned();
    let beta_uid = agent_catalog.uid(beta).unwrap().to_owned();
    let mut aegis_policy = crate::aegis::AegisPolicyStore::default();
    assert!(aegis_policy.enable(&alpha_uid, "keep pushing cleanly".into()));
    assert!(aegis_policy.restore_policy(&beta_uid, false, "hold position".into()));
    runtime_index.link_terminal(alpha, id_one, "neozeus-session-a".into(), None);
    runtime_index.link_terminal(beta, id_two, "neozeus-session-b".into(), None);
    agent_catalog.move_to_index(beta, 0);
    let mut app_session = crate::app::AppSessionState::default();
    app_session
        .focus_intent
        .focus_agent(beta, crate::app::VisibilityMode::FocusedOnly);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(app_session);
    world.insert_resource(agent_catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(aegis_policy);
    world.insert_resource(AppStatePersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world.run_system_once(save_app_state_if_dirty).unwrap();
    let serialized = fs::read_to_string(&path).expect("app state file missing");
    let persisted = parse_persisted_app_state(&serialized);
    assert_eq!(persisted.agents.len(), 2);

    let beta_record = persisted
        .agents
        .iter()
        .find(|record| record.agent_uid.as_deref() == Some(beta_uid.as_str()))
        .expect("beta should persist");
    assert_eq!(
        beta_record.runtime_session_name.as_deref(),
        Some("neozeus-session-b")
    );
    assert_eq!(beta_record.label.as_deref(), Some("BETA"));
    assert_eq!(beta_record.kind, PersistedAgentKind::Terminal);
    assert_eq!(beta_record.recovery, None);
    assert!(!beta_record.aegis_enabled);
    assert_eq!(
        beta_record.aegis_prompt_text.as_deref(),
        Some("hold position")
    );
    assert!(beta_record.last_focused);

    let alpha_record = persisted
        .agents
        .iter()
        .find(|record| record.agent_uid.as_deref() == Some(alpha_uid.as_str()))
        .expect("alpha should persist");
    assert_eq!(
        alpha_record.runtime_session_name.as_deref(),
        Some("neozeus-session-a")
    );
    assert_eq!(alpha_record.label.as_deref(), Some("ALPHA"));
    assert_eq!(alpha_record.kind, PersistedAgentKind::Claude);
    assert!(!alpha_record.last_focused);
    assert!(alpha_record.aegis_enabled);
    assert_eq!(
        alpha_record.aegis_prompt_text.as_deref(),
        Some("keep pushing cleanly")
    );
    assert!(matches!(
        alpha_record.recovery,
        Some(PersistedAgentRecoverySpec::Claude { .. })
    ));
}

#[test]
fn saving_app_state_does_not_mark_any_agent_focused_when_owned_tmux_has_focus() {
    let dir = temp_dir("neozeus-app-state-save-owned-tmux-focus");
    let path = dir.join("neozeus-state.v1");
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = crate::terminals::TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_two);

    let mut agent_catalog = AgentCatalog::default();
    let alpha = agent_catalog.create_agent(
        Some("alpha".into()),
        AgentKind::Claude,
        AgentKind::Claude.capabilities(),
    );
    let beta = agent_catalog.create_agent(
        Some("beta".into()),
        AgentKind::Terminal,
        AgentKind::Terminal.capabilities(),
    );
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(alpha, id_one, "neozeus-session-a".into(), None);
    runtime_index.link_terminal(beta, id_two, "neozeus-session-b".into(), None);
    let mut app_session = crate::app::AppSessionState::default();
    app_session.focus_intent.focus_owned_tmux("tmux-1".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(app_session);
    world.insert_resource(agent_catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(AppStatePersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world.run_system_once(save_app_state_if_dirty).unwrap();
    let persisted =
        parse_persisted_app_state(&fs::read_to_string(&path).expect("app state file missing"));
    assert_eq!(persisted.agents.len(), 2);
    assert!(persisted.agents.iter().all(|record| !record.last_focused));
}

#[test]
fn saving_app_state_persists_disabled_aegis_prompt() {
    let dir = temp_dir("neozeus-app-state-save-disabled-aegis");
    let path = dir.join("neozeus-state.v1");
    let (bridge, _) = test_bridge();
    let mut manager = crate::terminals::TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(terminal_id);

    let mut agent_catalog = AgentCatalog::default();
    let agent_id = agent_catalog.create_agent(
        Some("alpha".into()),
        AgentKind::Pi,
        AgentKind::Pi.capabilities(),
    );
    let agent_uid = agent_catalog.uid(agent_id).unwrap().to_owned();
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "neozeus-session-a".into(), None);
    let mut aegis_policy = crate::aegis::AegisPolicyStore::default();
    assert!(aegis_policy.restore_policy(&agent_uid, false, "keep pushing cleanly".into()));

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(agent_catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(aegis_policy);
    world.insert_resource(AppStatePersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: Some(0.0),
    });

    world.run_system_once(save_app_state_if_dirty).unwrap();
    let serialized = fs::read_to_string(&path).expect("app state file missing");
    let persisted = parse_persisted_app_state(&serialized);
    assert_eq!(persisted.agents.len(), 1);
    assert!(!persisted.agents[0].aegis_enabled);
    assert_eq!(
        persisted.agents[0].aegis_prompt_text.as_deref(),
        Some("keep pushing cleanly")
    );
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
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(AgentCatalog::default());
    world.insert_resource(AgentRuntimeIndex::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
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
