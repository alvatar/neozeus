use super::*;
use crate::{
    agents::AgentId,
    app::{AgentCommand as AppAgentCommand, AppCommand},
    hud::{
        AgentListRowView, AgentListUiState, AgentListView, ConversationListUiState,
        ConversationListView, DebugToolbarView, HudRect, HudState, HudWidgetKey,
    },
    terminals::{TerminalManager, TerminalPresentationStore, TerminalViewState},
    tests::{insert_test_hud_state, snapshot_test_hud_state, test_bridge},
};
use bevy::{
    ecs::system::RunSystemOnce,
    input::mouse::MouseWheel,
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};

/// Verifies the fixed geometry split between the main body, marker strip, and accent strip of an
/// agent-list row.
#[test]
fn agent_row_rect_splits_main_and_marker_geometry() {
    let row = HudRect {
        x: 40.0,
        y: 120.0,
        w: 220.0,
        h: 28.0,
    };
    let main = agent_row_rect(row, AgentListRowSection::Main);
    let marker = agent_row_rect(row, AgentListRowSection::Marker);
    let accent = agent_row_rect(row, AgentListRowSection::Accent);
    assert!(main.w > marker.w);
    assert!(main.x < marker.x);
    assert_eq!(main.y, row.y + 2.0);
    assert_eq!(marker.y, row.y + 2.0);
    assert_eq!(main.h, marker.h);
    assert_eq!(accent.x, row.x + 3.0);
    assert_eq!(accent.y, row.y + 3.0);
    assert_eq!(accent.w, 8.0);
    assert_eq!(accent.h, row.h - 6.0);
}

/// Verifies that reorder hit-testing chooses the first row whose midpoint is below the cursor.
#[test]
fn reorder_target_index_tracks_row_midpoints() {
    let state = AgentListUiState::default();
    let shell = HudRect {
        x: 24.0,
        y: 96.0,
        w: 300.0,
        h: 420.0,
    };
    let view = AgentListView {
        rows: vec![
            AgentListRowView {
                agent_id: AgentId(1),
                terminal_id: None,
                label: "alpha".into(),
                focused: false,
                has_tasks: false,
                interactive: true,
            },
            AgentListRowView {
                agent_id: AgentId(2),
                terminal_id: None,
                label: "beta".into(),
                focused: false,
                has_tasks: false,
                interactive: true,
            },
        ],
    };
    let rows = rows::agent_rows(shell, 0.0, None, &view);

    assert_eq!(
        reorder_target_index(
            &state,
            shell,
            Vec2::new(rows[0].rect.x + 4.0, rows[0].rect.y + 2.0),
            &view
        ),
        Some(0)
    );
    assert_eq!(
        reorder_target_index(
            &state,
            shell,
            Vec2::new(rows[1].rect.x + 4.0, rows[1].rect.y + rows[1].rect.h),
            &view
        ),
        Some(1)
    );
}

/// Verifies that the dragged row follows the cursor while the remaining rows reflow around the
/// live insertion slot.
#[test]
fn projected_agent_rows_follow_drag_cursor_and_reflow_other_rows() {
    let shell = HudRect {
        x: 24.0,
        y: 96.0,
        w: 300.0,
        h: 420.0,
    };
    let view = AgentListView {
        rows: vec![
            AgentListRowView {
                agent_id: AgentId(1),
                terminal_id: None,
                label: "alpha".into(),
                focused: false,
                has_tasks: false,
                interactive: true,
            },
            AgentListRowView {
                agent_id: AgentId(2),
                terminal_id: None,
                label: "beta".into(),
                focused: true,
                has_tasks: false,
                interactive: true,
            },
            AgentListRowView {
                agent_id: AgentId(3),
                terminal_id: None,
                label: "gamma".into(),
                focused: false,
                has_tasks: true,
                interactive: true,
            },
        ],
    };

    let rows = rows::projected_agent_rows(
        shell,
        0.0,
        None,
        &view,
        Some(rows::AgentListDragPreview {
            agent_id: AgentId(2),
            cursor_y: 260.0,
            grab_offset_y: 10.0,
            target_index: 2,
        }),
    );

    let alpha = rows
        .iter()
        .find(|row| row.agent_id == AgentId(1))
        .expect("alpha row should exist");
    let gamma = rows
        .iter()
        .find(|row| row.agent_id == AgentId(3))
        .expect("gamma row should exist");
    let beta = rows
        .iter()
        .find(|row| row.agent_id == AgentId(2))
        .expect("beta row should exist");

    assert!(!alpha.dragging);
    assert_eq!(alpha.rect.y, 158.0);
    assert_eq!(gamma.rect.y, 200.0);
    assert!(beta.dragging);
    assert_eq!(beta.rect.y, 250.0);
}

/// Verifies that explicit agent-directory labels override the synthetic `agent-N` fallback names.
#[test]
fn agent_rows_use_derived_agent_view_labels() {
    let rows = rows::agent_rows(
        HudRect {
            x: 24.0,
            y: 96.0,
            w: 300.0,
            h: 420.0,
        },
        0.0,
        None,
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(crate::terminals::TerminalId(1)),
                    label: "agent-1".into(),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                },
                AgentListRowView {
                    agent_id: crate::agents::AgentId(2),
                    terminal_id: Some(crate::terminals::TerminalId(2)),
                    label: "oracle".into(),
                    focused: true,
                    has_tasks: true,
                    interactive: true,
                },
            ],
        },
    );

    assert_eq!(rows[0].label, "agent-1");
    assert_eq!(rows[1].label, "oracle");
    assert_eq!(rows[1].display_label, "ORACLE");
    assert!(rows[1].focused);
    assert!(rows[1].has_tasks);
}

