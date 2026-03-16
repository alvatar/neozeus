mod hud;
mod input;
mod scene;
mod terminals;

use crate::terminals::{
    CachedTerminalGlyph, TerminalBridge, TerminalCommand, TerminalDebugStats, TerminalSurface,
    TerminalUpdateMailbox, TmuxClient, TmuxClientResource,
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

pub(super) fn test_bridge() -> (TerminalBridge, Arc<TerminalUpdateMailbox>) {
    let (input_tx, _input_rx) = mpsc::channel::<TerminalCommand>();
    let mailbox = Arc::new(TerminalUpdateMailbox::default());
    let bridge = TerminalBridge::new(
        input_tx,
        mailbox.clone(),
        Arc::new(Mutex::new(TerminalDebugStats::default())),
    );
    (bridge, mailbox)
}

#[derive(Default)]
pub(super) struct FakeTmuxClient {
    pub(super) sessions: Mutex<BTreeSet<String>>,
    pub(super) collision_checks_remaining: Mutex<usize>,
    pub(super) fail_kill: Mutex<bool>,
}

impl FakeTmuxClient {
    pub(super) fn with_collisions(count: usize) -> Self {
        Self {
            collision_checks_remaining: Mutex::new(count),
            ..Default::default()
        }
    }
}

impl TmuxClient for FakeTmuxClient {
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
        if *self.fail_kill.lock().unwrap() {
            return Err("kill failed".into());
        }
        self.sessions.lock().unwrap().remove(name);
        Ok(())
    }
}

pub(super) fn fake_tmux_resource(client: Arc<FakeTmuxClient>) -> TmuxClientResource {
    TmuxClientResource::from_client(client)
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
