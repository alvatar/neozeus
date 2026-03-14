use super::*;
use crate::*;

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_owned(),
            Err(_) => "unknown panic payload".to_owned(),
        },
    }
}

pub(crate) fn append_debug_log(message: impl AsRef<str>) {
    let message = message.as_ref();
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(DEBUG_LOG_PATH)
    {
        let _ = writeln!(file, "{message}");
    }
}

#[derive(Clone, Default)]
pub(crate) struct TerminalDebugStats {
    pub(crate) key_events_seen: u64,
    pub(crate) commands_queued: u64,
    pub(crate) pty_bytes_written: u64,
    pub(crate) pty_bytes_read: u64,
    pub(crate) snapshots_sent: u64,
    pub(crate) snapshots_applied: u64,
    pub(crate) updates_dropped: u64,
    pub(crate) dirty_rows_uploaded: u64,
    pub(crate) compose_micros: u64,
    pub(crate) last_key: String,
    pub(crate) last_command: String,
    pub(crate) last_error: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct TerminalId(pub(crate) u64);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalPanel {
    pub(crate) id: TerminalId,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalPanelFrame {
    pub(crate) id: TerminalId,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct TerminalPresentation {
    pub(crate) home_position: Vec2,
    pub(crate) current_position: Vec2,
    pub(crate) target_position: Vec2,
    pub(crate) current_size: Vec2,
    pub(crate) target_size: Vec2,
    pub(crate) current_alpha: f32,
    pub(crate) target_alpha: f32,
    pub(crate) current_z: f32,
    pub(crate) target_z: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalDisplayMode {
    #[default]
    Smooth,
    PixelPerfect,
}

pub(crate) struct ManagedTerminal {
    pub(crate) bridge: TerminalBridge,
    pub(crate) latest: TerminalSnapshot,
    pub(crate) pending_damage: Option<TerminalDamage>,
    pub(crate) surface_revision: u64,
    pub(crate) uploaded_revision: u64,
    pub(crate) texture_state: TerminalTextureState,
    pub(crate) display_mode: TerminalDisplayMode,
}

#[derive(Resource)]
pub(crate) struct TerminalManager {
    pub(crate) next_id: u64,
    pub(crate) active_id: Option<TerminalId>,
    pub(crate) order: Vec<TerminalId>,
    pub(crate) helper_entities: Option<TerminalFontEntities>,
    pub(crate) event_loop_proxy: EventLoopProxy<WinitUserEvent>,
    pub(crate) terminals: HashMap<TerminalId, ManagedTerminal>,
}

#[derive(Default)]
pub(crate) struct PendingTerminalUpdates {
    pub(crate) latest_frame: Option<TerminalFrameUpdate>,
    pub(crate) latest_status: Option<LatestTerminalStatus>,
    pub(crate) dropped_frames: u64,
    pub(crate) wake_pending: bool,
}

#[derive(Resource)]
pub(crate) struct TerminalBridge {
    pub(crate) input_tx: Sender<TerminalCommand>,
    pub(crate) update_mailbox: Arc<Mutex<PendingTerminalUpdates>>,
    pub(crate) debug_stats: Arc<Mutex<TerminalDebugStats>>,
}

fn terminal_home_position(slot: usize) -> Vec2 {
    const COLUMNS: usize = 3;
    const STEP_X: f32 = 360.0;
    const STEP_Y: f32 = 220.0;
    let column = slot % COLUMNS;
    let row = slot / COLUMNS;
    Vec2::new(-360.0 + column as f32 * STEP_X, 120.0 - row as f32 * STEP_Y)
}

impl TerminalManager {
    pub(crate) fn new(event_loop_proxy: EventLoopProxy<WinitUserEvent>) -> Self {
        Self {
            next_id: 1,
            active_id: None,
            order: Vec::new(),
            helper_entities: None,
            event_loop_proxy,
            terminals: HashMap::new(),
        }
    }

    pub(crate) fn set_helper_entities(&mut self, helper_entities: TerminalFontEntities) {
        self.helper_entities = Some(helper_entities);
        for terminal in self.terminals.values_mut() {
            terminal.texture_state.helper_entities = Some(helper_entities);
        }
    }

    pub(crate) fn spawn_terminal(
        &mut self,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        auto_verify: bool,
    ) -> Result<TerminalId, String> {
        let Some(helper_entities) = self.helper_entities else {
            return Err("terminal helper entities not initialized".into());
        };

        let slot = self.terminals.len();
        let id = TerminalId(self.next_id);
        self.next_id += 1;

        let home_position = terminal_home_position(slot);
        let presentation = TerminalPresentation {
            home_position,
            current_position: home_position,
            target_position: home_position,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 0.82,
            target_alpha: 0.82,
            current_z: -0.05,
            target_z: -0.05,
        };

        let image_handle = images.add(create_terminal_image(UVec2::ONE));
        commands.spawn((
            Sprite {
                color: Color::srgba(0.08, 0.08, 0.09, 0.94),
                custom_size: Some(Vec2::ONE),
                ..default()
            },
            Transform::from_xyz(
                home_position.x,
                home_position.y,
                presentation.current_z - 0.01,
            ),
            TerminalPanelFrame { id },
        ));
        commands.spawn((
            Sprite::from_image(image_handle.clone()),
            Transform::from_xyz(home_position.x, home_position.y, presentation.current_z),
            TerminalPlaneMarker,
            TerminalPanel { id },
            presentation,
        ));

        let bridge = TerminalBridge::spawn(self.event_loop_proxy.clone(), auto_verify);
        self.terminals.insert(
            id,
            ManagedTerminal {
                bridge,
                latest: TerminalSnapshot::default(),
                pending_damage: None,
                surface_revision: 0,
                uploaded_revision: 0,
                texture_state: TerminalTextureState {
                    image: Some(image_handle),
                    helper_entities: Some(helper_entities),
                    texture_size: UVec2::ONE,
                    cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
                },
                display_mode: TerminalDisplayMode::Smooth,
            },
        );
        self.focus_terminal(id);
        append_debug_log(format!("spawned terminal {}", id.0));
        Ok(id)
    }

    pub(crate) fn focus_terminal(&mut self, id: TerminalId) {
        if !self.terminals.contains_key(&id) {
            return;
        }
        self.active_id = Some(id);
        self.order.retain(|existing| *existing != id);
        self.order.push(id);
        append_debug_log(format!("focused terminal {}", id.0));
    }

    pub(crate) fn active_id(&self) -> Option<TerminalId> {
        self.active_id
    }

    fn active_terminal(&self) -> Option<&ManagedTerminal> {
        self.active_id.and_then(|id| self.terminals.get(&id))
    }

    fn active_terminal_mut(&mut self) -> Option<&mut ManagedTerminal> {
        self.active_id.and_then(|id| self.terminals.get_mut(&id))
    }

    pub(crate) fn active_bridge(&self) -> Option<&TerminalBridge> {
        self.active_terminal().map(|terminal| &terminal.bridge)
    }

    pub(crate) fn active_snapshot(&self) -> Option<&TerminalSnapshot> {
        self.active_terminal().map(|terminal| &terminal.latest)
    }

    pub(crate) fn active_texture_state(&self) -> Option<&TerminalTextureState> {
        self.active_terminal()
            .map(|terminal| &terminal.texture_state)
    }

    pub(crate) fn active_debug_stats(&self) -> TerminalDebugStats {
        self.active_bridge()
            .map(TerminalBridge::debug_stats_snapshot)
            .unwrap_or_default()
    }

    pub(crate) fn active_display_mode(&self) -> Option<TerminalDisplayMode> {
        self.active_terminal().map(|terminal| terminal.display_mode)
    }

    pub(crate) fn toggle_active_display_mode(&mut self) {
        let Some(terminal) = self.active_terminal_mut() else {
            return;
        };
        terminal.display_mode = match terminal.display_mode {
            TerminalDisplayMode::Smooth => TerminalDisplayMode::PixelPerfect,
            TerminalDisplayMode::PixelPerfect => TerminalDisplayMode::Smooth,
        };
        append_debug_log(format!(
            "active terminal display mode: {:?}",
            terminal.display_mode
        ));
    }

    pub(crate) fn terminal_ids(&self) -> &[TerminalId] {
        &self.order
    }
}

impl TerminalBridge {
    pub(crate) fn spawn(
        event_loop_proxy: EventLoopProxy<WinitUserEvent>,
        auto_verify: bool,
    ) -> Self {
        let (input_tx, input_rx) = mpsc::channel();
        let update_mailbox = Arc::new(Mutex::new(PendingTerminalUpdates::default()));
        let debug_stats = Arc::new(Mutex::new(TerminalDebugStats::default()));
        let worker_debug_stats = debug_stats.clone();
        let worker_event_loop_proxy = event_loop_proxy.clone();
        let worker_update_mailbox = update_mailbox.clone();

        thread::spawn(move || {
            append_debug_log("terminal worker thread spawn");
            let panic_update_mailbox = worker_update_mailbox.clone();
            let panic_debug_stats = worker_debug_stats.clone();
            let panic_event_loop_proxy = worker_event_loop_proxy.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal_worker(
                    input_rx,
                    worker_update_mailbox,
                    worker_debug_stats,
                    worker_event_loop_proxy,
                )
            }));
            if let Err(payload) = result {
                let message = panic_payload_to_string(payload);
                append_debug_log(format!("terminal worker panic: {message}"));
                enqueue_terminal_update(
                    &panic_update_mailbox,
                    TerminalUpdate::Status {
                        surface: None,
                        status: format!("terminal worker panicked: {message}"),
                    },
                    &panic_debug_stats,
                    &panic_event_loop_proxy,
                );
            }
        });

        if auto_verify {
            spawn_auto_verify_dispatcher(&input_tx, &debug_stats, &event_loop_proxy);
        }

        Self {
            input_tx,
            update_mailbox,
            debug_stats,
        }
    }

    pub(crate) fn send(&self, command: TerminalCommand) {
        let summary = summarize_terminal_command(&command).to_owned();
        match self.input_tx.send(command) {
            Ok(()) => {
                append_debug_log(format!("command queued: {summary}"));
                with_debug_stats(&self.debug_stats, |stats| {
                    stats.commands_queued += 1;
                    stats.last_command = summary;
                });
            }
            Err(_) => {
                append_debug_log(format!("command queue failed: {summary}"));
                with_debug_stats(&self.debug_stats, |stats| {
                    stats.last_command = summary;
                    stats.last_error = "input channel disconnected".into();
                });
            }
        }
    }

    pub(crate) fn note_key_event(&self, event: &KeyboardInput) {
        let summary = format!(
            "{:?} text={:?} logical={:?}",
            event.key_code, event.text, event.logical_key
        );
        append_debug_log(format!("key event: {summary}"));
        with_debug_stats(&self.debug_stats, |stats| {
            stats.key_events_seen += 1;
            stats.last_key = summary;
        });
    }

    pub(crate) fn note_snapshot_applied(&self) {
        with_debug_stats(&self.debug_stats, |stats| {
            stats.snapshots_applied += 1;
        });
    }

    pub(crate) fn debug_stats_snapshot(&self) -> TerminalDebugStats {
        match self.debug_stats.lock() {
            Ok(stats) => stats.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

pub(crate) fn with_debug_stats(
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    update: impl FnOnce(&mut TerminalDebugStats),
) {
    match debug_stats.lock() {
        Ok(mut stats) => update(&mut stats),
        Err(poisoned) => update(&mut poisoned.into_inner()),
    }
}

fn summarize_terminal_command(command: &TerminalCommand) -> &str {
    match command {
        TerminalCommand::InputText(_) => "InputText",
        TerminalCommand::InputEvent(_) => "InputEvent",
        TerminalCommand::SendCommand(_) => "SendCommand",
        TerminalCommand::ScrollDisplay(_) => "ScrollDisplay",
        TerminalCommand::Shutdown => "Shutdown",
    }
}

fn spawn_auto_verify_dispatcher(
    input_tx: &Sender<TerminalCommand>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    event_loop_proxy: &EventLoopProxy<WinitUserEvent>,
) {
    let Some(command) = env::var("NEOZEUS_AUTOVERIFY_COMMAND").ok() else {
        return;
    };
    let delay = env::var("NEOZEUS_AUTOVERIFY_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1500);

    let input_tx = input_tx.clone();
    let debug_stats = debug_stats.clone();
    let event_loop_proxy = event_loop_proxy.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(delay));
        append_debug_log(format!("auto-verify command dispatched: {command}"));
        match input_tx.send(TerminalCommand::SendCommand(command)) {
            Ok(()) => {
                with_debug_stats(&debug_stats, |stats| {
                    stats.commands_queued += 1;
                    stats.last_command = "SendCommand".into();
                });
                append_debug_log("command queued: SendCommand");
                let _ = event_loop_proxy.send_event(WinitUserEvent::WakeUp);
            }
            Err(_) => {
                append_debug_log("command queue failed: SendCommand");
                with_debug_stats(&debug_stats, |stats| {
                    stats.last_command = "SendCommand".into();
                    stats.last_error = "input channel disconnected".into();
                });
            }
        }
    });
}

pub(crate) fn set_terminal_error(
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    message: impl Into<String>,
) {
    let message = message.into();
    append_debug_log(format!("terminal error: {message}"));
    with_debug_stats(debug_stats, |stats| {
        stats.last_error = message;
    });
}

pub(crate) fn record_terminal_update(
    update_mailbox: &Arc<Mutex<PendingTerminalUpdates>>,
    update: TerminalUpdate,
) -> bool {
    let mut pending = match update_mailbox.lock() {
        Ok(pending) => pending,
        Err(poisoned) => poisoned.into_inner(),
    };
    match update {
        TerminalUpdate::Frame(frame) => {
            if pending.latest_frame.replace(frame).is_some() {
                pending.dropped_frames += 1;
            }
        }
        TerminalUpdate::Status { status, surface } => {
            pending.latest_status = Some((status, surface));
        }
    }
    if pending.wake_pending {
        false
    } else {
        pending.wake_pending = true;
        true
    }
}

pub(crate) fn enqueue_terminal_update(
    update_mailbox: &Arc<Mutex<PendingTerminalUpdates>>,
    update: TerminalUpdate,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    event_loop_proxy: &EventLoopProxy<WinitUserEvent>,
) {
    let should_wake = record_terminal_update(update_mailbox, update);

    with_debug_stats(debug_stats, |stats| {
        stats.snapshots_sent += 1;
    });
    if should_wake {
        let _ = event_loop_proxy.send_event(WinitUserEvent::WakeUp);
    }
}

pub(crate) fn send_terminal_status_update(
    update_mailbox: &Arc<Mutex<PendingTerminalUpdates>>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    terminal: &Term<VoidListener>,
    event_loop_proxy: &EventLoopProxy<WinitUserEvent>,
    status: impl Into<String>,
) {
    let status = status.into();
    append_debug_log(format!("status snapshot: {status}"));
    enqueue_terminal_update(
        update_mailbox,
        TerminalUpdate::Status {
            surface: Some(build_surface(terminal)),
            status: status.clone(),
        },
        debug_stats,
        event_loop_proxy,
    );
    set_terminal_error(debug_stats, status);
}

pub(crate) fn drain_terminal_updates(
    update_mailbox: &Mutex<PendingTerminalUpdates>,
) -> DrainedTerminalUpdates {
    let mut pending = match update_mailbox.lock() {
        Ok(pending) => pending,
        Err(poisoned) => poisoned.into_inner(),
    };
    pending.wake_pending = false;
    (
        pending.latest_frame.take(),
        pending.latest_status.take(),
        std::mem::take(&mut pending.dropped_frames),
    )
}

pub(crate) fn poll_terminal_snapshots(mut terminal_manager: ResMut<TerminalManager>) {
    for terminal in terminal_manager.terminals.values_mut() {
        let (latest_frame, latest_status, dropped_frames) =
            drain_terminal_updates(&terminal.bridge.update_mailbox);
        if dropped_frames > 0 {
            with_debug_stats(&terminal.bridge.debug_stats, |stats| {
                stats.updates_dropped += dropped_frames;
            });
        }

        if let Some((status, surface)) = latest_status {
            terminal.latest.status = status;
            if let Some(surface) = surface {
                terminal.latest.surface = Some(surface);
                terminal.surface_revision += 1;
                terminal.pending_damage = Some(TerminalDamage::Full);
            }
            terminal.bridge.note_snapshot_applied();
        }

        if let Some(frame) = latest_frame {
            terminal.latest.status = frame.status;
            terminal.latest.surface = Some(frame.surface);
            terminal.surface_revision += 1;
            terminal.pending_damage = Some(if dropped_frames > 0 {
                TerminalDamage::Full
            } else {
                frame.damage
            });
            terminal.bridge.note_snapshot_applied();
        }
    }
}

impl Drop for TerminalBridge {
    fn drop(&mut self) {
        let _ = self.input_tx.send(TerminalCommand::Shutdown);
    }
}
