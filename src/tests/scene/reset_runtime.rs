//! Test submodule: `reset_runtime` — extracted from the centralized test bucket.

#![allow(unused_imports)]

use crate::{
    app::{
        format_startup_panic, normalize_output_for_x11_fallback, primary_window_config_for,
        primary_window_plugin_config_for, resolve_disable_pipelined_rendering_for,
        resolve_force_fallback_adapter, resolve_force_fallback_adapter_for,
        resolve_linux_window_backend, resolve_output_dimension, resolve_output_mode,
        resolve_window_mode, resolve_window_scale_factor, should_force_x11_backend,
        uses_headless_runner, AppOutputConfig, LinuxWindowBackend, OutputMode,
    },
    hud::{HudState, HudWidgetKey, TerminalVisibilityPolicy},
    startup::{
        advance_startup_connecting, choose_startup_focus_session_name,
        request_redraw_while_visuals_active, should_request_visual_redraw,
        startup_visibility_policy_for_focus, DaemonConnectionState, StartupConnectPhase,
        StartupConnectState,
    },
    terminals::{
        TerminalId, TerminalPanel, TerminalPresentation, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalTextureState,
    },
    tests::{
        fake_daemon_resource, fake_runtime_spawner, insert_default_hud_resources,
        insert_terminal_manager_resources, insert_test_hud_state, surface_with_text, temp_dir,
        test_bridge, FakeDaemonClient,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::*,
    window::{RequestRedraw, WindowMode},
};
use std::sync::Arc;



use super::support::*;

#[test]
fn reset_runtime_kills_live_sessions_and_rebuilds_from_snapshot() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-live-a",
        crate::terminals::TerminalRuntimeState::running("live"),
    );
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-1".into(),
            owner_agent_uid: "agent-uid-live".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "tmux child".into(),
            cwd: "/tmp/demo".into(),
            attached: false,
            created_unix: 1,
        });
    let dir = temp_dir("neozeus-reset-rebuild");
    let app_state_path = dir.join("neozeus-state.v1");
    let conversations_path = dir.join("conversations.v1");
    let notes_path = dir.join("notes.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState {
        path: Some(conversations_path.clone()),
        dirty_since_secs: Some(1.0),
    });
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path.clone()),
        dirty_since_secs: None,
    });
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    notes_state.path = Some(notes_path.clone());
    notes_state.dirty_since_secs = Some(1.0);
    assert!(notes_state.set_note_text_by_agent_uid("agent-uid-live", "- [ ] stale task"));
    assert!(notes_state.set_note_text("neozeus-live-a", "legacy note"));
    world.insert_resource(notes_state);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    assert!(client.owned_tmux_sessions.lock().unwrap().is_empty());
    assert!(!client.sessions.lock().unwrap().contains("neozeus-live-a"));
    let notes_state = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(notes_state.path.as_ref(), Some(&notes_path));
    assert_eq!(notes_state.dirty_since_secs, None);
    assert!(notes_state
        .note_text_by_agent_uid("agent-uid-live")
        .is_none());
    assert!(notes_state.note_text("neozeus-live-a").is_none());
    let app_state_persistence = world.resource::<crate::app::AppStatePersistenceState>();
    assert_eq!(app_state_persistence.path.as_ref(), Some(&app_state_path));
    let conversation_persistence =
        world.resource::<crate::conversations::ConversationPersistenceState>();
    assert_eq!(
        conversation_persistence.path.as_ref(),
        Some(&conversations_path)
    );
    assert_eq!(conversation_persistence.dirty_since_secs, None);
    let commands = client.sent_commands.lock().unwrap().clone();
    assert!(commands.iter().any(|(_, command)| matches!(
        command,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "claude --resume claude-session-1"
    )));
    assert_eq!(
        world.resource::<crate::agents::AgentCatalog>().order.len(),
        1
    );
    let focused_terminal = world
        .resource::<crate::terminals::TerminalFocusState>()
        .active_id()
        .expect("reset restore should refocus the restored terminal");
    assert_eq!(
        world
            .resource::<crate::hud::TerminalVisibilityState>()
            .policy,
        TerminalVisibilityPolicy::Isolate(focused_terminal)
    );
    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Reset recovery completed: 1 restored, 0 failed")
    );
    assert!(status.details.iter().any(|line| line == "Reset confirmed"));
    assert!(status
        .details
        .iter()
        .any(|line| line == "Runtime clear started"));
    assert!(status
        .details
        .iter()
        .any(|line| line == "Runtime clear completed"));
    assert!(status
        .details
        .iter()
        .any(|line| line == "Automatic recovery started from saved snapshot"));
}