/// Verifies that agent-row generation follows terminal creation order and annotates the focused row.
#[test]
fn agent_rows_follow_terminal_order_and_focus() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);

    let shell_rect = HudRect {
        x: 24.0,
        y: 96.0,
        w: 300.0,
        h: 420.0,
    };
    let rows = rows::agent_rows(
        shell_rect,
        0.0,
        None,
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(id_one),
                    label: "agent-1".into(),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                },
                AgentListRowView {
                    agent_id: crate::agents::AgentId(2),
                    terminal_id: Some(id_two),
                    label: "agent-2".into(),
                    focused: true,
                    has_tasks: false,
                    interactive: true,
                },
            ],
        },
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].terminal_id, Some(id_one));
    assert_eq!(rows[0].label, "agent-1");
    assert!(rows[0].rect.y > shell_rect.y + 20.0);
    assert!(rows[0].rect.x > shell_rect.x + 20.0);
    assert_eq!(rows[1].terminal_id, Some(id_two));
    assert!(rows[1].focused);
    assert_eq!(rows[1].rect.y - rows[0].rect.y, 42.0);
}

/// Verifies that agent-row generation marks only the explicitly hovered agent as hovered.
#[test]
fn agent_rows_mark_hovered_agent() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

    let rows = rows::agent_rows(
        HudRect {
            x: 24.0,
            y: 96.0,
            w: 300.0,
            h: 420.0,
        },
        0.0,
        Some(crate::agents::AgentId(1)),
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(id_one),
                    label: "agent-1".into(),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                },
                AgentListRowView {
                    agent_id: crate::agents::AgentId(2),
                    terminal_id: Some(id_two),
                    label: "agent-2".into(),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                },
            ],
        },
    );
    assert!(
        rows.iter()
            .find(|row| row.terminal_id == Some(id_one))
            .expect("first row should exist")
            .hovered
    );
    assert!(
        !rows
            .iter()
            .find(|row| row.terminal_id == Some(id_two))
            .expect("second row should exist")
            .hovered
    );
}

/// Verifies that clicking the agent-list title region does not start drag state like ordinary HUD
/// modules do.
#[test]
fn agent_list_is_not_draggable() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    window.set_cursor_position(Some(Vec2::new(120.0, 16.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentListView::default());
    world.init_resource::<Messages<AppCommand>>();
    world.spawn((window, PrimaryWindow));
    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world
        .run_system_once(crate::hud::handle_hud_pointer_input)
        .unwrap();

    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.drag.is_none());
    assert!(!hud_state.dirty_layout);
}

/// Verifies that clicking an agent-list row emits the standard focus-plus-isolate command pair for
/// that terminal.
#[test]
fn clicking_agent_list_row_emits_focus_and_isolate_commands() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut emitted_commands = Vec::new();
    let agent_list_view = AgentListView {
        rows: vec![
            AgentListRowView {
                agent_id: crate::agents::AgentId(1),
                terminal_id: Some(crate::terminals::TerminalId(1)),
                label: "agent-1".into(),
                focused: false,
                has_tasks: false,
                interactive: true,
            },
            AgentListRowView {
                agent_id: crate::agents::AgentId(2),
                terminal_id: Some(id_two),
                label: "agent-2".into(),
                focused: false,
                has_tasks: false,
                interactive: true,
            },
        ],
    };
    let rows = rows::agent_rows(
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 392.0,
        },
        0.0,
        None,
        &agent_list_view,
    );
    let target_row = rows
        .iter()
        .find(|row| row.terminal_id == Some(id_two))
        .expect("agent row for second terminal missing");
    let click_point = Vec2::new(
        target_row.rect.x + target_row.rect.w * 0.5,
        target_row.rect.y + target_row.rect.h * 0.5,
    );

    crate::hud::handle_pointer_click(
        HudWidgetKey::AgentList,
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 392.0,
        },
        click_point,
        &AgentListUiState::default(),
        &ConversationListUiState::default(),
        &agent_list_view,
        &ConversationListView::default(),
        &DebugToolbarView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(
        emitted_commands,
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(crate::agents::AgentId(2))),
            AppCommand::Agent(AppAgentCommand::Inspect(crate::agents::AgentId(2))),
        ]
    );
}

/// Verifies that agent-list wheel scrolling clamps at the maximum content offset rather than running
/// past the last row.
#[test]
fn agent_list_scroll_clamps_to_content_height() {
    let mut agent_list_state = AgentListUiState::default();
    let mut conversation_list_state = ConversationListUiState::default();

    crate::hud::handle_scroll(
        HudWidgetKey::AgentList,
        -500.0,
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 112.0,
        },
        &mut agent_list_state,
        &mut conversation_list_state,
        &AgentListView {
            rows: (0..5)
                .map(|index| AgentListRowView {
                    agent_id: crate::agents::AgentId(index + 1),
                    terminal_id: Some(crate::terminals::TerminalId(index + 1)),
                    label: format!("agent-{}", index + 1),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                })
                .collect(),
        },
        &ConversationListView::default(),
    );

    assert_eq!(agent_list_state.scroll_offset, 84.0);
}
