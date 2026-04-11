use super::*;

#[test]
fn clone_agent_dialog_pointer_click_updates_focus_toggles_workdir_and_emits_command() {
    let mut world = World::default();
    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let name_rect = clone_agent_name_field_rect(&window);
    let workdir_rect = clone_agent_workdir_rect(&window);
    let clone_rect = clone_agent_submit_button_rect(&window);

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    init_hud_commands(&mut world);
    insert_default_hud_resources(&mut world);
    world.spawn((window.clone(), PrimaryWindow));

    {
        let mut session = world.resource_mut::<AppSessionState>();
        session.clone_agent_dialog.open(
            crate::agents::AgentId(7),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
        session.clone_agent_dialog.error = Some("stale error".into());
    }

    window.set_cursor_position(Some(Vec2::new(name_rect.x + 4.0, name_rect.y + 4.0)));
    *world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .single_mut(&mut world)
        .expect("primary window should exist") = window.clone();
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();
    assert_eq!(
        world.resource::<AppSessionState>().clone_agent_dialog.focus,
        crate::app::CloneAgentDialogField::Name
    );
    assert_eq!(
        world.resource::<AppSessionState>().clone_agent_dialog.error,
        None
    );

    window.set_cursor_position(Some(Vec2::new(workdir_rect.x + 4.0, workdir_rect.y + 4.0)));
    *world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .single_mut(&mut world)
        .expect("primary window should exist") = window.clone();
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();
    assert!(
        world
            .resource::<AppSessionState>()
            .clone_agent_dialog
            .workdir
    );

    world
        .resource_mut::<AppSessionState>()
        .clone_agent_dialog
        .name_field
        .load_text("child");
    window.set_cursor_position(Some(Vec2::new(clone_rect.x + 4.0, clone_rect.y + 4.0)));
    *world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .single_mut(&mut world)
        .expect("primary window should exist") = window;
    world.insert_resource(ButtonInput::<MouseButton>::default());
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: crate::agents::AgentId(7),
            label: "CHILD".into(),
            workdir: true,
        })]
    );
}

#[test]
fn clone_claude_agent_request_forks_and_persists_child_recovery_spec() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Claude,
            crate::agents::AgentKind::Claude.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: None,
                recovery: Some(crate::agents::AgentRecoverySpec::Claude {
                    session_id: "claude-parent".into(),
                    cwd: "/tmp/claude-demo".into(),
                    model: Some("sonnet".into()),
                    profile: None,
                }),
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/claude-demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value.starts_with("claude --resume claude-parent --fork-session --session-id ")
    ));
    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().expect("Claude child should exist");
    assert!(matches!(
        catalog.recovery_spec(clone_agent),
        Some(crate::agents::AgentRecoverySpec::Claude { session_id, cwd, model, .. })
            if session_id != "claude-parent"
                && cwd == "/tmp/claude-demo"
                && model.as_deref() == Some("sonnet")
    ));
}

#[test]
fn clone_codex_agent_request_forks_and_captures_child_recovery_spec() {
    if !sqlite3_available() {
        return;
    }
    let _home_lock = home_env_test_lock().lock().unwrap();
    let previous_home = std::env::var_os("HOME");
    let codex_home = temp_dir("neozeus-codex-clone-test-home");
    std::env::set_var("HOME", &codex_home);
    write_codex_state_db(
        &codex_home.join(".codex").join("state_5.sqlite"),
        &[("thread-old", "/tmp/other", 10, "old")],
    );
    std::thread::spawn({
        let codex_home = codex_home.clone();
        move || {
            std::thread::sleep(Duration::from_millis(150));
            write_codex_state_db(
                &codex_home.join(".codex").join("state_6.sqlite"),
                &[
                    ("thread-old", "/tmp/other", 10, "old"),
                    ("thread-child", "/tmp/codex-demo", 20, "child"),
                ],
            );
        }
    });

    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Codex,
            crate::agents::AgentKind::Codex.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: None,
                recovery: Some(crate::agents::AgentRecoverySpec::Codex {
                    session_id: "codex-parent".into(),
                    cwd: "/tmp/codex-demo".into(),
                    model: None,
                    profile: None,
                }),
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some("/tmp/codex-demo"));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value == "codex fork codex-parent -C /tmp/codex-demo"
    ));
    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().expect("Codex child should exist");
    assert!(matches!(
        catalog.recovery_spec(clone_agent),
        Some(crate::agents::AgentRecoverySpec::Codex { session_id, cwd, .. })
            if session_id == "thread-child" && cwd == "/tmp/codex-demo"
    ));
    if let Some(previous_home) = previous_home {
        std::env::set_var("HOME", previous_home);
    } else {
        std::env::remove_var("HOME");
    }
}