#[test]
fn reset_runtime_rehydrates_persisted_conversations_and_task_notes_after_restore() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-rehydrate-projections");
    let app_state_path = dir.join("neozeus-state.v1");
    let conversations_path = dir.join("conversations.v1");
    let notes_path = dir.join("notes.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");
    std::fs::write(
        &conversations_path,
        "version 2\n[conversation]\nagent_uid=\"agent-uid-1\"\n[message]\nauthor=\"user\"\ndelivery=\"delivered\"\nbody=\"hello after reset\"\n",
    )
    .expect("conversations should write");
    std::fs::write(
        &notes_path,
        "version 2\nnote agent_uid=agent-uid-1\n- [ ] restored task\n.\n",
    )
    .expect("notes should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState {
        path: Some(conversations_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    notes_state.path = Some(notes_path);
    world.insert_resource(notes_state);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let restored_agent = world.resource::<crate::agents::AgentCatalog>().order[0];
    assert_eq!(
        world
            .resource::<crate::conversations::AgentTaskStore>()
            .text(restored_agent),
        Some("- [ ] restored task")
    );
    let notes_state = world.resource::<crate::terminals::TerminalNotesState>();
    assert_eq!(
        notes_state.note_text_by_agent_uid("agent-uid-1"),
        Some("- [ ] restored task")
    );
    let conversations = world.resource::<crate::conversations::ConversationStore>();
    let conversation_id = conversations
        .conversation_for_agent(restored_agent)
        .expect("restored conversation should exist");
    assert_eq!(
        conversations.messages_for(conversation_id),
        vec![(
            "hello after reset".to_owned(),
            crate::conversations::MessageDeliveryState::Delivered,
        )]
    );
}


#[test]
fn reset_runtime_followed_by_conversation_mutation_still_persists_conversations() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-persists-conversations");
    let app_state_path = dir.join("neozeus-state.v1");
    let conversations_path = dir.join("conversations.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState {
        path: Some(conversations_path.clone()),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let agent_id = world.resource::<crate::agents::AgentCatalog>().order[0];
    {
        let mut conversations = world.resource_mut::<crate::conversations::ConversationStore>();
        let conversation_id = conversations.ensure_conversation(agent_id);
        let _ = conversations.push_message(
            conversation_id,
            crate::conversations::MessageAuthor::User,
            "hello after reset".into(),
            crate::conversations::MessageDeliveryState::Delivered,
        );
    }
    {
        let time = *world.resource::<Time>();
        let mut persistence =
            world.resource_mut::<crate::conversations::ConversationPersistenceState>();
        crate::conversations::mark_conversations_dirty(&mut persistence, Some(&time));
    }
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::conversations::save_conversations_if_dirty)
        .unwrap();

    let mut restored = crate::conversations::ConversationStore::default();
    crate::conversations::restore_persisted_conversations_from_path(
        &conversations_path,
        world.resource::<crate::agents::AgentCatalog>(),
        world.resource::<crate::agents::AgentRuntimeIndex>(),
        &mut restored,
    );
    let conversation_id = restored
        .conversation_for_agent(agent_id)
        .expect("restored conversation should exist");
    assert_eq!(
        restored.messages_for(conversation_id),
        vec![(
            "hello after reset".to_owned(),
            crate::conversations::MessageDeliveryState::Delivered,
        )]
    );
}


