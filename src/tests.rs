mod hud;
mod input;
mod scene;
mod terminals;

use crate::{
    hud::{HudInputCaptureState, HudLayoutState, HudModalState},
    terminals::{
        AttachedDaemonSession, CachedTerminalGlyph, DaemonSessionInfo, TerminalBridge,
        TerminalCommand, TerminalDaemonClient, TerminalDaemonClientResource, TerminalDebugStats,
        TerminalRuntimeSpawner, TerminalRuntimeState, TerminalSnapshot, TerminalSurface,
        TerminalUpdate, TerminalUpdateMailbox,
    },
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

pub(super) fn insert_default_hud_resources(world: &mut World) {
    world.insert_resource(HudLayoutState::default());
    world.insert_resource(HudModalState::default());
    world.insert_resource(HudInputCaptureState::default());
    if !world.contains_resource::<crate::terminals::TerminalFocusState>() {
        world.insert_resource(crate::terminals::TerminalFocusState::default());
    }
}

pub(super) fn insert_terminal_manager_resources(
    world: &mut World,
    terminal_manager: crate::terminals::TerminalManager,
) {
    #[cfg(test)]
    {
        world.insert_resource(terminal_manager.clone_focus_state());
    }
    world.insert_resource(terminal_manager);
}

pub(super) fn insert_terminal_manager_resources_into_app(
    app: &mut App,
    terminal_manager: crate::terminals::TerminalManager,
) {
    insert_terminal_manager_resources(app.world_mut(), terminal_manager);
}

pub(super) fn insert_hud_resources(
    world: &mut World,
    layout_state: HudLayoutState,
    modal_state: HudModalState,
    input_capture: HudInputCaptureState,
) {
    world.insert_resource(layout_state);
    world.insert_resource(modal_state);
    world.insert_resource(input_capture);
}

#[cfg(test)]
pub(super) fn insert_test_hud_state(world: &mut World, hud_state: crate::hud::HudState) {
    let (layout_state, modal_state, input_capture) = hud_state.into_resources();
    insert_hud_resources(world, layout_state, modal_state, input_capture);
    if !world.contains_resource::<crate::terminals::TerminalFocusState>() {
        world.insert_resource(crate::terminals::TerminalFocusState::default());
    }
}

#[cfg(test)]
pub(super) fn snapshot_test_hud_state(world: &World) -> crate::hud::HudState {
    crate::hud::HudState::from_resources(
        world.resource::<HudLayoutState>(),
        world.resource::<HudModalState>(),
        world.resource::<HudInputCaptureState>(),
    )
}

#[cfg(test)]
pub(super) fn insert_test_hud_state_into_app(app: &mut App, hud_state: crate::hud::HudState) {
    insert_test_hud_state(app.world_mut(), hud_state);
}

#[derive(Default)]
pub(super) struct FakeDaemonClient {
    pub(super) sessions: Mutex<BTreeSet<String>>,
    pub(super) session_runtimes: Mutex<std::collections::HashMap<String, TerminalRuntimeState>>,
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

    pub(super) fn set_session_runtime(&self, session_id: &str, runtime: TerminalRuntimeState) {
        self.sessions.lock().unwrap().insert(session_id.to_owned());
        self.session_runtimes
            .lock()
            .unwrap()
            .insert(session_id.to_owned(), runtime);
    }

    fn session_runtime(&self, session_id: &str) -> TerminalRuntimeState {
        self.session_runtimes
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| TerminalRuntimeState::running("fake daemon"))
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
            .enumerate()
            .map(|(index, session_id)| DaemonSessionInfo {
                runtime: self.session_runtime(&session_id),
                session_id,
                revision: 0,
                created_order: index as u64,
            })
            .collect())
    }

    fn create_session(&self, prefix: &str) -> Result<String, String> {
        let mut next = self.next_session_index.lock().unwrap();
        let session_id = format!("{prefix}{}", *next);
        *next += 1;
        self.set_session_runtime(&session_id, TerminalRuntimeState::running("fake daemon"));
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
                runtime: self.session_runtime(session_id),
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
        self.session_runtimes.lock().unwrap().remove(session_id);
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
