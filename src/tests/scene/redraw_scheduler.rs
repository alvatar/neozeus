//! Test submodule: `redraw_scheduler` — extracted from the centralized test bucket.

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

/// Verifies that the combined redraw predicate stays false when no terminal or HUD visual work is
/// pending.
#[test]
fn redraw_scheduler_stays_idle_without_visual_work() {
    assert!(!should_request_visual_redraw(false, false, false, false));
}


/// Verifies that any one of the three visual-work sources is enough to request another redraw.
#[test]
fn redraw_scheduler_runs_when_visual_work_exists() {
    assert!(should_request_visual_redraw(true, false, false, false));
    assert!(should_request_visual_redraw(false, true, false, false));
    assert!(should_request_visual_redraw(false, false, true, false));
    assert!(should_request_visual_redraw(false, false, false, true));
}


/// Verifies that a selected agent row changing from idle to working requests a redraw even when the
/// terminal texture is already fully uploaded and no panel animation is active.
#[test]
fn working_agent_row_transition_requests_redraw_for_hud_feedback() {
    let (bridge, _) = test_bridge();
    let mut terminal_manager = crate::terminals::TerminalManager::default();
    let terminal_id = terminal_manager.create_terminal(bridge);
    terminal_manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some(surface_with_text(8, 120, 0, "header"));

    let mut agent_catalog = crate::agents::AgentCatalog::default();
    let agent_id = agent_catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );
    let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);

    let mut app = App::new();
    app.insert_resource(Time::<()>::default());
    app.insert_resource(agent_catalog);
    app.insert_resource(runtime_index);
    app.insert_resource(crate::agents::AgentStatusStore::default());
    app.insert_resource(crate::app::AppSessionState::default());
    app.insert_resource(crate::aegis::AegisPolicyStore::default());
    app.insert_resource(crate::aegis::AegisRuntimeStore::default());
    app.insert_resource(crate::conversations::AgentTaskStore::default());
    app.insert_resource(crate::conversations::ConversationStore::default());
    app.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    app.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    insert_terminal_manager_resources(app.world_mut(), terminal_manager);
    insert_test_hud_state(app.world_mut(), hud_state);
    app.insert_resource(TerminalPresentationStore::default());
    app.init_resource::<Messages<RequestRedraw>>();
    app.add_systems(
        Update,
        (
            crate::agents::sync_agent_status,
            crate::visual_contract::sync_visual_contract_state,
            crate::hud::sync_hud_view_models,
            request_redraw_while_visuals_active,
        )
            .chain(),
    );

    let panel_entity = app
        .world_mut()
        .spawn((
            TerminalPanel { id: terminal_id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::new(320.0, 180.0),
                target_size: Vec2::new(320.0, 180.0),
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.3,
                target_z: 0.3,
            },
        ))
        .id();
    app.world_mut()
        .resource_mut::<TerminalPresentationStore>()
        .register(
            terminal_id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: UVec2::new(1200, 160),
                    cell_size: UVec2::new(10, 20),
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: UVec2::new(1200, 160),
                    cell_size: UVec2::new(10, 20),
                },
                display_mode: crate::terminals::TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

    app.update();
    app.world_mut()
        .resource_mut::<Messages<RequestRedraw>>()
        .clear();

    {
        let world = app.world_mut();
        let mut time = world.resource_mut::<Time<()>>();
        time.advance_by(std::time::Duration::from_secs(1));
    }
    {
        let world = app.world_mut();
        let mut terminal_manager = world.resource_mut::<crate::terminals::TerminalManager>();
        let terminal = terminal_manager.get_mut(terminal_id).unwrap();
        terminal.snapshot.surface = Some({
            let mut surface = surface_with_text(8, 120, 0, "header");
            surface.set_text_cell(1, 3, "⠋ Working...");
            surface
        });
        terminal.surface_revision = 1;
    }
    app.world_mut()
        .resource_mut::<TerminalPresentationStore>()
        .get_mut(terminal_id)
        .unwrap()
        .uploaded_revision = 1;

    app.update();

    let world = app.world();
    let agent_list = world.resource::<crate::hud::AgentListView>();
    match &agent_list.rows[0].kind {
        crate::hud::AgentListRowKind::Agent { activity, .. } => {
            assert_eq!(*activity, crate::hud::AgentListActivity::Working)
        }
        other => panic!("expected agent row, got {other:?}"),
    }

    assert_eq!(
        world.resource::<Messages<RequestRedraw>>().len(),
        1,
        "working-state HUD transition should request redraw even without pending terminal upload"
    );
}


#[test]
fn stable_visual_contract_does_not_request_continuous_redraws() {
    let (bridge, _) = test_bridge();
    let mut terminal_manager = crate::terminals::TerminalManager::default();
    let terminal_id = terminal_manager.create_terminal(bridge);
    terminal_manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some({
        let mut surface = surface_with_text(8, 120, 0, "header");
        surface.set_text_cell(1, 3, "⠋ Working...");
        surface
    });

    let mut agent_catalog = crate::agents::AgentCatalog::default();
    let agent_id = agent_catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );
    let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_secs(1));
    app.insert_resource(time);
    app.insert_resource(agent_catalog);
    app.insert_resource(runtime_index);
    app.insert_resource(crate::agents::AgentStatusStore::default());
    app.insert_resource(crate::app::AppSessionState::default());
    app.insert_resource(crate::aegis::AegisPolicyStore::default());
    app.insert_resource(crate::aegis::AegisRuntimeStore::default());
    app.insert_resource(crate::conversations::AgentTaskStore::default());
    app.insert_resource(crate::conversations::ConversationStore::default());
    app.insert_resource(crate::hud::AgentListSelection::Agent(agent_id));
    app.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    insert_terminal_manager_resources(app.world_mut(), terminal_manager);
    insert_test_hud_state(app.world_mut(), hud_state);
    app.insert_resource(TerminalPresentationStore::default());
    app.init_resource::<Messages<RequestRedraw>>();
    app.add_systems(
        Update,
        (
            crate::agents::sync_agent_status,
            crate::visual_contract::sync_visual_contract_state,
            crate::hud::sync_hud_view_models,
            request_redraw_while_visuals_active,
        )
            .chain(),
    );

    app.update();
    app.world_mut()
        .resource_mut::<Messages<RequestRedraw>>()
        .clear();
    app.update();

    assert_eq!(
        app.world().resource::<Messages<RequestRedraw>>().len(),
        0,
        "stable contract signatures must not keep the redraw loop alive"
    );
}