#[test]
fn reset_runtime_followed_by_task_mutation_still_persists_notes() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-persists-notes");
    let app_state_path = dir.join("neozeus-state.v1");
    let notes_path = dir.join("notes.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    let mut notes_state = crate::terminals::TerminalNotesState::default();
    notes_state.path = Some(notes_path.clone());
    world.insert_resource(notes_state);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let agent_id = world.resource::<crate::agents::AgentCatalog>().order[0];
    {
        let mut task_store = world.resource_mut::<crate::conversations::AgentTaskStore>();
        let _ = task_store.set_text(agent_id, "- [ ] persisted after reset");
    }
    world
        .run_system_once(crate::conversations::sync_task_notes_projection)
        .unwrap();
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::terminals::save_terminal_notes_if_dirty)
        .unwrap();

    let agent_uid = world
        .resource::<crate::agents::AgentCatalog>()
        .uid(agent_id)
        .expect("restored agent uid should exist")
        .to_owned();
    let restored = crate::terminals::load_terminal_notes_from(&notes_path);
    assert_eq!(
        restored.notes_by_agent_uid.get(&agent_uid),
        Some(&"- [ ] persisted after reset".to_owned())
    );
}


#[test]
fn reset_restore_still_supports_truthful_app_state_save() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-save-app-state");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path.clone()),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    {
        let time = *world.resource::<Time>();
        let mut persistence = world.resource_mut::<crate::app::AppStatePersistenceState>();
        crate::app::mark_app_state_dirty(&mut persistence, Some(&time));
    }
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    world
        .run_system_once(crate::app::save_app_state_if_dirty)
        .unwrap();

    let persisted = crate::app::load_persisted_app_state_from(&app_state_path);
    assert_eq!(persisted.agents.len(), 1);
    let restored = &persisted.agents[0];
    assert_eq!(restored.agent_uid.as_deref(), Some("agent-uid-1"));
    assert_eq!(restored.label.as_deref(), Some("ALPHA"));
    assert_eq!(
        restored.kind,
        crate::shared::app_state_file::PersistedAgentKind::Claude
    );
    assert!(restored.runtime_session_name.is_some());
    assert!(matches!(
        restored.recovery,
        Some(crate::shared::app_state_file::PersistedAgentRecoverySpec::Claude {
            ref session_id,
            ref cwd,
            ..
        }) if session_id == "claude-session-1" && cwd == "/tmp/demo"
    ));
    assert!(restored.last_focused);
}


#[test]
fn reset_runtime_is_idempotent_when_triggered_twice() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-idempotent");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    for _ in 0..2 {
        world
            .resource_mut::<Messages<crate::app::AppCommand>>()
            .write(crate::app::AppCommand::Recovery(
                crate::app::RecoveryCommand::ResetAll,
            ));
        world
            .run_system_once(crate::app::run_apply_app_commands)
            .unwrap();
    }

    assert_eq!(
        world.resource::<crate::agents::AgentCatalog>().order.len(),
        1
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalManager>()
            .terminal_ids()
            .len(),
        1
    );
    assert_eq!(client.sessions.lock().unwrap().len(), 1);
    assert_eq!(
        world
            .resource::<crate::app::AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset recovery completed: 1 restored, 0 failed")
    );
}


