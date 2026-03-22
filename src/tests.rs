mod hud;
mod input;
mod scene;
mod terminals;

use crate::terminals::{
    AttachedDaemonSession, CachedTerminalGlyph, DaemonSessionInfo, TerminalBridge, TerminalCommand,
    TerminalDaemonClient, TerminalDaemonClientResource, TerminalDebugStats, TerminalRuntimeSpawner,
    TerminalRuntimeState, TerminalSessionClient, TerminalSnapshot, TerminalSurface, TerminalUpdate,
    TerminalUpdateMailbox, TmuxPaneClient, TmuxPaneDescriptor, TmuxPaneState,
};
use bevy::{
    input::{
        keyboard::{Key, KeyboardInput},
        ButtonState,
    },
    prelude::*,
};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn pressed_text(key_code: KeyCode, text: Option<&str>) -> KeyboardInput {
    KeyboardInput {
        key_code,
        logical_key: Key::Character(text.unwrap_or("").into()),
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window: Entity::PLACEHOLDER,
    }
}

pub(super) fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

pub(super) fn capturing_bridge() -> (
    TerminalBridge,
    mpsc::Receiver<TerminalCommand>,
    Arc<TerminalUpdateMailbox>,
) {
    let (input_tx, input_rx) = mpsc::channel::<TerminalCommand>();
    let mailbox = Arc::new(TerminalUpdateMailbox::default());
    let bridge = TerminalBridge::new(
        input_tx,
        mailbox.clone(),
        Arc::new(Mutex::new(TerminalDebugStats::default())),
    );
    (bridge, input_rx, mailbox)
}

pub(super) fn test_bridge() -> (TerminalBridge, Arc<TerminalUpdateMailbox>) {
    let (bridge, _input_rx, mailbox) = capturing_bridge();
    (bridge, mailbox)
}

#[derive(Default)]
pub(super) struct FakeDaemonClient {
    pub(super) sessions: Mutex<BTreeSet<String>>,
    pub(super) sent_commands: Mutex<Vec<(String, TerminalCommand)>>,
    pub(super) resize_requests: Mutex<Vec<(String, usize, usize)>>,
    pub(super) fail_kill: Mutex<bool>,
    pub(super) next_session_index: Mutex<u64>,
    updates: Mutex<std::collections::HashMap<String, Vec<mpsc::Sender<TerminalUpdate>>>>,
}

impl FakeDaemonClient {
    pub(super) fn emit_update(&self, session_id: &str, update: TerminalUpdate) {
        let senders = self
            .updates
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        for sender in senders {
            let _ = sender.send(update.clone());
        }
    }
}

impl TerminalDaemonClient for FakeDaemonClient {
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .map(|session_id| DaemonSessionInfo {
                session_id,
                runtime: TerminalRuntimeState::running("fake daemon"),
                revision: 0,
            })
            .collect())
    }

    fn create_session(&self, prefix: &str) -> Result<String, String> {
        let mut next = self.next_session_index.lock().unwrap();
        let session_id = format!("{prefix}{}", *next);
        *next += 1;
        self.sessions.lock().unwrap().insert(session_id.clone());
        Ok(session_id)
    }

    fn attach_session(&self, session_id: &str) -> Result<AttachedDaemonSession, String> {
        if !self.sessions.lock().unwrap().contains(session_id) {
            return Err(format!("daemon session `{session_id}` not found"));
        }
        let (tx, rx) = mpsc::channel();
        self.updates
            .lock()
            .unwrap()
            .entry(session_id.to_owned())
            .or_default()
            .push(tx);
        Ok(AttachedDaemonSession {
            snapshot: TerminalSnapshot {
                surface: Some(TerminalSurface::new(120, 38)),
                runtime: TerminalRuntimeState::running("fake daemon"),
            },
            updates: rx,
        })
    }

    fn send_command(&self, session_id: &str, command: TerminalCommand) -> Result<(), String> {
        self.sent_commands
            .lock()
            .unwrap()
            .push((session_id.to_owned(), command));
        Ok(())
    }

    fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String> {
        self.resize_requests
            .lock()
            .unwrap()
            .push((session_id.to_owned(), cols, rows));
        Ok(())
    }

    fn kill_session(&self, session_id: &str) -> Result<(), String> {
        if *self.fail_kill.lock().unwrap() {
            return Err("kill failed".into());
        }
        self.sessions.lock().unwrap().remove(session_id);
        self.updates.lock().unwrap().remove(session_id);
        Ok(())
    }
}

pub(super) fn fake_daemon_resource(client: Arc<FakeDaemonClient>) -> TerminalDaemonClientResource {
    TerminalDaemonClientResource::from_client(client)
}

pub(super) fn fake_runtime_spawner(client: Arc<FakeDaemonClient>) -> TerminalRuntimeSpawner {
    TerminalRuntimeSpawner::for_tests(fake_daemon_resource(client))
}

#[derive(Default)]
pub(super) struct FakeTmuxClient {
    pub(super) sessions: Mutex<BTreeSet<String>>,
    pub(super) collision_checks_remaining: Mutex<usize>,
}

impl FakeTmuxClient {
    pub(super) fn with_collisions(count: usize) -> Self {
        Self {
            collision_checks_remaining: Mutex::new(count),
            ..Default::default()
        }
    }
}

impl TerminalSessionClient for FakeTmuxClient {
    fn ensure_tmux_available(&self) -> Result<(), String> {
        Ok(())
    }

    fn create_detached_session(&self, name: &str) -> Result<(), String> {
        self.sessions.lock().unwrap().insert(name.to_owned());
        Ok(())
    }

    fn list_sessions(&self) -> Result<Vec<String>, String> {
        Ok(self.sessions.lock().unwrap().iter().cloned().collect())
    }

    fn has_session(&self, name: &str) -> Result<bool, String> {
        let mut remaining = self.collision_checks_remaining.lock().unwrap();
        if *remaining > 0 {
            *remaining -= 1;
            return Ok(true);
        }
        Ok(self.sessions.lock().unwrap().contains(name))
    }

    fn kill_session(&self, name: &str) -> Result<(), String> {
        self.sessions.lock().unwrap().remove(name);
        Ok(())
    }
}

impl TmuxPaneClient for FakeTmuxClient {
    fn list_panes(&self, _session_name: &str) -> Result<Vec<TmuxPaneDescriptor>, String> {
        Ok(vec![TmuxPaneDescriptor {
            pane_id: "%1".into(),
            active: true,
        }])
    }

    fn pane_state(&self, _pane_target: &str) -> Result<TmuxPaneState, String> {
        Ok(TmuxPaneState {
            cols: 120,
            rows: 38,
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: true,
        })
    }

    fn capture_pane(&self, _pane_target: &str, _history_limit: usize) -> Result<String, String> {
        Ok(String::new())
    }

    fn send_bytes(&self, _pane_target: &str, _bytes: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

pub(super) fn surface_with_text(rows: usize, cols: usize, y: usize, text: &str) -> TerminalSurface {
    let mut surface = TerminalSurface::new(cols, rows);
    for (x, ch) in text.chars().enumerate() {
        surface.set_text_cell(x, y, &ch.to_string());
    }
    surface
}

pub(super) fn assert_glyph_has_visible_pixels(glyph: &CachedTerminalGlyph) {
    let non_zero_alpha = glyph
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 0)
        .count();
    assert!(
        non_zero_alpha > 0,
        "glyph rasterized to fully transparent image"
    );
}