#[test]
fn clone_agent_request_rejects_non_pi_source() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_agent = world.resource_mut::<AgentCatalog>().create_agent(
        Some("source".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert_eq!(world.resource::<AgentCatalog>().order.len(), 1);
    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 0);
}

#[test]
fn clone_agent_request_rejects_missing_clone_provenance() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_agent = world.resource_mut::<AgentCatalog>().create_agent(
        Some("source".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert_eq!(world.resource::<AgentCatalog>().order.len(), 1);
}

#[test]
fn clone_agent_request_rejects_duplicate_name() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_dir = temp_dir("clone-pi-duplicate-source");
    let source_session = source_dir.join("source.jsonl");
    write_pi_session_file(&source_session, "/tmp/clone-pi-duplicate-cwd");
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );
    world.resource_mut::<AgentCatalog>().create_agent(
        Some("child".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert_eq!(world.resource::<AgentCatalog>().order.len(), 2);
}

#[test]
fn clone_agent_request_creates_top_level_pi_clone_and_focuses_it() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_dir = temp_dir("clone-pi-source");
    let source_session = source_dir.join("source.jsonl");
    let source_cwd = "/tmp/clone-pi-cwd";
    write_pi_session_file(&source_session, source_cwd);
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child".into(),
            workdir: false,
        }));

    run_app_commands(&mut world);

    let catalog = world.resource::<AgentCatalog>();
    assert_eq!(catalog.order.len(), 2);
    let clone_agent = *catalog.order.last().expect("clone agent should exist");
    assert_eq!(catalog.label(clone_agent), Some("CHILD"));
    assert_eq!(
        catalog.kind(clone_agent),
        Some(crate::agents::AgentKind::Pi)
    );
    let clone_session_path = catalog
        .clone_source_session_path(clone_agent)
        .expect("clone should persist forked Pi session path")
        .to_owned();
    assert!(!catalog.is_workdir(clone_agent));

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), Some(source_cwd));
    let sent_commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(sent_commands.len(), 1);
    assert!(matches!(
        &sent_commands[0].1,
        crate::terminals::TerminalCommand::SendCommand(value)
            if value.contains(&clone_session_path)
    ));
    let clone_header = crate::shared::pi_session_files::read_session_header(&clone_session_path)
        .expect("forked Pi session should read");
    assert_eq!(clone_header.cwd, source_cwd);
    assert_eq!(
        clone_header.parent_session.as_deref(),
        Some(source_session.to_string_lossy().as_ref())
    );
    let _ = catalog;

    assert_eq!(
        *world.resource::<crate::hud::AgentListSelection>(),
        crate::hud::AgentListSelection::Agent(clone_agent)
    );
    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
}

