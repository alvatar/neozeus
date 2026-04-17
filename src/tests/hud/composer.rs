//! Test submodule: `composer` — extracted from the centralized test bucket.

#![allow(unused_imports)]

use super::super::{
    ensure_shared_app_command_test_resources, fake_runtime_spawner, init_git_repo,
    insert_default_hud_resources, insert_terminal_manager_resources, insert_test_hud_state,
    pressed_text, snapshot_test_hud_state, temp_dir, test_bridge, write_pi_session_file,
    FakeDaemonClient,
};
use crate::agents::{AgentCatalog, AgentRuntimeIndex};
use crate::terminals::{
    kill_active_terminal_session_and_remove as kill_active_terminal, TerminalFontState,
    TerminalGlyphCache, TerminalManager, TerminalNotesState, TerminalPanel, TerminalPanelFrame,
    TerminalPresentationStore, TerminalTextRenderer, TerminalViewState,
};
use crate::{
    app::{
        AgentCommand as AppAgentCommand, AppCommand, AppSessionState, AppStatePersistenceState,
        ComposerCommand as AppComposerCommand, CreateAgentDialogField,
        CreateAgentKind as AppCreateAgentKind, TaskCommand as AppTaskCommand, WidgetCommand,
    },
    app_config::DEFAULT_BG,
    composer::{
        clone_agent_name_field_rect, clone_agent_submit_button_rect, clone_agent_workdir_rect,
        create_agent_name_field_rect, message_box_action_buttons, message_box_rect,
        message_box_shortcut_button_rects, task_dialog_action_buttons,
    },
    hud::{
        handle_hud_module_shortcuts, handle_hud_pointer_input, AgentListDragState,
        AgentListUiState, AgentListView, HudDragState, HudRect, HudState, HudWidgetKey,
        TerminalVisibilityPolicy, TerminalVisibilityState,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    image::Image,
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::MouseWheel,
        ButtonState,
    },
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};


use super::support::*;

#[test]
fn composer_submit_marks_message_failed_when_daemon_send_fails() {
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_send.lock().unwrap() = true;
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(AppStatePersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let mut catalog = AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Terminal,
        crate::agents::AgentKind::Terminal.capabilities(),
    );
    world.insert_resource(catalog);
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "neozeus-session-a".into(), None);
    world.insert_resource(runtime_index);

    {
        let mut app_session = world.resource_mut::<AppSessionState>();
        app_session.composer.open_message(agent_id);
        app_session.composer.message_editor.load_text("status");
    }

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Composer(AppComposerCommand::Submit));
    run_app_commands(&mut world);

    let conversations = world.resource::<crate::conversations::ConversationStore>();
    let conversation_id = conversations
        .conversation_for_agent(agent_id)
        .expect("conversation should exist");
    let messages = conversations.messages_for(conversation_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, "status");
    assert_eq!(
        messages[0].1,
        crate::conversations::MessageDeliveryState::Failed("send failed".into())
    );
}