#[test]
fn reset_runtime_tolerates_partial_daemon_kill_failures_without_corrupting_local_state() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    *client.fail_owned_tmux_kill.lock().unwrap() = true;
    client.set_session_runtime(
        "neozeus-live-a",
        crate::terminals::TerminalRuntimeState::running("live"),
    );
    client
        .owned_tmux_sessions
        .lock()
        .unwrap()
        .push(crate::terminals::OwnedTmuxSessionInfo {
            session_uid: "tmux-1".into(),
            owner_agent_uid: "agent-uid-live".into(),
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "tmux child".into(),
            cwd: "/tmp/demo".into(),
            attached: false,
            created_unix: 1,
        });
    let dir = temp_dir("neozeus-reset-kill-failures");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    assert_eq!(
        world.resource::<crate::agents::AgentCatalog>().order.len(),
        1
    );
    assert_eq!(
        world
            .resource::<crate::terminals::TerminalManager>()
            .terminal_ids()
            .len(),
        1
    );
    assert_eq!(
        world
            .resource::<crate::app::AppSessionState>()
            .recovery_status
            .title
            .as_deref(),
        Some("Reset recovery completed: 1 restored, 0 failed")
    );
    assert!(client.sessions.lock().unwrap().contains("neozeus-live-a"));
    assert_eq!(client.owned_tmux_sessions.lock().unwrap().len(), 1);
}


#[test]
fn reset_runtime_reports_success_when_no_saved_snapshot_exists() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-no-snapshot");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(&app_state_path, "neozeus state version 4\n").expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Reset completed: runtime cleared; no saved snapshot to restore")
    );
    assert!(status
        .details
        .iter()
        .any(|line| line == "No saved snapshot to restore"));
}


#[test]
fn reset_runtime_reports_daemon_discovery_failure_without_corrupting_clear_state() {
    let dir = temp_dir("neozeus-reset-discovery-failure");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 3\n[agent]\nagent_uid=\"agent-uid-1\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(crate::terminals::TerminalRuntimeSpawner::pending_headless());
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Reset recovery completed: 0 restored, 1 failed")
    );
    assert!(status.details.iter().any(|line| {
        line == "daemon session discovery failed: terminal runtime still connecting"
    }));
    assert!(world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .is_empty());
    assert!(world
        .resource::<crate::terminals::TerminalManager>()
        .terminal_ids()
        .is_empty());
}


#[test]
fn reset_runtime_reports_missing_live_only_agents_as_skipped() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let dir = temp_dir("neozeus-reset-skipped-live-only");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 4\n[agent]\nagent_uid=\"agent-recoverable\"\nlabel=\"ALPHA\"\nkind=\"claude\"\nrecovery_mode=\"claude\"\nrecovery_session_id=\"claude-session-1\"\nrecovery_cwd=\"/tmp/demo\"\norder_index=0\nfocused=1\n[/agent]\n[agent]\nagent_uid=\"agent-live-only\"\nruntime_session_name=\"neozeus-session-missing\"\nlabel=\"BETA\"\nkind=\"terminal\"\norder_index=1\nfocused=0\n[/agent]\n",
    )
    .expect("app state should write");

    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(crate::terminals::TerminalManager::default());
    world.insert_resource(crate::terminals::TerminalFocusState::default());
    world.insert_resource(crate::terminals::TerminalPresentationStore::default());
    world.insert_resource(crate::agents::AgentCatalog::default());
    world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(crate::aegis::AegisPolicyStore::default());
    world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    world.insert_resource(crate::conversations::ConversationStore::default());
    world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    world.insert_resource(crate::conversations::AgentTaskStore::default());
    world.insert_resource(crate::conversations::MessageTransportAdapter);
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.insert_resource(Time::<()>::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(crate::app::AppStatePersistenceState {
        path: Some(app_state_path),
        dirty_since_secs: None,
    });
    world.insert_resource(crate::terminals::TerminalNotesState::default());
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(crate::startup::DaemonConnectionState::default());
    world.insert_resource(crate::startup::StartupConnectState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    crate::tests::insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::app::AppCommand>>();

    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Recovery(
            crate::app::RecoveryCommand::ResetAll,
        ));
    world
        .run_system_once(crate::app::run_apply_app_commands)
        .unwrap();

    let status = &world
        .resource::<crate::app::AppSessionState>()
        .recovery_status;
    assert_eq!(
        status.title.as_deref(),
        Some("Reset recovery completed: 1 restored, 0 failed, 1 skipped")
    );
    assert!(status.details.iter().any(|line| {
        line == "startup skipped live-only agent BETA: runtime session unavailable"
    }));
}

