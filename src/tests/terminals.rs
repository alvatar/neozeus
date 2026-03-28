use super::{fake_runtime_spawner, surface_with_text, FakeDaemonClient};
use crate::terminals::{
    TerminalCommand, TerminalDamage, TerminalFrameUpdate, TerminalRuntimeState, TerminalSurface,
    TerminalUpdate, TerminalViewState, PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
};
use bevy::prelude::*;
use std::{sync::Arc, time::Duration};

/// Verifies the mailbox coalescing rule that draining returns only the newest frame and newest status
/// plus the dropped-frame count.
#[test]
fn drain_terminal_updates_keeps_latest_frame_and_status() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mailbox = crate::terminals::TerminalUpdateMailbox::default();

    assert!(
        mailbox
            .push(TerminalUpdate::Frame(TerminalFrameUpdate {
                surface: surface_with_text(2, 2, 0, "a"),
                damage: TerminalDamage::Rows(vec![0]),
                runtime: TerminalRuntimeState::running("one"),
            }))
            .should_wake
    );
    assert!(
        !mailbox
            .push(TerminalUpdate::Frame(TerminalFrameUpdate {
                surface: surface_with_text(2, 2, 1, "b"),
                damage: TerminalDamage::Rows(vec![1]),
                runtime: TerminalRuntimeState::running("two"),
            }))
            .should_wake
    );
    assert!(
        !mailbox
            .push(TerminalUpdate::Status {
                runtime: TerminalRuntimeState::running("done"),
                surface: None,
            })
            .should_wake
    );

    let (frame, status, dropped) = mailbox.drain();
    assert_eq!(dropped, 1);
    assert_eq!(frame.unwrap().runtime.status, "two");
    assert_eq!(status.unwrap().0.status, "done");
}

/// Verifies that terminal view offsets are remembered per terminal and restored on focus changes.
#[test]
fn terminal_view_state_restores_offsets_per_terminal() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let id_one = crate::terminals::TerminalId(1);
    let id_two = crate::terminals::TerminalId(2);
    let mut view_state = TerminalViewState::default();

    view_state.apply_offset_delta(Some(id_one), Vec2::new(120.0, -30.0));
    assert_eq!(view_state.offset, Vec2::new(120.0, -30.0));

    view_state.focus_terminal(Some(id_two));
    assert_eq!(view_state.offset, Vec2::ZERO);

    view_state.apply_offset_delta(Some(id_two), Vec2::new(-48.0, 64.0));
    assert_eq!(view_state.offset, Vec2::new(-48.0, 64.0));

    view_state.focus_terminal(Some(id_one));
    assert_eq!(view_state.offset, Vec2::new(120.0, -30.0));

    view_state.focus_terminal(Some(id_two));
    assert_eq!(view_state.offset, Vec2::new(-48.0, 64.0));
}

/// Flattens a terminal surface into newline-separated text for daemon integration assertions.
fn surface_to_text(surface: &TerminalSurface) -> String {
    let mut text = String::new();
    for y in 0..surface.rows {
        if y > 0 {
            text.push('\n');
        }
        for x in 0..surface.cols {
            text.push_str(&surface.cell(x, y).content.to_owned_string());
        }
    }
    text
}

/// Verifies that persistent-session bootstrap sends exactly the plain `pi` bootstrap command, while
/// verifier sessions do not get the same bootstrap.
#[test]
fn runtime_spawner_bootstraps_persistent_sessions_with_plain_pi_only() {
    let client = Arc::new(FakeDaemonClient::default());
    let spawner = fake_runtime_spawner(client.clone());

    let persistent = spawner
        .create_session(PERSISTENT_SESSION_PREFIX)
        .expect("persistent session should be created");
    let _verifier = spawner
        .create_session(VERIFIER_SESSION_PREFIX)
        .expect("verifier session should be created");

    let commands = client.sent_commands.lock().unwrap().clone();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0, persistent);
    assert!(matches!(
        &commands[0].1,
        TerminalCommand::SendCommand(command) if command == "pi"
    ));
}

/// Verifies that the runtime spawner's daemon bridge exposes the initial snapshot as a status update
/// and forwards outgoing commands back to the daemon client.
#[test]
fn daemon_runtime_bridge_pushes_initial_snapshot_and_forwards_commands() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-1".into());
    let spawner = fake_runtime_spawner(client.clone());
    let bridge = spawner
        .spawn_attached("neozeus-session-1")
        .expect("daemon bridge should attach");

    let (frame, status, _) = bridge.drain_updates();
    assert!(frame.is_none());
    assert!(status.is_some());
    bridge.send(TerminalCommand::SendCommand("pwd".into()));
    std::thread::sleep(Duration::from_millis(20));
    let commands = client.sent_commands.lock().unwrap().clone();
    assert!(commands.iter().any(|(session_id, command)| {
        session_id == "neozeus-session-1"
            && matches!(command, TerminalCommand::SendCommand(value) if value == "pwd")
    }));
}

/// Verifies that streamed daemon updates propagate through the runtime bridge into the caller's
/// drained update stream.
#[test]
fn daemon_runtime_bridge_applies_streamed_updates() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-2".into());
    let spawner = fake_runtime_spawner(client.clone());
    let bridge = spawner
        .spawn_attached("neozeus-session-2")
        .expect("daemon bridge should attach");
    client.emit_update(
        "neozeus-session-2",
        TerminalUpdate::Status {
            runtime: TerminalRuntimeState::running("fake daemon streamed"),
            surface: Some(surface_with_text(1, 4, 0, "ok")),
        },
    );
    std::thread::sleep(Duration::from_millis(20));
    let (_, status, _) = bridge.drain_updates();
    let surface = status
        .expect("bridge should receive streamed update")
        .1
        .expect("streamed status should carry surface");
    assert!(surface_to_text(&surface).contains("ok"));
}
