use super::*;

#[test]
fn startup_restore_preserves_pi_clone_provenance_and_workdir_identity() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-clone-provenance");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\nclone_source_session_path=\"/tmp/pi-alpha.jsonl\"\nworkdir=1\norder_index=0\nfocused=1\n[/agent]\n",
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

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog.order.first().expect("restored agent should exist");
    assert_eq!(
        catalog.clone_source_session_path(restored_agent),
        Some("/tmp/pi-alpha.jsonl")
    );
    assert!(catalog.is_workdir(restored_agent));
    assert_eq!(
        catalog.kind(restored_agent),
        Some(crate::agents::AgentKind::Pi)
    );
}

#[test]
fn startup_restore_plain_clone_can_be_cloned_again() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let source_root = std::path::PathBuf::from("/tmp").join(format!(
        "neozeus-restored-clone-source-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&source_root).unwrap();
    let source_session = source_root.join("source.jsonl");
    std::fs::write(
        &source_session,
        format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"parent-id\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"{}\"}}\n",
            source_root.display()
        ),
    )
    .unwrap();

    let dir = temp_dir("neozeus-startup-clone-again");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        format!(
            "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\nclone_source_session_path=\"{}\"\nworkdir=0\norder_index=0\nfocused=1\n[/agent]\n",
            source_session.display()
        ),
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
    world.insert_resource(crate::hud::AgentListSelection::default());
    world.insert_resource(crate::hud::AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    world.init_resource::<Messages<RequestRedraw>>();
    world.init_resource::<Messages<crate::app::AppCommand>>();
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

    world.run_system_once(crate::startup::setup_scene).unwrap();
    insert_default_hud_resources(&mut world);
    let restored_agent = *world
        .resource::<crate::agents::AgentCatalog>()
        .order
        .first()
        .unwrap();
    world
        .resource_mut::<Messages<crate::app::AppCommand>>()
        .write(crate::app::AppCommand::Agent(
            crate::app::AgentCommand::Clone {
                source_agent_id: restored_agent,
                label: "beta".into(),
                workdir: false,
            },
        ));
    crate::app::run_apply_app_commands(&mut world);

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    assert_eq!(catalog.order.len(), 2);
    let clone_agent = *catalog.order.last().unwrap();
    assert_eq!(catalog.label(clone_agent), Some("BETA"));
    assert_eq!(
        catalog.kind(clone_agent),
        Some(crate::agents::AgentKind::Pi)
    );
    assert!(catalog.clone_source_session_path(clone_agent).is_some());
}

#[test]
fn startup_restore_workdir_clone_projects_marker_after_sync() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::running("restored"),
    );
    let dir = temp_dir("neozeus-startup-workdir-marker");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        "neozeus state version 1\n[agent]\nagent_uid=\"agent-uid-1\"\nsession_name=\"neozeus-session-a\"\nlabel=\"ALPHA\"\nkind=\"pi\"\nclone_source_session_path=\"/tmp/pi-alpha.jsonl\"\nworkdir=1\norder_index=0\nfocused=1\n[/agent]\n",
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
    world.insert_resource(crate::hud::HudInputCaptureState::default());
    world.insert_resource(crate::terminals::TerminalViewState::default());
    world.insert_resource(crate::hud::AgentListSelection::default());
    world.insert_resource(crate::hud::AgentListView::default());
    world.insert_resource(crate::hud::ConversationListView::default());
    world.insert_resource(crate::hud::ThreadView::default());
    world.insert_resource(crate::hud::ComposerView::default());
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
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

    world.run_system_once(crate::startup::setup_scene).unwrap();
    run_synced_hud_view_models(&mut world);

    let rows = &world.resource::<crate::hud::AgentListView>().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "⎇ ALPHA");
}

#[test]
fn startup_restore_keeps_explicit_pi_clone_provenance_separate_from_recovery_fallback() {
    let client = Arc::new(crate::tests::FakeDaemonClient::default());
    let source_root = temp_dir("neozeus-pi-clone-provenance-source");
    let recovery_root = temp_dir("neozeus-pi-clone-provenance-recovery");
    let source_session = source_root.join("source.jsonl");
    let recovery_session = recovery_root.join("recovery.jsonl");
    std::fs::write(
        &source_session,
        format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"source-id\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"{}\"}}\n",
            source_root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        &recovery_session,
        format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"recovery-id\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"{}\"}}\n",
            recovery_root.display()
        ),
    )
    .unwrap();
    let dir = temp_dir("neozeus-startup-pi-clone-vs-recovery");
    let app_state_path = dir.join("neozeus-state.v1");
    std::fs::write(
        &app_state_path,
        format!(
            "neozeus state version 4\n[agent]\nagent_uid=\"agent-uid-1\"\nruntime_session_name=\"neozeus-session-stale\"\nlabel=\"ALPHA\"\nkind=\"pi\"\nclone_source_session_path=\"{}\"\nrecovery_mode=\"pi\"\nrecovery_session_path=\"{}\"\nrecovery_cwd=\"{}\"\norder_index=0\nfocused=1\n[/agent]\n",
            source_session.display(),
            recovery_session.display(),
            recovery_root.display(),
        ),
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

    world.run_system_once(crate::startup::setup_scene).unwrap();

    let catalog = world.resource::<crate::agents::AgentCatalog>();
    let restored_agent = *catalog
        .order
        .first()
        .expect("restored Pi agent should exist");
    assert_eq!(
        catalog.clone_source_session_path(restored_agent),
        Some(source_session.to_str().unwrap())
    );
    assert!(matches!(
        catalog.recovery_spec(restored_agent),
        Some(crate::agents::AgentRecoverySpec::Pi { session_path, .. })
            if session_path == recovery_session.to_str().unwrap()
    ));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value.starts_with("pi --session ")
                && value.contains(&recovery_session.display().to_string())
    ));
}
