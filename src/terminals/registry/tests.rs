use super::super::{
    bridge::TerminalBridge,
    debug::TerminalDebugStats,
    mailbox::TerminalUpdateMailbox,
    types::{
        TerminalCell, TerminalCellContent, TerminalFrameUpdate, TerminalLifecycle,
        TerminalRuntimeState, TerminalSurface, TerminalUpdate,
    },
};
use super::*;
use bevy::{ecs::system::RunSystemOnce, prelude::World};
use std::sync::{mpsc, Arc, Mutex};

/// Creates a test terminal bridge together with a mailbox tests can use for synthetic updates.
fn test_bridge() -> (TerminalBridge, Arc<TerminalUpdateMailbox>) {
    let (input_tx, _input_rx) = mpsc::channel::<super::super::types::TerminalCommand>();
    let mailbox = Arc::new(TerminalUpdateMailbox::default());
    let bridge = TerminalBridge::new(
        input_tx,
        mailbox.clone(),
        Arc::new(Mutex::new(TerminalDebugStats::default())),
    );
    (bridge, mailbox)
}

/// Builds a simple surface containing one short text run on the requested row.
fn surface_with_text(rows: usize, cols: usize, y: usize, text: &str) -> TerminalSurface {
    let mut surface = TerminalSurface::new(cols, rows);
    for (x, ch) in text.chars().enumerate() {
        if x >= cols || y >= rows {
            break;
        }
        surface.set_cell(
            x,
            y,
            TerminalCell {
                content: TerminalCellContent::Single(ch),
                fg: bevy_egui::egui::Color32::from_rgb(220, 220, 220),
                bg: crate::app_config::DEFAULT_BG,
                width: 1,
            },
        );
    }
    surface
}

/// Verifies that when both a frame and a later status update are drained, polling leaves the newer
/// status runtime in the retained terminal snapshot.
#[test]
fn poll_terminal_snapshots_keeps_latest_status_over_latest_frame_runtime() {
    let (bridge, mailbox) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    mailbox.push(TerminalUpdate::Frame(TerminalFrameUpdate {
        surface: surface_with_text(2, 2, 0, "a"),
        damage: TerminalDamage::Rows(vec![0]),
        runtime: TerminalRuntimeState::running("running"),
    }));
    mailbox.push(TerminalUpdate::Status {
        runtime: TerminalRuntimeState::failed("boom"),
        surface: None,
    });

    let mut world = World::default();
    world.insert_resource(manager);
    world.run_system_once(poll_terminal_snapshots).unwrap();
    let manager = world.resource::<TerminalManager>();
    let terminal = manager.get(terminal_id).unwrap();
    assert_eq!(terminal.snapshot.runtime.status, "boom");
    assert!(matches!(
        terminal.snapshot.runtime.lifecycle,
        TerminalLifecycle::Failed
    ));
}

/// Verifies that terminal creation order remains stable even when focus changes.
#[test]
fn terminal_creation_order_stays_stable_when_focus_changes() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_one);

    assert_eq!(manager.terminal_ids(), &[id_one, id_two]);
    assert_eq!(manager.clone_focus_state().focus_order(), &[id_two, id_one]);
}

/// Verifies that a terminal can be created without becoming active.
#[test]
fn terminal_can_be_created_without_becoming_active() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);
    let focus = manager.clone_focus_state();

    assert_eq!(manager.terminal_ids(), &[id]);
    assert_eq!(focus.active_id(), None);
    assert_eq!(focus.focus_order(), &[]);
}

/// Verifies that explicit session names are retained in manager state.
#[test]
fn terminal_with_session_name_is_retained_in_manager_state() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-42".into());

    let terminal = manager.get(terminal_id).expect("terminal should exist");
    assert_eq!(terminal.session_name, "neozeus-session-42");
}

/// Verifies that removing a terminal clears active focus and both ordering lists consistently.
#[test]
fn remove_terminal_clears_orders_and_active_state() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_one);

    let removed = manager
        .remove_terminal(id_one)
        .expect("terminal should exist");

    let focus = manager.clone_focus_state();
    assert_eq!(removed.session_name, "neozeus-session-a");
    assert_eq!(focus.active_id(), None);
    assert_eq!(manager.terminal_ids(), &[id_two]);
    assert_eq!(focus.focus_order(), &[id_two]);
}