#[test]
fn clone_agent_request_creates_workdir_clone_and_persists_metadata() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let repo = init_git_repo("neozeus-clone-worktree");
    let source_session = repo.join("source.jsonl");
    write_pi_session_file(&source_session, repo.to_str().unwrap());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );
    let app_state_path = temp_dir("clone-pi-workdir-appstate").join("neozeus-state.v1");
    world.insert_resource(AppStatePersistenceState {
        path: Some(app_state_path.clone()),
        dirty_since_secs: None,
    });

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child-wt".into(),
            workdir: true,
        }));

    run_app_commands(&mut world);

    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().expect("workdir clone should exist");
    assert_eq!(catalog.label(clone_agent), Some("CHILD-WT"));
    assert!(catalog.is_workdir(clone_agent));
    assert_eq!(catalog.workdir_slug(clone_agent), Some("CHILD-WT"));
    let clone_session_path = catalog
        .clone_source_session_path(clone_agent)
        .expect("workdir clone should persist forked Pi session path")
        .to_owned();
    let clone_header = crate::shared::pi_session_files::read_session_header(&clone_session_path)
        .expect("workdir clone session should read");
    let expected_worktree = repo.join(".worktrees").join("CHILD-WT");
    assert_eq!(PathBuf::from(&clone_header.cwd), expected_worktree);
    assert!(expected_worktree.is_dir());
    let _ = catalog;

    let created_sessions = client.created_sessions.lock().unwrap().clone();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(created_sessions[0].1.as_deref(), expected_worktree.to_str());

    world
        .resource_mut::<Time<()>>()
        .advance_by(Duration::from_secs(1));
    world
        .run_system_once(crate::app::save_app_state_if_dirty)
        .unwrap();
    let persisted = crate::shared::app_state_file::parse_persisted_app_state(
        &fs::read_to_string(app_state_path).expect("app state should persist"),
    );
    let persisted_clone = persisted
        .agents
        .iter()
        .find(|record| record.label.as_deref() == Some("CHILD-WT"))
        .expect("persisted workdir clone should exist");
    assert_eq!(
        persisted_clone.clone_source_session_path.as_deref(),
        Some(clone_session_path.as_str())
    );
    assert!(matches!(
        &persisted_clone.recovery,
        Some(crate::shared::app_state_file::PersistedAgentRecoverySpec::Pi {
            session_path,
            is_workdir: true,
            workdir_slug: Some(slug),
            ..
        }) if session_path == &clone_session_path && slug == "CHILD-WT"
    ));
}

#[test]
fn clone_agent_request_sanitizes_workdir_slug_without_changing_display_label() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let repo = init_git_repo("neozeus-clone-worktree");
    let source_session = repo.join("source.jsonl");
    write_pi_session_file(&source_session, repo.to_str().unwrap());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child wt/1".into(),
            workdir: true,
        }));

    run_app_commands(&mut world);

    let catalog = world.resource::<AgentCatalog>();
    let clone_agent = *catalog.order.last().expect("workdir clone should exist");
    assert_eq!(catalog.label(clone_agent), Some("CHILD WT/1"));
    assert_eq!(catalog.workdir_slug(clone_agent), Some("CHILD-WT-1"));
    let clone_session_path = catalog
        .clone_source_session_path(clone_agent)
        .expect("workdir clone should persist session path")
        .to_owned();
    let clone_header = crate::shared::pi_session_files::read_session_header(&clone_session_path)
        .expect("workdir clone session should read");
    assert_eq!(
        PathBuf::from(&clone_header.cwd),
        repo.join(".worktrees").join("CHILD-WT-1")
    );
}

#[test]
fn clone_agent_request_rejects_non_git_workdir_source() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = clone_test_world(client.clone());
    let source_dir = temp_dir("clone-pi-non-git-source");
    let source_session = source_dir.join("source.jsonl");
    let non_git_cwd = PathBuf::from("/tmp").join(format!(
        "neozeus-clone-non-git-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&non_git_cwd).expect("non-git cwd should create");
    write_pi_session_file(&source_session, non_git_cwd.to_str().unwrap());
    let source_agent = world
        .resource_mut::<AgentCatalog>()
        .create_agent_with_metadata(
            Some("source".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
            crate::agents::AgentMetadata {
                clone_source_session_path: Some(source_session.to_string_lossy().into_owned()),
                recovery: None,
            },
        );

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Clone {
            source_agent_id: source_agent,
            label: "child-wt".into(),
            workdir: true,
        }));

    run_app_commands(&mut world);

    assert!(client.created_sessions.lock().unwrap().is_empty());
    assert_eq!(world.resource::<AgentCatalog>().order.len(), 1);
}
