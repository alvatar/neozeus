#[path = "../tests/hud.rs"]
mod hud;
#[path = "../tests/input.rs"]
mod input;
#[path = "../tests/scene.rs"]
mod scene;
#[path = "../tests/terminals.rs"]
mod terminals;

use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::AppSessionState,
    conversations::{
        AgentTaskStore, ConversationPersistenceState, ConversationStore, MessageTransportAdapter,
    },
    hud::{
        AgentListView, ComposerView, ConversationListView, HudInputCaptureState, HudLayoutState,
        HudModalState, InfoBarView, ThreadView,
    },
    terminals::{
        AttachedDaemonSession, DaemonSessionInfo, TerminalBridge, TerminalCommand,
        TerminalDaemonClient, TerminalDaemonClientResource, TerminalDebugStats,
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

/// Builds a pressed-key [`KeyboardInput`] event for tests that care about text payloads.
///
/// The helper fills in the minimal Bevy fields needed by the input systems and uses the supplied text
/// both as `text` and as the logical character key when present, which matches how typing-like events
/// are normally observed by the HUD editor paths.
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

/// Creates a unique temporary directory for a test case.
///
/// Uniqueness comes from the current UNIX timestamp in nanoseconds appended to the caller-supplied
/// prefix. The directory is created eagerly so tests can assume the returned path already exists.
pub(super) fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

/// Creates a test terminal bridge together with handles that let the test inspect what was sent.
///
/// The returned tuple contains the bridge itself, the receiving end of the command channel, and the
/// update mailbox so tests can drive both command emission and inbound update delivery without a real
/// runtime worker.
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

/// Convenience wrapper that returns only the bridge and mailbox for tests that do not need to inspect
/// the outgoing command channel directly.
///
/// It is built on top of [`capturing_bridge`] and simply discards the receiver side of the command
/// channel.
pub(super) fn test_bridge() -> (TerminalBridge, Arc<TerminalUpdateMailbox>) {
    let (bridge, _input_rx, mailbox) = capturing_bridge();
    (bridge, mailbox)
}

/// Seeds a test world with the minimal default HUD resources expected by HUD/input systems.
///
/// The helper also ensures a terminal focus resource exists, because many systems assume it is always
/// present even in stripped-down unit-test worlds.
pub(super) fn insert_default_hud_resources(world: &mut World) {
    // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
    world.insert_resource(HudLayoutState::default());
    world.insert_resource(crate::hud::AgentListUiState::default());
    world.insert_resource(crate::hud::ConversationListUiState::default());
    world.insert_resource(crate::hud::InfoBarUiState);
    world.insert_resource(crate::hud::ThreadPaneUiState);
    if !world.contains_resource::<AppSessionState>() {
        world.insert_resource(AppSessionState::default());
    }
    world.insert_resource(HudInputCaptureState::default());
    if !world.contains_resource::<AgentListView>() {
        world.insert_resource(AgentListView::default());
    }
    if !world.contains_resource::<ConversationListView>() {
        world.insert_resource(ConversationListView::default());
    }
    if !world.contains_resource::<ThreadView>() {
        world.insert_resource(ThreadView::default());
    }
    if !world.contains_resource::<ComposerView>() {
        world.insert_resource(ComposerView::default());
    }
    if !world.contains_resource::<InfoBarView>() {
        world.insert_resource(InfoBarView::default());
    }
    if !world.contains_resource::<crate::agents::AgentStatusStore>() {
        world.insert_resource(crate::agents::AgentStatusStore::default());
    }
    if !world.contains_resource::<crate::usage::UsageSnapshot>() {
        world.insert_resource(crate::usage::UsageSnapshot::default());
    }
    if !world.contains_resource::<crate::usage::UsagePersistenceState>() {
        world.insert_resource(crate::usage::default_usage_persistence_state());
    }
    if !world.contains_resource::<crate::terminals::TerminalFocusState>() {
        world.insert_resource(crate::terminals::TerminalFocusState::default());
    }
}

/// Inserts a prepared terminal manager into a test world, along with the mirrored test focus state
/// when tests enable that path.
///
/// This keeps tests from having to know about the manager/focus dual-resource arrangement directly.
pub(super) fn insert_terminal_manager_resources(
    world: &mut World,
    terminal_manager: crate::terminals::TerminalManager,
) {
    // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
    #[cfg(test)]
    {
        world.insert_resource(terminal_manager.clone_focus_state());
    }
    if !world.contains_resource::<AgentCatalog>() {
        let mut catalog = AgentCatalog::default();
        let mut runtime_index = AgentRuntimeIndex::default();
        for terminal_id in terminal_manager.terminal_ids().iter().copied() {
            let session_name = terminal_manager
                .get(terminal_id)
                .expect("terminal should exist")
                .session_name
                .clone();
            let agent_id = catalog.create_agent(
                None,
                crate::agents::AgentKind::Terminal,
                crate::agents::AgentCapabilities::terminal_defaults(),
            );
            runtime_index.link_terminal(agent_id, terminal_id, session_name, None);
        }
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
    }
    if !world.contains_resource::<AppSessionState>() {
        world.insert_resource(AppSessionState::default());
    }
    if !world.contains_resource::<ConversationStore>() {
        world.insert_resource(ConversationStore::default());
    }
    if !world.contains_resource::<ConversationPersistenceState>() {
        world.insert_resource(ConversationPersistenceState::default());
    }
    if !world.contains_resource::<AgentTaskStore>() {
        world.insert_resource(AgentTaskStore::default());
    }
    if !world.contains_resource::<MessageTransportAdapter>() {
        world.insert_resource(MessageTransportAdapter);
    }
    if !world.contains_resource::<crate::hud::AgentListView>() {
        let rows = {
            let catalog = world.resource::<AgentCatalog>();
            let runtime_index = world.resource::<AgentRuntimeIndex>();
            let focus_state = world.resource::<crate::terminals::TerminalFocusState>();
            let active_agent = focus_state
                .active_id()
                .and_then(|terminal_id| runtime_index.agent_for_terminal(terminal_id));
            let rows = catalog
                .iter()
                .map(|(agent_id, label)| crate::hud::AgentListRowView {
                    agent_id,
                    terminal_id: runtime_index.primary_terminal(agent_id),
                    label: label.to_owned(),
                    focused: active_agent == Some(agent_id),
                    has_tasks: false,
                    interactive: true,
                    status: crate::agents::AgentStatus::Unknown,
                })
                .collect::<Vec<_>>();
            (active_agent, rows)
        };
        if let Some(active_agent) = rows.0 {
            world.resource_mut::<AppSessionState>().active_agent = Some(active_agent);
        }
        world.insert_resource(crate::hud::AgentListView { rows: rows.1 });
    }
    world.insert_resource(terminal_manager);
}

/// Inserts an explicit HUD resource triple into a test world.
///
/// This is the lower-level helper used when a test wants exact control over layout, modal, and input
/// capture state instead of the defaults from [`insert_default_hud_resources`].
pub(super) fn insert_hud_resources(
    world: &mut World,
    layout_state: HudLayoutState,
    modal_state: HudModalState,
    input_capture: HudInputCaptureState,
) {
    // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
    if !world.contains_resource::<crate::hud::AgentListUiState>() {
        world.insert_resource(crate::hud::AgentListUiState::default());
    }
    if !world.contains_resource::<crate::hud::ConversationListUiState>() {
        world.insert_resource(crate::hud::ConversationListUiState::default());
    }
    if !world.contains_resource::<crate::hud::InfoBarUiState>() {
        world.insert_resource(crate::hud::InfoBarUiState);
    }
    if !world.contains_resource::<crate::hud::ThreadPaneUiState>() {
        world.insert_resource(crate::hud::ThreadPaneUiState);
    }
    world.insert_resource(layout_state);
    let mut app_session = world
        .remove_resource::<AppSessionState>()
        .unwrap_or_default();
    let message_box_visible = modal_state.message_box.visible;
    let task_dialog_visible = modal_state.task_dialog.visible;
    app_session.composer.message_editor = modal_state.message_box;
    app_session.composer.task_editor = modal_state.task_dialog;
    let default_agent = app_session
        .active_agent
        .or_else(|| {
            world
                .get_resource::<AgentRuntimeIndex>()
                .and_then(|runtime_index| runtime_index.agent_ids().next())
        })
        .or_else(|| {
            (message_box_visible || task_dialog_visible).then_some(crate::agents::AgentId(1))
        });
    app_session.composer.session = if app_session.composer.message_editor.visible {
        default_agent.map(|agent_id| crate::composer::ComposerSession {
            mode: crate::composer::ComposerMode::Message { agent_id },
        })
    } else if app_session.composer.task_editor.visible {
        default_agent.map(|agent_id| crate::composer::ComposerSession {
            mode: crate::composer::ComposerMode::TaskEdit { agent_id },
        })
    } else {
        None
    };
    world.insert_resource(app_session);
    world.insert_resource(input_capture);
}

/// Inserts a serialized test HUD snapshot into a world by expanding it back into live resources.
///
/// The helper mirrors the production split between retained HUD resources and also ensures focus
/// state exists, because many tests snapshot only the HUD layer but still execute systems that touch
/// terminal focus.
#[cfg(test)]
pub(super) fn insert_test_hud_state(world: &mut World, hud_state: crate::hud::HudState) {
    // Keep the editor or collection mutation explicit so cursor state and stored data stay synchronized after each change.
    let (layout_state, modal_state, input_capture) = hud_state.into_resources();
    insert_hud_resources(world, layout_state, modal_state, input_capture);
    if !world.contains_resource::<AgentListView>() {
        world.insert_resource(AgentListView::default());
    }
    if !world.contains_resource::<ConversationListView>() {
        world.insert_resource(ConversationListView::default());
    }
    if !world.contains_resource::<ThreadView>() {
        world.insert_resource(ThreadView::default());
    }
    if !world.contains_resource::<ComposerView>() {
        world.insert_resource(ComposerView::default());
    }
    if !world.contains_resource::<InfoBarView>() {
        world.insert_resource(InfoBarView::default());
    }
    if !world.contains_resource::<crate::usage::UsageSnapshot>() {
        world.insert_resource(crate::usage::UsageSnapshot::default());
    }
    if !world.contains_resource::<crate::usage::UsagePersistenceState>() {
        world.insert_resource(crate::usage::default_usage_persistence_state());
    }
    if !world.contains_resource::<crate::terminals::TerminalFocusState>() {
        world.insert_resource(crate::terminals::TerminalFocusState::default());
    }
}

/// Captures the live HUD resources in a world into the compact `HudState` test snapshot type.
///
/// Tests use this to assert whole-HUD state transitions without comparing every resource manually.
#[cfg(test)]
pub(super) fn snapshot_test_hud_state(world: &World) -> crate::hud::HudState {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let app_session = world.resource::<AppSessionState>();
    let runtime_index = world.get_resource::<AgentRuntimeIndex>();
    let mut message_box = app_session.composer.message_editor.clone();
    let mut task_dialog = app_session.composer.task_editor.clone();
    let target_terminal = app_session.composer.current_agent().and_then(|agent_id| {
        runtime_index.and_then(|runtime_index| runtime_index.primary_terminal(agent_id))
    });
    #[cfg(test)]
    {
        message_box.target_terminal = if message_box.visible {
            target_terminal
        } else {
            None
        };
        task_dialog.target_terminal = if task_dialog.visible {
            target_terminal
        } else {
            None
        };
    }
    crate::hud::HudState::from_resources(
        world.resource::<HudLayoutState>(),
        &HudModalState {
            message_box,
            task_dialog,
        },
        world.resource::<HudInputCaptureState>(),
    )
}

#[derive(Default)]
pub(super) struct FakeDaemonClient {
    pub(super) sessions: Mutex<BTreeSet<String>>,
    pub(super) session_runtimes: Mutex<std::collections::HashMap<String, TerminalRuntimeState>>,
    pub(super) sent_commands: Mutex<Vec<(String, TerminalCommand)>>,
    pub(super) resize_requests: Mutex<Vec<(String, usize, usize)>>,
    pub(super) created_sessions: Mutex<Vec<(String, Option<String>)>>,
    pub(super) fail_kill: Mutex<bool>,
    pub(super) next_session_index: Mutex<u64>,
    updates: Mutex<std::collections::HashMap<String, Vec<mpsc::Sender<TerminalUpdate>>>>,
}

impl FakeDaemonClient {
    /// Broadcasts a synthetic terminal update to every test subscriber attached to a fake session.
    ///
    /// The fake daemon stores one sender per attachment. Emitting clones the update to all of them so
    /// multi-client tests can exercise fanout behavior without a real daemon server.
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

    /// Creates or updates the stored runtime state for one fake daemon session.
    ///
    /// The helper also ensures the session id exists in the session set, because tests often seed a
    /// runtime state as the act that makes a fake session "exist".
    pub(super) fn set_session_runtime(&self, session_id: &str, runtime: TerminalRuntimeState) {
        self.sessions.lock().unwrap().insert(session_id.to_owned());
        self.session_runtimes
            .lock()
            .unwrap()
            .insert(session_id.to_owned(), runtime);
    }

    /// Looks up the stored runtime state for a fake session, falling back to a generic running state.
    ///
    /// The fallback keeps tests terse: callers only need to seed special runtime conditions when they
    /// care about them, while ordinary sessions behave as healthy running terminals by default.
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
    /// Returns the fake daemon's current session list in deterministic creation order.
    ///
    /// Ordering comes from iterating the stored session ids and enumerating them into `created_order`,
    /// which is sufficient for tests that care about stable session ordering semantics.
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

    /// Creates a new fake daemon session id using the requested prefix.
    ///
    /// The implementation just increments an in-memory counter and seeds the new session with a
    /// default running runtime state, which is enough for tests that only need unique session names.
    fn create_session(&self, prefix: &str, cwd: Option<&str>) -> Result<String, String> {
        let mut next = self.next_session_index.lock().unwrap();
        let session_id = format!("{prefix}{}", *next);
        *next += 1;
        self.created_sessions
            .lock()
            .unwrap()
            .push((session_id.clone(), cwd.map(str::to_owned)));
        self.set_session_runtime(&session_id, TerminalRuntimeState::running("fake daemon"));
        Ok(session_id)
    }

    /// Attaches to a fake session by registering a new update receiver and returning an initial
    /// snapshot.
    ///
    /// Missing sessions produce an error just like the real daemon. Successful attaches return a
    /// default blank surface plus the session's current runtime state so tests begin from a complete
    /// snapshot.
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

    /// Records a sent command in the fake daemon without executing it.
    ///
    /// Tests inspect `sent_commands` afterward to verify what would have been delivered to the real
    /// daemon/session.
    fn send_command(&self, session_id: &str, command: TerminalCommand) -> Result<(), String> {
        self.sent_commands
            .lock()
            .unwrap()
            .push((session_id.to_owned(), command));
        Ok(())
    }

    /// Records a resize request for later inspection by the test.
    ///
    /// The fake client does not maintain a full PTY surface here; it just logs the resize tuple so
    /// tests can assert the request was issued.
    fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String> {
        self.resize_requests
            .lock()
            .unwrap()
            .push((session_id.to_owned(), cols, rows));
        Ok(())
    }

    /// Removes a fake session unless the test has configured kill failures.
    ///
    /// When `fail_kill` is set, the method returns an error to let tests exercise best-effort vs
    /// hard-failure kill paths. Otherwise it clears the session, runtime state, and subscriber list.
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

/// Wraps a [`FakeDaemonClient`] in the same resource type production code uses for daemon access.
///
/// This lets tests exercise the exact same systems that the real app runs, just with a fake client
/// behind the resource boundary.
pub(super) fn fake_daemon_resource(client: Arc<FakeDaemonClient>) -> TerminalDaemonClientResource {
    TerminalDaemonClientResource::from_client(client)
}

/// Builds a [`TerminalRuntimeSpawner`] backed by the fake daemon resource.
///
/// Tests that need to go through the normal runtime-spawner API use this instead of constructing the
/// spawner manually.
pub(super) fn fake_runtime_spawner(client: Arc<FakeDaemonClient>) -> TerminalRuntimeSpawner {
    TerminalRuntimeSpawner::for_tests(fake_daemon_resource(client))
}

/// Builds a terminal surface with one text run written onto a single row.
///
/// This is the compact fixture builder used by many rendering and damage tests: start from a blank
/// surface, then write the supplied string beginning at column zero on row `y`.
pub(super) fn surface_with_text(rows: usize, cols: usize, y: usize, text: &str) -> TerminalSurface {
    let mut surface = TerminalSurface::new(cols, rows);
    for (x, ch) in text.chars().enumerate() {
        surface.set_text_cell(x, y, &ch.to_string());
    }
    surface
}
