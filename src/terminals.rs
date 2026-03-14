#![allow(
    dead_code,
    reason = "legacy single-plane and per-cell terminal code kept temporarily during terminal-manager transition"
)]

use crate::*;

pub(crate) fn create_terminal_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[
            DEFAULT_BG.r(),
            DEFAULT_BG.g(),
            DEFAULT_BG.b(),
            DEFAULT_BG.a(),
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

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

fn dump_terminal_image_ppm(image: &Image, path: &Path) -> Result<(), String> {
    let width = image.texture_descriptor.size.width;
    let height = image.texture_descriptor.size.height;
    let data = image
        .data
        .as_ref()
        .ok_or_else(|| "image data missing".to_owned())?;
    let mut output = Vec::with_capacity((width as usize * height as usize * 3) + 64);
    output.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
    for pixel in data.chunks_exact(4) {
        output.extend_from_slice(&pixel[..3]);
    }
    fs::write(path, output).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

#[derive(Clone, Default)]
pub(crate) struct TerminalDebugStats {
    pub(crate) key_events_seen: u64,
    pub(crate) commands_queued: u64,
    pub(crate) pty_bytes_written: u64,
    pub(crate) pty_bytes_read: u64,
    pub(crate) snapshots_sent: u64,
    pub(crate) snapshots_applied: u64,
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

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct TerminalPresentation {
    home_position: Vec2,
    current_position: Vec2,
    target_position: Vec2,
    current_size: Vec2,
    target_size: Vec2,
    current_alpha: f32,
    target_alpha: f32,
    current_z: f32,
    target_z: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalDisplayMode {
    #[default]
    Smooth,
    PixelPerfect,
}

struct ManagedTerminal {
    bridge: TerminalBridge,
    latest: TerminalSnapshot,
    texture_state: TerminalTextureState,
    sprite_entity: Entity,
    display_mode: TerminalDisplayMode,
}

#[derive(Resource)]
pub(crate) struct TerminalManager {
    next_id: u64,
    active_id: Option<TerminalId>,
    order: Vec<TerminalId>,
    helper_entities: Option<TerminalFontEntities>,
    event_loop_proxy: EventLoopProxy<WinitUserEvent>,
    terminals: HashMap<TerminalId, ManagedTerminal>,
}

#[derive(Resource)]
pub(crate) struct TerminalBridge {
    input_tx: Sender<TerminalCommand>,
    snapshot_rx: Mutex<Receiver<TerminalSnapshot>>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
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
        let sprite_entity = commands
            .spawn((
                Sprite::from_image(image_handle.clone()),
                Transform::from_xyz(home_position.x, home_position.y, presentation.current_z),
                TerminalPlaneMarker,
                TerminalPanel { id },
                presentation,
            ))
            .id();

        let bridge = TerminalBridge::spawn(self.event_loop_proxy.clone(), auto_verify);
        self.terminals.insert(
            id,
            ManagedTerminal {
                bridge,
                latest: TerminalSnapshot::default(),
                texture_state: TerminalTextureState {
                    image: Some(image_handle),
                    helper_entities: Some(helper_entities),
                    texture_size: UVec2::ONE,
                    cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
                    last_surface: None,
                },
                sprite_entity,
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
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let debug_stats = Arc::new(Mutex::new(TerminalDebugStats::default()));
        let worker_debug_stats = debug_stats.clone();
        let worker_event_loop_proxy = event_loop_proxy.clone();

        thread::spawn(move || {
            append_debug_log("terminal worker thread spawn");
            let panic_snapshot_tx = snapshot_tx.clone();
            let panic_event_loop_proxy = worker_event_loop_proxy.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal_worker(
                    input_rx,
                    snapshot_tx,
                    worker_debug_stats,
                    worker_event_loop_proxy,
                )
            }));
            if let Err(payload) = result {
                let message = panic_payload_to_string(payload);
                append_debug_log(format!("terminal worker panic: {message}"));
                let _ = panic_snapshot_tx.send(TerminalSnapshot {
                    surface: None,
                    status: format!("terminal worker panicked: {message}"),
                });
                let _ = panic_event_loop_proxy.send_event(WinitUserEvent::WakeUp);
            }
        });

        if auto_verify {
            spawn_auto_verify_dispatcher(&input_tx, &debug_stats, &event_loop_proxy);
        }

        Self {
            input_tx,
            snapshot_rx: Mutex::new(snapshot_rx),
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

fn with_debug_stats(
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

fn set_terminal_error(debug_stats: &Arc<Mutex<TerminalDebugStats>>, message: impl Into<String>) {
    let message = message.into();
    append_debug_log(format!("terminal error: {message}"));
    with_debug_stats(debug_stats, |stats| {
        stats.last_error = message;
    });
}

fn send_terminal_status_snapshot(
    snapshot_tx: &Sender<TerminalSnapshot>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    terminal: &Term<VoidListener>,
    event_loop_proxy: &EventLoopProxy<WinitUserEvent>,
    status: impl Into<String>,
) {
    let status = status.into();
    append_debug_log(format!("status snapshot: {status}"));
    let snapshot = TerminalSnapshot {
        surface: Some(build_surface(terminal)),
        status: status.clone(),
    };
    if snapshot_tx.send(snapshot).is_ok() {
        with_debug_stats(debug_stats, |stats| {
            stats.snapshots_sent += 1;
        });
        let _ = event_loop_proxy.send_event(WinitUserEvent::WakeUp);
    }
    set_terminal_error(debug_stats, status);
}

#[derive(Resource, Default)]
pub(crate) struct TerminalView {
    pub(crate) latest: TerminalSnapshot,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalFontState {
    pub(crate) report: Option<Result<TerminalFontReport, String>>,
    pub(crate) primary_font: Option<Handle<Font>>,
    pub(crate) private_use_font: Option<Handle<Font>>,
    pub(crate) emoji_font: Option<Handle<Font>>,
}

#[derive(Resource)]
pub(crate) struct TerminalTextRenderer {
    font_system: Option<CtFontSystem>,
    swash_cache: CtSwashCache,
}

impl Default for TerminalTextRenderer {
    fn default() -> Self {
        Self {
            font_system: None,
            swash_cache: CtSwashCache::new(),
        }
    }
}

#[derive(Resource)]
pub(crate) struct TerminalPlaneState {
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) distance: f32,
    pub(crate) focal_length: f32,
    pub(crate) offset: Vec2,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalPointerState {
    pub(crate) scroll_drag_remainder_px: f32,
}

impl Default for TerminalPlaneState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            focal_length: 10.0,
            offset: Vec2::ZERO,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct TerminalSceneState {
    cols: usize,
    rows: usize,
    initialized: bool,
    last_surface: Option<TerminalSurface>,
    last_layout_key: Option<PlaneLayoutKey>,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalTextureState {
    pub(crate) image: Option<Handle<Image>>,
    pub(crate) helper_entities: Option<TerminalFontEntities>,
    pub(crate) texture_size: UVec2,
    pub(crate) cell_size: UVec2,
    last_surface: Option<TerminalSurface>,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalGlyphCache {
    glyphs: HashMap<TerminalGlyphCacheKey, CachedTerminalGlyph>,
}

#[derive(Clone, Copy)]
pub(crate) struct TerminalFontEntities {
    pub(crate) primary: Entity,
    pub(crate) private_use: Entity,
    pub(crate) emoji: Entity,
}

#[derive(Component)]
pub(crate) struct TerminalPlaneMarker;

#[derive(Component)]
pub(crate) struct TerminalCameraMarker;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TerminalFontRole {
    Primary,
    PrivateUse,
    Emoji,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalGlyphCacheKey {
    pub(crate) text: String,
    pub(crate) font_role: TerminalFontRole,
    pub(crate) width_cells: u8,
    pub(crate) cell_width: u32,
    pub(crate) cell_height: u32,
}

#[derive(Clone)]
pub(crate) struct CachedTerminalGlyph {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
    pub(crate) preserve_color: bool,
}

#[derive(Clone, Default, PartialEq)]
pub(crate) struct TerminalSnapshot {
    pub(crate) surface: Option<TerminalSurface>,
    pub(crate) status: String,
}

pub(crate) enum TerminalCommand {
    InputText(String),
    InputEvent(String),
    SendCommand(String),
    ScrollDisplay(i32),
    Shutdown,
}

struct TerminalDimensions {
    cols: usize,
    rows: usize,
}

impl Dimensions for TerminalDimensions {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Clone, Debug, PartialEq)]
struct TerminalCell {
    pub(crate) text: String,
    fg: egui::Color32,
    bg: egui::Color32,
    width: u8,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            text: String::new(),
            fg: egui::Color32::from_rgb(220, 220, 220),
            bg: DEFAULT_BG,
            width: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalCursorShape {
    Block,
    Underline,
    Beam,
}

#[derive(Clone, Debug, PartialEq)]
struct TerminalCursor {
    x: usize,
    y: usize,
    shape: TerminalCursorShape,
    visible: bool,
    color: egui::Color32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerminalSurface {
    cols: usize,
    rows: usize,
    cells: Vec<TerminalCell>,
    cursor: Option<TerminalCursor>,
}

impl TerminalSurface {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![TerminalCell::default(); cols.saturating_mul(rows)],
            cursor: None,
        }
    }

    fn set_cell(&mut self, x: usize, y: usize, cell: TerminalCell) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        let index = y * self.cols + x;
        self.cells[index] = cell;
    }

    fn cell(&self, x: usize, y: usize) -> &TerminalCell {
        &self.cells[y * self.cols + x]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalFontFace {
    pub(crate) family: String,
    pub(crate) path: PathBuf,
    source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalFontReport {
    pub(crate) requested_family: String,
    pub(crate) primary: TerminalFontFace,
    pub(crate) fallbacks: Vec<TerminalFontFace>,
}

#[derive(Component, Clone, Copy)]
struct TerminalCellIndex {
    x: usize,
    y: usize,
}

#[derive(Component)]
struct TerminalBackgroundMarker;

#[derive(Component)]
struct TerminalGlyphMarker;

#[derive(Component)]
struct TerminalCursorMarker;

type BackgroundQueryItem<'a> = (
    Entity,
    &'a TerminalCellIndex,
    &'a mut Sprite,
    &'a mut Transform,
    &'a mut Visibility,
);

type GlyphQueryItem<'a> = (
    Entity,
    &'a TerminalCellIndex,
    &'a mut Text2d,
    &'a mut TextFont,
    &'a mut TextColor,
    &'a mut TextBounds,
    &'a mut Transform,
    &'a mut Visibility,
);

type BackgroundQueryFilter = (
    With<TerminalBackgroundMarker>,
    Without<TerminalGlyphMarker>,
    Without<TerminalCursorMarker>,
);

type GlyphQueryFilter = (
    With<TerminalGlyphMarker>,
    Without<TerminalBackgroundMarker>,
    Without<TerminalCursorMarker>,
);

type CursorQueryFilter = (
    With<TerminalCursorMarker>,
    Without<TerminalBackgroundMarker>,
    Without<TerminalGlyphMarker>,
);

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct TerminalPlaneQueries<'w, 's> {
    bg_query: Query<'w, 's, BackgroundQueryItem<'static>, BackgroundQueryFilter>,
    glyph_query: Query<'w, 's, GlyphQueryItem<'static>, GlyphQueryFilter>,
    cursor_query: Query<
        'w,
        's,
        (
            &'static mut Sprite,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        CursorQueryFilter,
    >,
}

#[derive(Clone, Copy)]
struct ProjectedBasis {
    origin: Vec2,
    horizontal: Vec2,
    vertical: Vec2,
}

fn terminal_worker(
    input_rx: Receiver<TerminalCommand>,
    snapshot_tx: Sender<TerminalSnapshot>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
    event_loop_proxy: EventLoopProxy<WinitUserEvent>,
) {
    let PtySession {
        master,
        writer,
        mut child,
    } = match spawn_pty(DEFAULT_COLS, DEFAULT_ROWS) {
        Ok(session) => session,
        Err(error) => {
            let status = format!("failed to start PTY backend: {error}");
            let _ = snapshot_tx.send(TerminalSnapshot {
                surface: None,
                status: status.clone(),
            });
            set_terminal_error(&debug_stats, status);
            return;
        }
    };
    append_debug_log("pty spawned successfully");

    let mut reader = match master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let status = format!("failed to attach PTY reader: {error}");
            let _ = snapshot_tx.send(TerminalSnapshot {
                surface: None,
                status: status.clone(),
            });
            set_terminal_error(&debug_stats, status);
            let _ = child.kill();
            return;
        }
    };

    let (pty_output_tx, pty_output_rx) = mpsc::channel::<Vec<u8>>();
    let reader_state = Arc::new(Mutex::new(None::<String>));
    let worker_reader_state = reader_state.clone();
    let reader_thread = thread::spawn(move || {
        append_debug_log("pty reader thread start");
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    match worker_reader_state.lock() {
                        Ok(mut state) => *state = Some("PTY reader reached EOF".into()),
                        Err(poisoned) => {
                            *poisoned.into_inner() = Some("PTY reader reached EOF".into())
                        }
                    }
                    break;
                }
                Ok(read) => {
                    if pty_output_tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    match worker_reader_state.lock() {
                        Ok(mut state) => *state = Some(format!("PTY reader error: {error}")),
                        Err(poisoned) => {
                            *poisoned.into_inner() = Some(format!("PTY reader error: {error}"))
                        }
                    }
                    break;
                }
            }
        }
    });

    enum InputThreadEvent {
        WriteResult(Result<(), String>),
        ScrollDisplay(i32),
        Shutdown,
    }

    let (input_status_tx, input_status_rx) = mpsc::channel::<InputThreadEvent>();
    let input_debug_stats = debug_stats.clone();
    let input_thread = thread::spawn(move || {
        let mut writer = writer;
        while let Ok(command) = input_rx.recv() {
            let event = match command {
                TerminalCommand::InputText(text) => {
                    let bytes = text.into_bytes();
                    append_debug_log(format!("pty write text: {} bytes", bytes.len()));
                    let result = match write_input(&mut *writer, &bytes) {
                        Ok(()) => {
                            with_debug_stats(&input_debug_stats, |stats| {
                                stats.pty_bytes_written += bytes.len() as u64;
                            });
                            Ok(())
                        }
                        Err(error) => Err(format!("PTY write failed for text input: {error}")),
                    };
                    InputThreadEvent::WriteResult(result)
                }
                TerminalCommand::InputEvent(event) => {
                    let bytes = event.into_bytes();
                    append_debug_log(format!("pty write input event: {} bytes", bytes.len()));
                    let result = match write_input(&mut *writer, &bytes) {
                        Ok(()) => {
                            with_debug_stats(&input_debug_stats, |stats| {
                                stats.pty_bytes_written += bytes.len() as u64;
                            });
                            Ok(())
                        }
                        Err(error) => Err(format!("PTY write failed for input event: {error}")),
                    };
                    InputThreadEvent::WriteResult(result)
                }
                TerminalCommand::SendCommand(command) => {
                    let payload = format!("{command}\r");
                    let bytes = payload.into_bytes();
                    append_debug_log(format!(
                        "pty write command `{command}`: {} bytes",
                        bytes.len()
                    ));
                    let result = match write_input(&mut *writer, &bytes) {
                        Ok(()) => {
                            with_debug_stats(&input_debug_stats, |stats| {
                                stats.pty_bytes_written += bytes.len() as u64;
                            });
                            Ok(())
                        }
                        Err(error) => {
                            Err(format!("PTY write failed for command `{command}`: {error}"))
                        }
                    };
                    InputThreadEvent::WriteResult(result)
                }
                TerminalCommand::ScrollDisplay(lines) => InputThreadEvent::ScrollDisplay(lines),
                TerminalCommand::Shutdown => {
                    let _ = input_status_tx.send(InputThreadEvent::Shutdown);
                    break;
                }
            };

            if input_status_tx.send(event).is_err() {
                break;
            }
        }
    });

    let dimensions = TerminalDimensions {
        cols: usize::from(DEFAULT_COLS),
        rows: usize::from(DEFAULT_ROWS),
    };
    let config = TermConfig {
        scrolling_history: 5000,
        ..TermConfig::default()
    };
    let mut terminal = Term::new(config, &dimensions, VoidListener);
    let mut parser = ansi::Processor::<ansi::StdSyncHandler>::new();
    let mut last_snapshot = TerminalSnapshot::default();
    let mut running = true;

    while running {
        let mut received_output = false;
        match pty_output_rx.recv_timeout(Duration::from_millis(16)) {
            Ok(bytes) => {
                append_debug_log(format!("pty read: {} bytes", bytes.len()));
                with_debug_stats(&debug_stats, |stats| {
                    stats.pty_bytes_read += bytes.len() as u64;
                });
                parser.advance(&mut terminal, &bytes);
                received_output = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                send_terminal_status_snapshot(
                    &snapshot_tx,
                    &debug_stats,
                    &terminal,
                    &event_loop_proxy,
                    "PTY reader channel disconnected",
                );
                running = false;
            }
        }

        while let Ok(bytes) = pty_output_rx.try_recv() {
            append_debug_log(format!("pty read: {} bytes", bytes.len()));
            with_debug_stats(&debug_stats, |stats| {
                stats.pty_bytes_read += bytes.len() as u64;
            });
            parser.advance(&mut terminal, &bytes);
            received_output = true;
        }

        while let Ok(event) = input_status_rx.try_recv() {
            match event {
                InputThreadEvent::WriteResult(Ok(())) => {}
                InputThreadEvent::WriteResult(Err(status)) => {
                    send_terminal_status_snapshot(
                        &snapshot_tx,
                        &debug_stats,
                        &terminal,
                        &event_loop_proxy,
                        status,
                    );
                    running = false;
                }
                InputThreadEvent::ScrollDisplay(lines) => {
                    append_debug_log(format!("terminal scroll display: {lines}"));
                    terminal.scroll_display(Scroll::Delta(lines));
                    let snapshot = TerminalSnapshot {
                        surface: Some(build_surface(&terminal)),
                        status: "backend: alacritty_terminal + portable-pty".into(),
                    };
                    if snapshot != last_snapshot {
                        last_snapshot = snapshot.clone();
                        if snapshot_tx.send(snapshot).is_ok() {
                            with_debug_stats(&debug_stats, |stats| {
                                stats.snapshots_sent += 1;
                            });
                            let _ = event_loop_proxy.send_event(WinitUserEvent::WakeUp);
                        }
                    }
                }
                InputThreadEvent::Shutdown => {
                    running = false;
                }
            }
        }

        let reader_status = match reader_state.lock() {
            Ok(state) => state.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(status) = reader_status {
            send_terminal_status_snapshot(
                &snapshot_tx,
                &debug_stats,
                &terminal,
                &event_loop_proxy,
                status,
            );
            running = false;
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                send_terminal_status_snapshot(
                    &snapshot_tx,
                    &debug_stats,
                    &terminal,
                    &event_loop_proxy,
                    format!(
                        "PTY child exited: code={} signal={:?}",
                        status.exit_code(),
                        status.signal()
                    ),
                );
                running = false;
            }
            Ok(None) => {}
            Err(error) => {
                send_terminal_status_snapshot(
                    &snapshot_tx,
                    &debug_stats,
                    &terminal,
                    &event_loop_proxy,
                    format!("PTY child wait failed: {error}"),
                );
                running = false;
            }
        }

        if received_output && running {
            let snapshot = TerminalSnapshot {
                surface: Some(build_surface(&terminal)),
                status: "backend: alacritty_terminal + portable-pty".into(),
            };

            if snapshot != last_snapshot {
                last_snapshot = snapshot.clone();
                if snapshot_tx.send(snapshot).is_ok() {
                    with_debug_stats(&debug_stats, |stats| {
                        stats.snapshots_sent += 1;
                    });
                    let _ = event_loop_proxy.send_event(WinitUserEvent::WakeUp);
                }
            }
        }
    }

    let _ = child.kill();
    let _ = reader_thread.join();
    let _ = input_thread.join();
}

fn build_surface(term: &Term<VoidListener>) -> TerminalSurface {
    let content = term.renderable_content();
    let cols = term.columns();
    let rows = term.screen_lines();
    let mut surface = TerminalSurface::new(cols, rows);

    for indexed in content.display_iter {
        let x = indexed.point.column.0;
        let y_i32 = indexed.point.line.0;
        if y_i32 < 0 {
            continue;
        }
        let y = y_i32 as usize;
        if x >= cols || y >= rows {
            continue;
        }

        let mut fg = resolve_alacritty_color(indexed.cell.fg, content.colors, true);
        let mut bg = resolve_alacritty_color(indexed.cell.bg, content.colors, false);
        if indexed.cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }

        let mut text = String::new();
        if !indexed.cell.flags.contains(Flags::HIDDEN)
            && !indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            && !indexed.cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            text.push(indexed.cell.c);
            if let Some(extra) = indexed.cell.zerowidth() {
                for character in extra {
                    text.push(*character);
                }
            }
        }

        let width = if indexed.cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else if indexed
            .cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            0
        } else {
            1
        };

        surface.set_cell(
            x,
            y,
            TerminalCell {
                text,
                fg,
                bg,
                width,
            },
        );
    }

    surface.cursor = Some(TerminalCursor {
        x: content.cursor.point.column.0.min(cols.saturating_sub(1)),
        y: content.cursor.point.line.0.max(0) as usize,
        shape: map_cursor_shape(content.cursor.shape),
        visible: content.cursor.shape != CursorShape::Hidden,
        color: resolve_alacritty_color(AnsiColor::Named(NamedColor::Cursor), content.colors, true),
    });
    surface
}

fn map_cursor_shape(shape: CursorShape) -> TerminalCursorShape {
    match shape {
        CursorShape::Underline => TerminalCursorShape::Underline,
        CursorShape::Beam => TerminalCursorShape::Beam,
        CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => {
            TerminalCursorShape::Block
        }
    }
}

pub(crate) fn resolve_alacritty_color(
    color: AnsiColor,
    colors: &Colors,
    is_foreground: bool,
) -> egui::Color32 {
    let rgb = match color {
        AnsiColor::Spec(rgb) => rgb,
        AnsiColor::Indexed(index) => xterm_indexed_rgb(index),
        AnsiColor::Named(named) => match colors[named] {
            Some(rgb) => rgb,
            None => fallback_named_rgb(named, is_foreground),
        },
    };
    egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

fn fallback_named_rgb(named: NamedColor, is_foreground: bool) -> Rgb {
    match named {
        NamedColor::Black => Rgb { r: 0, g: 0, b: 0 },
        NamedColor::Red => Rgb {
            r: 204,
            g: 85,
            b: 85,
        },
        NamedColor::Green => Rgb {
            r: 85,
            g: 204,
            b: 85,
        },
        NamedColor::Yellow => Rgb {
            r: 205,
            g: 205,
            b: 85,
        },
        NamedColor::Blue => Rgb {
            r: 84,
            g: 85,
            b: 203,
        },
        NamedColor::Magenta => Rgb {
            r: 204,
            g: 85,
            b: 204,
        },
        NamedColor::Cyan => Rgb {
            r: 122,
            g: 202,
            b: 202,
        },
        NamedColor::White => Rgb {
            r: 204,
            g: 204,
            b: 204,
        },
        NamedColor::BrightBlack => Rgb {
            r: 85,
            g: 85,
            b: 85,
        },
        NamedColor::BrightRed => Rgb {
            r: 255,
            g: 85,
            b: 85,
        },
        NamedColor::BrightGreen => Rgb {
            r: 85,
            g: 255,
            b: 85,
        },
        NamedColor::BrightYellow => Rgb {
            r: 255,
            g: 255,
            b: 85,
        },
        NamedColor::BrightBlue => Rgb {
            r: 85,
            g: 85,
            b: 255,
        },
        NamedColor::BrightMagenta => Rgb {
            r: 255,
            g: 85,
            b: 255,
        },
        NamedColor::BrightCyan => Rgb {
            r: 85,
            g: 255,
            b: 255,
        },
        NamedColor::BrightWhite => Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        NamedColor::Foreground | NamedColor::BrightForeground => Rgb {
            r: 190,
            g: 190,
            b: 190,
        },
        NamedColor::Background => Rgb {
            r: 10,
            g: 10,
            b: 10,
        },
        NamedColor::Cursor => Rgb {
            r: 82,
            g: 173,
            b: 112,
        },
        NamedColor::DimBlack => Rgb {
            r: 40,
            g: 40,
            b: 40,
        },
        NamedColor::DimRed => Rgb {
            r: 120,
            g: 50,
            b: 50,
        },
        NamedColor::DimGreen => Rgb {
            r: 50,
            g: 120,
            b: 50,
        },
        NamedColor::DimYellow => Rgb {
            r: 120,
            g: 120,
            b: 50,
        },
        NamedColor::DimBlue => Rgb {
            r: 50,
            g: 50,
            b: 120,
        },
        NamedColor::DimMagenta => Rgb {
            r: 120,
            g: 50,
            b: 120,
        },
        NamedColor::DimCyan => Rgb {
            r: 50,
            g: 120,
            b: 120,
        },
        NamedColor::DimWhite | NamedColor::DimForeground => {
            if is_foreground {
                Rgb {
                    r: 120,
                    g: 120,
                    b: 120,
                }
            } else {
                Rgb {
                    r: 10,
                    g: 10,
                    b: 10,
                }
            }
        }
    }
}

pub(crate) fn xterm_indexed_rgb(index: u8) -> Rgb {
    const ANSI: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcc, 0x55, 0x55),
        (0x55, 0xcc, 0x55),
        (0xcd, 0xcd, 0x55),
        (0x54, 0x55, 0xcb),
        (0xcc, 0x55, 0xcc),
        (0x7a, 0xca, 0xca),
        (0xcc, 0xcc, 0xcc),
        (0x55, 0x55, 0x55),
        (0xff, 0x55, 0x55),
        (0x55, 0xff, 0x55),
        (0xff, 0xff, 0x55),
        (0x55, 0x55, 0xff),
        (0xff, 0x55, 0xff),
        (0x55, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];

    if index < 16 {
        let (r, g, b) = ANSI[index as usize];
        return Rgb { r, g, b };
    }

    if index < 232 {
        const RAMP6: [u8; 6] = [0, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
        let idx = index - 16;
        let blue = RAMP6[(idx % 6) as usize];
        let green = RAMP6[((idx / 6) % 6) as usize];
        let red = RAMP6[((idx / 36) % 6) as usize];
        return Rgb {
            r: red,
            g: green,
            b: blue,
        };
    }

    let grey = 0x08 + (index - 232) * 10;
    Rgb {
        r: grey,
        g: grey,
        b: grey,
    }
}

pub(crate) fn configure_terminal_fonts(
    mut font_assets: ResMut<Assets<Font>>,
    mut font_state: ResMut<TerminalFontState>,
    mut text_renderer: ResMut<TerminalTextRenderer>,
) {
    if font_state.report.is_some() {
        return;
    }

    match resolve_terminal_font_report() {
        Ok(report) => {
            match initialize_terminal_text_renderer(&report, &mut text_renderer) {
                Ok(()) => {}
                Err(error) => {
                    font_state.report = Some(Err(error));
                    return;
                }
            }

            if let Ok(primary) = load_font_handle(&mut font_assets, &report.primary.path) {
                font_state.primary_font = Some(primary);
            }

            for fallback in &report.fallbacks {
                if let Ok(handle) = load_font_handle(&mut font_assets, &fallback.path) {
                    if fallback.source.contains("private-use") {
                        font_state.private_use_font = Some(handle.clone());
                    }
                    if fallback.source.contains("emoji") {
                        font_state.emoji_font = Some(handle.clone());
                    }
                }
            }

            font_state.report = Some(Ok(report));
        }
        Err(error) => {
            font_state.report = Some(Err(error));
        }
    }
}

fn load_font_handle(font_assets: &mut Assets<Font>, path: &Path) -> Result<Handle<Font>, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read font {}: {error}", path.display()))?;
    let font = Font::try_from_bytes(bytes)
        .map_err(|error| format!("failed to parse font {}: {error}", path.display()))?;
    Ok(font_assets.add(font))
}

pub(crate) fn initialize_terminal_text_renderer(
    report: &TerminalFontReport,
    text_renderer: &mut TerminalTextRenderer,
) -> Result<(), String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    db.set_monospace_family(report.primary.family.clone());
    db.load_font_file(&report.primary.path).map_err(|error| {
        format!(
            "failed to load primary terminal font {} into text renderer: {error}",
            report.primary.path.display()
        )
    })?;

    for fallback in &report.fallbacks {
        db.load_font_file(&fallback.path).map_err(|error| {
            format!(
                "failed to load fallback terminal font {} into text renderer: {error}",
                fallback.path.display()
            )
        })?;
    }

    let locale = env::var("LANG").unwrap_or_else(|_| "en-US".to_owned());
    text_renderer.font_system = Some(CtFontSystem::new_with_locale_and_db(locale, db));
    text_renderer.swash_cache = CtSwashCache::new();
    Ok(())
}

pub(crate) fn resolve_terminal_font_report() -> Result<TerminalFontReport, String> {
    let requested_family = load_kitty_font_family()?.unwrap_or_else(|| "monospace".to_owned());
    let primary = fc_match_face(&requested_family, "kitty primary font")?;
    let mut fallbacks = Vec::new();
    let mut seen_paths = BTreeSet::from([primary.path.clone()]);

    for (query, source) in [
        (
            format!("{requested_family}:charset=F013"),
            "kitty fallback for private-use symbols",
        ),
        (
            format!("{requested_family}:charset=1F680"),
            "kitty fallback for emoji",
        ),
    ] {
        let candidate = fc_match_face(&query, source)?;
        if seen_paths.insert(candidate.path.clone()) {
            fallbacks.push(candidate);
        }
    }

    Ok(TerminalFontReport {
        requested_family,
        primary,
        fallbacks,
    })
}

#[derive(Default)]
pub(crate) struct KittyFontConfig {
    pub(crate) font_family: Option<String>,
}

fn load_kitty_font_family() -> Result<Option<String>, String> {
    let Some(config_path) = find_kitty_config_path() else {
        return Ok(None);
    };

    let mut visited = BTreeSet::new();
    let mut config = KittyFontConfig::default();
    parse_kitty_config_file(&config_path, &mut visited, &mut config)?;
    Ok(config.font_family)
}

pub(crate) fn find_kitty_config_path() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("KITTY_CONFIG_DIRECTORY") {
        let path = PathBuf::from(dir).join("kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg_config_home).join("kitty/kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let path = PathBuf::from(home).join(".config/kitty/kitty.conf");
        if path.is_file() {
            return Some(path);
        }
    }

    if let Some(xdg_config_dirs) = env::var_os("XDG_CONFIG_DIRS") {
        for base in env::split_paths(&xdg_config_dirs) {
            let path = base.join("kitty/kitty.conf");
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let system_path = PathBuf::from("/etc/xdg/kitty/kitty.conf");
    if system_path.is_file() {
        Some(system_path)
    } else {
        None
    }
}

pub(crate) fn parse_kitty_config_file(
    path: &Path,
    visited: &mut BTreeSet<PathBuf>,
    config: &mut KittyFontConfig,
) -> Result<(), String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize {}: {error}", path.display()))?;
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let content = fs::read_to_string(&canonical).map_err(|error| {
        format!(
            "failed to read kitty config {}: {error}",
            canonical.display()
        )
    })?;

    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let value = parts.collect::<Vec<_>>().join(" ");
        if value.is_empty() {
            continue;
        }

        match key {
            "include" => {
                let include = canonical
                    .parent()
                    .map(|parent| parent.join(&value))
                    .unwrap_or_else(|| PathBuf::from(&value));
                if include.is_file() {
                    parse_kitty_config_file(&include, visited, config)?;
                }
            }
            "font_family" => {
                config.font_family = Some(value);
            }
            _ => {}
        }
    }

    Ok(())
}

fn fc_match_face(query: &str, source: &str) -> Result<TerminalFontFace, String> {
    let output = Command::new("/usr/bin/fc-match")
        .arg("-f")
        .arg("%{family}\n%{file}\n")
        .arg(query)
        .output()
        .map_err(|error| format!("failed to execute fc-match for `{query}`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "fc-match failed for `{query}` with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let family = lines
        .next()
        .ok_or_else(|| format!("fc-match returned no family for `{query}`"))?
        .to_owned();
    let path = PathBuf::from(
        lines
            .next()
            .ok_or_else(|| format!("fc-match returned no path for `{query}`"))?,
    );

    if !path.is_file() {
        return Err(format!(
            "fc-match resolved `{query}` to missing file {}",
            path.display()
        ));
    }

    Ok(TerminalFontFace {
        family,
        path,
        source: source.to_owned(),
    })
}

pub(crate) fn sync_terminal_font_helpers(
    font_state: Res<TerminalFontState>,
    terminal_manager: Res<TerminalManager>,
    mut helper_fonts: Query<(&TerminalFontRole, &mut TextFont)>,
) {
    if !font_state.is_changed() || terminal_manager.helper_entities.is_none() {
        return;
    }

    let font_size = DEFAULT_CELL_HEIGHT_PX as f32 * 0.9;
    for (role, mut text_font) in &mut helper_fonts {
        text_font.font_size = font_size;
        match role {
            TerminalFontRole::Primary => {
                if let Some(handle) = &font_state.primary_font {
                    text_font.font = handle.clone();
                }
            }
            TerminalFontRole::PrivateUse => {
                if let Some(handle) = font_state
                    .private_use_font
                    .as_ref()
                    .or(font_state.primary_font.as_ref())
                {
                    text_font.font = handle.clone();
                }
            }
            TerminalFontRole::Emoji => {
                if let Some(handle) = font_state
                    .emoji_font
                    .as_ref()
                    .or(font_state.primary_font.as_ref())
                {
                    text_font.font = handle.clone();
                }
            }
        }
    }
}

pub(crate) fn sync_terminal_texture(
    mut terminal_manager: ResMut<TerminalManager>,
    font_state: Res<TerminalFontState>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut glyph_cache: ResMut<TerminalGlyphCache>,
    mut images: ResMut<Assets<Image>>,
    mut text_renderer: ResMut<TerminalTextRenderer>,
) {
    if text_renderer.font_system.is_none() {
        append_debug_log("texture sync: no font system");
        return;
    }

    if font_state.is_changed() {
        append_debug_log("texture sync: font state changed, clearing glyph cache");
        glyph_cache.glyphs.clear();
    }

    let active_id = terminal_manager.active_id;
    for (terminal_id, terminal) in terminal_manager.terminals.iter_mut() {
        let Some(surface) = &terminal.latest.surface else {
            terminal.texture_state.last_surface = None;
            continue;
        };

        let Some(image_handle) = terminal.texture_state.image.clone() else {
            append_debug_log("texture sync: missing image handle");
            continue;
        };
        let Some(helper_entities) = terminal.texture_state.helper_entities else {
            append_debug_log("texture sync: missing helper entities");
            continue;
        };

        let pixel_perfect = Some(*terminal_id) == active_id
            && terminal.display_mode == TerminalDisplayMode::PixelPerfect;
        let desired_cell_size = if pixel_perfect {
            pixel_perfect_cell_size(surface.cols, surface.rows, &primary_window)
        } else {
            UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX)
        };
        if terminal.texture_state.cell_size != desired_cell_size {
            terminal.texture_state.cell_size = desired_cell_size;
            terminal.texture_state.last_surface = None;
        }

        let cell_size = terminal.texture_state.cell_size;
        let texture_size = UVec2::new(
            surface.cols as u32 * cell_size.x.max(1),
            surface.rows as u32 * cell_size.y.max(1),
        );
        let mut full_redraw = font_state.is_changed()
            || terminal.texture_state.texture_size != texture_size
            || terminal.texture_state.last_surface.is_none();
        let mut dirty_rows = if full_redraw {
            (0..surface.rows).collect::<Vec<_>>()
        } else {
            dirty_rows_between(terminal.texture_state.last_surface.as_ref(), surface)
        };

        if dirty_rows.is_empty() {
            continue;
        }

        if let Some(target_image) = images.get_mut(&image_handle) {
            if target_image.texture_descriptor.size.width != texture_size.x
                || target_image.texture_descriptor.size.height != texture_size.y
            {
                *target_image = create_terminal_image(texture_size);
                full_redraw = true;
                dirty_rows = (0..surface.rows).collect();
            }

            if full_redraw {
                clear_terminal_image(target_image);
            }

            append_debug_log(format!(
                "texture sync: repaint rows={} cols={} size={}x{}",
                dirty_rows.len(),
                surface.cols,
                texture_size.x,
                texture_size.y
            ));
            repaint_terminal_rows(
                target_image,
                surface,
                &dirty_rows,
                cell_size,
                helper_entities,
                &mut text_renderer,
                &mut glyph_cache,
                &font_state,
            );
            if env::var_os("NEOZEUS_DUMP_TEXTURE").is_some() {
                let _ = dump_terminal_image_ppm(target_image, Path::new(DEBUG_TEXTURE_DUMP_PATH));
            }
            terminal.texture_state.texture_size = texture_size;
            terminal.texture_state.last_surface = Some(surface.clone());
        } else {
            append_debug_log("texture sync: target image missing in assets");
        }
    }
}

fn dirty_rows_between(
    previous_surface: Option<&TerminalSurface>,
    surface: &TerminalSurface,
) -> Vec<usize> {
    let Some(previous_surface) = previous_surface else {
        return (0..surface.rows).collect();
    };
    if previous_surface.cols != surface.cols || previous_surface.rows != surface.rows {
        return (0..surface.rows).collect();
    }

    let mut dirty_rows = BTreeSet::new();
    for y in 0..surface.rows {
        let start = y * surface.cols;
        let end = start + surface.cols;
        if previous_surface.cells[start..end] != surface.cells[start..end] {
            dirty_rows.insert(y);
        }
    }

    if previous_surface.cursor != surface.cursor {
        if let Some(cursor) = previous_surface.cursor.as_ref() {
            if cursor.visible && cursor.y < surface.rows {
                dirty_rows.insert(cursor.y);
            }
        }
        if let Some(cursor) = surface.cursor.as_ref() {
            if cursor.visible && cursor.y < surface.rows {
                dirty_rows.insert(cursor.y);
            }
        }
    }

    dirty_rows.into_iter().collect()
}

fn clear_terminal_image(image: &mut Image) {
    image.clear(&[
        DEFAULT_BG.r(),
        DEFAULT_BG.g(),
        DEFAULT_BG.b(),
        DEFAULT_BG.a(),
    ]);
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal row repaint needs renderer/cache/font state together"
)]
fn repaint_terminal_rows(
    image: &mut Image,
    surface: &TerminalSurface,
    rows: &[usize],
    cell_size: UVec2,
    helper_entities: TerminalFontEntities,
    text_renderer: &mut TerminalTextRenderer,
    glyph_cache: &mut TerminalGlyphCache,
    font_state: &TerminalFontState,
) {
    for &y in rows {
        if y >= surface.rows {
            continue;
        }

        for x in 0..surface.cols {
            let cell = surface.cell(x, y);
            let origin_x = x as u32 * cell_size.x;
            let origin_y = y as u32 * cell_size.y;
            fill_rect(image, origin_x, origin_y, cell_size.x, cell_size.y, cell.bg);

            if cell.width == 0 || cell.text.is_empty() {
                continue;
            }

            let (font_role, _helper_entity, preserve_color) =
                select_terminal_font_role(&cell.text, font_state, helper_entities);
            let cache_key = TerminalGlyphCacheKey {
                text: cell.text.clone(),
                font_role,
                width_cells: cell.width,
                cell_width: cell_size.x,
                cell_height: cell_size.y,
            };

            if !glyph_cache.glyphs.contains_key(&cache_key) {
                let glyph = rasterize_terminal_glyph(
                    &cache_key,
                    font_role,
                    preserve_color,
                    text_renderer,
                    font_state,
                );
                glyph_cache.glyphs.insert(cache_key.clone(), glyph);
            }

            if let Some(glyph) = glyph_cache.glyphs.get(&cache_key) {
                blit_cached_glyph(image, origin_x, origin_y, glyph, cell.fg);
            }
        }
    }

    if let Some(cursor) = &surface.cursor {
        if cursor.visible && rows.binary_search(&cursor.y).is_ok() {
            draw_cursor(image, cursor, cell_size);
        }
    }
}

fn select_terminal_font_role(
    text: &str,
    font_state: &TerminalFontState,
    helper_entities: TerminalFontEntities,
) -> (TerminalFontRole, Entity, bool) {
    if text.chars().any(is_emoji_like) && font_state.emoji_font.is_some() {
        return (TerminalFontRole::Emoji, helper_entities.emoji, true);
    }

    if text.chars().any(is_private_use_like) && font_state.private_use_font.is_some() {
        return (
            TerminalFontRole::PrivateUse,
            helper_entities.private_use,
            false,
        );
    }

    (TerminalFontRole::Primary, helper_entities.primary, false)
}

fn terminal_text_attrs<'a>(
    font_role: TerminalFontRole,
    font_state: &'a TerminalFontState,
) -> CtAttrs<'a> {
    let family = match font_role {
        TerminalFontRole::Primary => CtFamily::Monospace,
        TerminalFontRole::PrivateUse => terminal_font_family_name(font_state, "private-use")
            .map(CtFamily::Name)
            .unwrap_or(CtFamily::Monospace),
        TerminalFontRole::Emoji => terminal_font_family_name(font_state, "emoji")
            .map(CtFamily::Name)
            .unwrap_or(CtFamily::Monospace),
    };
    CtAttrs::new().family(family)
}

fn terminal_font_family_name<'a>(
    font_state: &'a TerminalFontState,
    needle: &str,
) -> Option<&'a str> {
    let report = font_state.report.as_ref()?.as_ref().ok()?;
    report
        .fallbacks
        .iter()
        .find(|face| face.source.contains(needle))
        .map(|face| face.family.as_str())
}

pub(crate) fn rasterize_terminal_glyph(
    cache_key: &TerminalGlyphCacheKey,
    font_role: TerminalFontRole,
    preserve_color: bool,
    text_renderer: &mut TerminalTextRenderer,
    font_state: &TerminalFontState,
) -> CachedTerminalGlyph {
    let width = cache_key.cell_width * u32::from(cache_key.width_cells.max(1));
    let height = cache_key.cell_height.max(1);
    let mut pixels = vec![0; (width * height * 4) as usize];

    let Some(font_system) = text_renderer.font_system.as_mut() else {
        return CachedTerminalGlyph {
            width,
            height,
            pixels,
            preserve_color,
        };
    };

    let metrics = CtMetrics::new(height as f32 * 0.9, height as f32);
    let mut buffer = CtBuffer::new_empty(metrics);
    {
        let mut borrowed = buffer.borrow_with(font_system);
        borrowed.set_size(Some(width as f32), Some(height as f32));
        let attrs = terminal_text_attrs(font_role, font_state).metrics(metrics);
        borrowed.set_text(cache_key.text.as_str(), &attrs, CtShaping::Advanced, None);
        borrowed.shape_until_scroll(false);
    }

    let base_color = CtColor::rgb(0xFF, 0xFF, 0xFF);
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, run.line_y), 1.0);
            text_renderer.swash_cache.with_pixels(
                font_system,
                physical.cache_key,
                base_color,
                |x, y, color| {
                    let rgba = color.as_rgba();
                    let source = if preserve_color {
                        rgba
                    } else {
                        [255, 255, 255, rgba[3]]
                    };
                    let target_x = physical.x + x;
                    let target_y = physical.y + y;
                    if target_x < 0
                        || target_y < 0
                        || target_x >= width as i32
                        || target_y >= height as i32
                    {
                        return;
                    }
                    blend_over_pixel(&mut pixels, width, target_x as u32, target_y as u32, source);
                },
            );
        }
    }

    CachedTerminalGlyph {
        width,
        height,
        pixels,
        preserve_color,
    }
}

fn blit_cached_glyph(
    image: &mut Image,
    origin_x: u32,
    origin_y: u32,
    glyph: &CachedTerminalGlyph,
    fg: egui::Color32,
) {
    for y in 0..glyph.height {
        for x in 0..glyph.width {
            let index = ((y * glyph.width + x) * 4) as usize;
            let pixel = &glyph.pixels[index..index + 4];
            if pixel[3] == 0 {
                continue;
            }

            let source = if glyph.preserve_color {
                [pixel[0], pixel[1], pixel[2], pixel[3]]
            } else {
                [fg.r(), fg.g(), fg.b(), pixel[3]]
            };
            blend_image_pixel(image, origin_x + x, origin_y + y, source);
        }
    }
}

fn fill_rect(image: &mut Image, x: u32, y: u32, width: u32, height: u32, color: egui::Color32) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            if let Some(pixel) = image.pixel_bytes_mut(UVec3::new(col, row, 0)) {
                pixel.copy_from_slice(&[color.r(), color.g(), color.b(), color.a()]);
            }
        }
    }
}

fn draw_cursor(image: &mut Image, cursor: &TerminalCursor, cell_size: UVec2) {
    let origin_x = cursor.x as u32 * cell_size.x;
    let origin_y = cursor.y as u32 * cell_size.y;
    let color = [cursor.color.r(), cursor.color.g(), cursor.color.b(), 160];

    match cursor.shape {
        TerminalCursorShape::Block => {
            fill_alpha_rect(image, origin_x, origin_y, cell_size.x, cell_size.y, color);
        }
        TerminalCursorShape::Underline => {
            let height = (cell_size.y / 8).max(1);
            fill_alpha_rect(
                image,
                origin_x,
                origin_y + cell_size.y.saturating_sub(height),
                cell_size.x,
                height,
                [cursor.color.r(), cursor.color.g(), cursor.color.b(), 255],
            );
        }
        TerminalCursorShape::Beam => {
            let width = (cell_size.x / 10).max(1);
            fill_alpha_rect(
                image,
                origin_x,
                origin_y,
                width,
                cell_size.y,
                [cursor.color.r(), cursor.color.g(), cursor.color.b(), 255],
            );
        }
    }
}

fn fill_alpha_rect(image: &mut Image, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            blend_image_pixel(image, col, row, color);
        }
    }
}

fn blend_image_pixel(image: &mut Image, x: u32, y: u32, source: [u8; 4]) {
    let Some(pixel) = image.pixel_bytes_mut(UVec3::new(x, y, 0)) else {
        return;
    };
    blend_rgba_in_place(pixel, source);
}

fn blend_over_pixel(buffer: &mut [u8], width: u32, x: u32, y: u32, source: [u8; 4]) {
    let index = ((y * width + x) * 4) as usize;
    blend_rgba_in_place(&mut buffer[index..index + 4], source);
}

pub(crate) fn blend_rgba_in_place(dst: &mut [u8], source: [u8; 4]) {
    let src_alpha = source[3] as f32 / 255.0;
    let dst_alpha = dst[3] as f32 / 255.0;
    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);

    if out_alpha <= f32::EPSILON {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }

    for channel in 0..3 {
        let src = source[channel] as f32 / 255.0;
        let dst_value = dst[channel] as f32 / 255.0;
        let out = (src * src_alpha + dst_value * dst_alpha * (1.0 - src_alpha)) / out_alpha;
        dst[channel] = (out * 255.0).round() as u8;
    }

    dst[3] = (out_alpha * 255.0).round() as u8;
}

const HUD_TOP_RESERVED: f32 = 120.0;
const HUD_BOTTOM_RESERVED: f32 = TERMINAL_MARGIN;

pub(crate) fn pixel_perfect_cell_size(cols: usize, rows: usize, window: &Window) -> UVec2 {
    let base_texture_width = (cols as u32).max(1) as f32 * DEFAULT_CELL_WIDTH_PX as f32;
    let base_texture_height = (rows as u32).max(1) as f32 * DEFAULT_CELL_HEIGHT_PX as f32;
    let fit_width = (window.width() - TERMINAL_MARGIN * 2.0).max(64.0);
    let fit_height = (window.height() - HUD_TOP_RESERVED - HUD_BOTTOM_RESERVED).max(64.0);
    let raster_scale = (fit_width / base_texture_width)
        .min(fit_height / base_texture_height)
        .max(1.0 / DEFAULT_CELL_HEIGHT_PX as f32);

    UVec2::new(
        (DEFAULT_CELL_WIDTH_PX as f32 * raster_scale)
            .floor()
            .max(1.0) as u32,
        (DEFAULT_CELL_HEIGHT_PX as f32 * raster_scale)
            .floor()
            .max(1.0) as u32,
    )
}

pub(crate) fn snap_to_pixel_grid(position: Vec2, window: &Window) -> Vec2 {
    let scale_factor = window.scale_factor();
    if scale_factor <= f32::EPSILON {
        return position.round();
    }
    (position * scale_factor).round() / scale_factor
}

fn smooth_terminal_screen_size(
    texture_state: &TerminalTextureState,
    plane_state: &TerminalPlaneState,
    window: &Window,
) -> Vec2 {
    let texture_width = texture_state.texture_size.x.max(1) as f32;
    let texture_height = texture_state.texture_size.y.max(1) as f32;
    let fit_width = (window.width() - TERMINAL_MARGIN * 2.0).max(64.0);
    let fit_height = (window.height() - TERMINAL_MARGIN * 2.0).max(64.0);
    let fit_scale = (fit_width / texture_width).min(fit_height / texture_height);
    let zoom_scale = 10.0 / plane_state.distance.max(0.1);
    Vec2::new(texture_width, texture_height) * fit_scale * zoom_scale
}

fn hud_terminal_target_position(window: &Window) -> Vec2 {
    let top = window.height() * 0.5 - HUD_TOP_RESERVED;
    let bottom = -window.height() * 0.5 + HUD_BOTTOM_RESERVED;
    snap_to_pixel_grid(Vec2::new(0.0, (top + bottom) * 0.5), window)
}

pub(crate) fn terminal_texture_screen_size(
    texture_state: &TerminalTextureState,
    plane_state: &TerminalPlaneState,
    window: &Window,
    pixel_perfect: bool,
) -> Vec2 {
    if pixel_perfect {
        return Vec2::new(
            texture_state.texture_size.x.max(1) as f32,
            texture_state.texture_size.y.max(1) as f32,
        );
    }

    smooth_terminal_screen_size(texture_state, plane_state, window)
}

pub(crate) fn sync_terminal_plane_transform(
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    plane_state: Res<TerminalPlaneState>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut panels: Query<(
        &TerminalPanel,
        &mut TerminalPresentation,
        &mut Transform,
        &mut Sprite,
        &mut Visibility,
    )>,
) {
    let active_id = terminal_manager.active_id;
    let background_ids = terminal_manager
        .order
        .iter()
        .copied()
        .filter(|id| Some(*id) != active_id)
        .collect::<Vec<_>>();
    let blend = 1.0 - (-time.delta_secs() * 10.0).exp();

    for (panel, mut presentation, mut transform, mut sprite, mut visibility) in &mut panels {
        let Some(terminal) = terminal_manager.terminals.get(&panel.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if terminal.latest.surface.is_none() {
            *visibility = Visibility::Hidden;
            continue;
        }

        let smooth_size =
            smooth_terminal_screen_size(&terminal.texture_state, &plane_state, &primary_window);
        let hud_size = Vec2::new(
            terminal.texture_state.texture_size.x.max(1) as f32,
            terminal.texture_state.texture_size.y.max(1) as f32,
        );
        let pixel_perfect = Some(panel.id) == active_id
            && terminal.display_mode == TerminalDisplayMode::PixelPerfect;
        let background_rank = background_ids
            .iter()
            .position(|id| *id == panel.id)
            .unwrap_or_default() as f32;

        if Some(panel.id) == active_id {
            presentation.target_alpha = 1.0;
            if pixel_perfect {
                presentation.target_position = hud_terminal_target_position(&primary_window);
                presentation.target_size = hud_size;
                presentation.target_z = 3.0;
            } else {
                presentation.target_position = plane_state.offset;
                presentation.target_size = smooth_size;
                presentation.target_z = 0.3;
            }
        } else {
            presentation.target_position = plane_state.offset + presentation.home_position;
            presentation.target_size = smooth_size * 0.62;
            presentation.target_alpha = 0.84;
            presentation.target_z = -0.05 - background_rank * 0.02;
        }

        presentation.current_position = presentation
            .current_position
            .lerp(presentation.target_position, blend);
        presentation.current_size = presentation
            .current_size
            .lerp(presentation.target_size, blend);
        presentation.current_alpha +=
            (presentation.target_alpha - presentation.current_alpha) * blend;
        presentation.current_z += (presentation.target_z - presentation.current_z) * blend;

        if pixel_perfect {
            if presentation
                .current_position
                .distance(presentation.target_position)
                < 0.75
            {
                presentation.current_position = presentation.target_position;
            }
            if presentation.current_size.distance(presentation.target_size) < 0.75 {
                presentation.current_size = presentation.target_size;
            }
        }

        *visibility = Visibility::Visible;
        sprite.custom_size = Some(presentation.current_size.max(Vec2::ONE));
        sprite.color = Color::srgba(1.0, 1.0, 1.0, presentation.current_alpha);
        transform.translation = presentation.current_position.extend(presentation.current_z);
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "legacy per-cell renderer kept temporarily while texture renderer stabilizes"
)]
pub(crate) fn sync_terminal_plane(
    texture_state: Option<Res<TerminalTextureState>>,
    mut commands: Commands,
    view: Res<TerminalView>,
    window: Single<&Window, With<PrimaryWindow>>,
    plane_state: Res<TerminalPlaneState>,
    mut scene_state: ResMut<TerminalSceneState>,
    font_state: Res<TerminalFontState>,
    mut queries: TerminalPlaneQueries,
) {
    if texture_state.is_some() {
        return;
    }

    let Some(surface) = &view.latest.surface else {
        for (_, _, _, _, mut visibility) in &mut queries.bg_query {
            *visibility = Visibility::Hidden;
        }
        for (_, _, _, _, _, _, _, mut visibility) in &mut queries.glyph_query {
            *visibility = Visibility::Hidden;
        }
        for (_, _, mut visibility) in &mut queries.cursor_query {
            *visibility = Visibility::Hidden;
        }
        scene_state.last_surface = None;
        scene_state.last_layout_key = None;
        return;
    };

    if !scene_state.initialized
        || scene_state.cols != surface.cols
        || scene_state.rows != surface.rows
    {
        for (entity, _, _, _, _) in &mut queries.bg_query {
            commands.entity(entity).despawn();
        }
        for (entity, _, _, _, _, _, _, _) in &mut queries.glyph_query {
            commands.entity(entity).despawn();
        }

        for y in 0..surface.rows {
            for x in 0..surface.cols {
                commands.spawn((
                    Sprite::from_color(Color::BLACK, Vec2::ONE),
                    Anchor::TOP_LEFT,
                    Transform::from_xyz(0.0, 0.0, BG_Z),
                    TerminalCellIndex { x, y },
                    TerminalBackgroundMarker,
                ));
                commands.spawn((
                    Text2d::new(""),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    TextBounds::UNBOUNDED,
                    Anchor::TOP_LEFT,
                    Transform::from_xyz(0.0, 0.0, TEXT_Z),
                    TerminalCellIndex { x, y },
                    TerminalGlyphMarker,
                ));
            }
        }

        if queries.cursor_query.is_empty() {
            commands.spawn((
                Sprite::from_color(Color::WHITE, Vec2::ONE),
                Anchor::TOP_LEFT,
                Transform::from_xyz(0.0, 0.0, CURSOR_Z),
                TerminalCursorMarker,
            ));
        }

        scene_state.cols = surface.cols;
        scene_state.rows = surface.rows;
        scene_state.initialized = true;
        scene_state.last_surface = None;
        scene_state.last_layout_key = None;
        return;
    }

    let layout = compute_plane_layout(surface, *window, &plane_state);
    let layout_key = PlaneLayoutKey::from_state(surface, *window, &plane_state);
    let previous_surface = scene_state.last_surface.as_ref();
    let projection_changed = scene_state.last_layout_key.as_ref() != Some(&layout_key);
    let surface_changed = previous_surface != Some(surface);
    let font_changed = font_state.is_changed();

    if !projection_changed && !surface_changed && !font_changed {
        return;
    }

    for (_, index, mut sprite, mut transform, mut visibility) in &mut queries.bg_query {
        let cell = surface.cell(index.x, index.y);
        if !projection_changed && !background_changed(previous_surface, cell, index.x, index.y) {
            continue;
        }

        if let Some(projected) =
            project_cell(index.x, index.y, cell.width.max(1), &layout, &plane_state)
        {
            *visibility = Visibility::Visible;
            apply_projected_sprite(&mut sprite, &mut transform, projected, BG_Z);
            sprite.color = color32_to_bevy(cell.bg);
        } else {
            *visibility = Visibility::Hidden;
        }
    }

    for (_, index, mut text, mut font, mut color, mut bounds, mut transform, mut visibility) in
        &mut queries.glyph_query
    {
        let cell = surface.cell(index.x, index.y);
        let cell_changed = glyph_changed(previous_surface, cell, index.x, index.y);
        if !projection_changed && !cell_changed && !font_changed {
            continue;
        }

        if cell.width == 0 || cell.text.is_empty() {
            *visibility = Visibility::Hidden;
            continue;
        }

        if let Some(projected) = project_cell(index.x, index.y, cell.width, &layout, &plane_state) {
            *visibility = Visibility::Visible;
            if cell_changed {
                *text = Text2d::new(cell.text.clone());
            }
            font.font_size = projected.vertical.length().max(1.0) * 0.9;
            if let Some(handle) = select_font_handle(&cell.text, &font_state) {
                font.font = handle;
            }
            color.0 = color32_to_bevy(cell.fg);
            bounds.width = Some(projected.horizontal.length().max(1.0) * f32::from(cell.width));
            bounds.height = Some(projected.vertical.length().max(1.0) * 1.2);
            apply_projected_text(&mut transform, projected, TEXT_Z);
        } else {
            *visibility = Visibility::Hidden;
        }
    }

    let cursor_changed =
        previous_surface.and_then(|previous| previous.cursor.as_ref()) != surface.cursor.as_ref();
    if projection_changed || cursor_changed {
        if let Ok((mut sprite, mut transform, mut visibility)) = queries.cursor_query.single_mut() {
            if let Some(cursor) = &surface.cursor {
                if cursor.visible {
                    if let Some(projected) =
                        project_cell(cursor.x, cursor.y, 1, &layout, &plane_state)
                    {
                        *visibility = Visibility::Visible;
                        apply_projected_cursor(
                            &mut sprite,
                            &mut transform,
                            projected,
                            cursor.shape,
                            color32_to_bevy(cursor.color),
                        );
                    } else {
                        *visibility = Visibility::Hidden;
                    }
                } else {
                    *visibility = Visibility::Hidden;
                }
            } else {
                *visibility = Visibility::Hidden;
            }
        }
    }

    scene_state.last_surface = Some(surface.clone());
    scene_state.last_layout_key = Some(layout_key);
}

fn compute_plane_layout(
    surface: &TerminalSurface,
    window: &Window,
    plane_state: &TerminalPlaneState,
) -> PlaneLayout {
    let usable_w = (window.width() - TERMINAL_MARGIN * 2.0).max(100.0);
    let usable_h = (window.height() - TERMINAL_MARGIN * 2.0).max(100.0);
    let cell_h = (usable_h / surface.rows as f32)
        .min(usable_w / (surface.cols as f32 * BASE_CELL_ASPECT))
        .max(4.0);
    let cell_w = cell_h * BASE_CELL_ASPECT;
    let plane_w = cell_w * surface.cols as f32;
    let plane_h = cell_h * surface.rows as f32;
    let distance = plane_state.distance.max(plane_h * 1.25);
    let focal_length = plane_state.focal_length.max(distance * 0.8);

    PlaneLayout {
        cell_w,
        cell_h,
        plane_w,
        plane_h,
        distance,
        focal_length,
    }
}

struct PlaneLayout {
    cell_w: f32,
    cell_h: f32,
    plane_w: f32,
    plane_h: f32,
    pub(crate) distance: f32,
    pub(crate) focal_length: f32,
}

#[derive(Clone, PartialEq)]
struct PlaneLayoutKey {
    window_width: f32,
    window_height: f32,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) distance: f32,
    pub(crate) focal_length: f32,
    cols: usize,
    rows: usize,
}

impl PlaneLayoutKey {
    fn from_state(
        surface: &TerminalSurface,
        window: &Window,
        plane_state: &TerminalPlaneState,
    ) -> Self {
        Self {
            window_width: window.width(),
            window_height: window.height(),
            yaw: plane_state.yaw,
            pitch: plane_state.pitch,
            distance: plane_state.distance,
            focal_length: plane_state.focal_length,
            cols: surface.cols,
            rows: surface.rows,
        }
    }
}

fn background_changed(
    previous_surface: Option<&TerminalSurface>,
    cell: &TerminalCell,
    x: usize,
    y: usize,
) -> bool {
    let Some(previous_surface) = previous_surface else {
        return true;
    };
    let previous = previous_surface.cell(x, y);
    previous.bg != cell.bg || previous.width != cell.width
}

fn glyph_changed(
    previous_surface: Option<&TerminalSurface>,
    cell: &TerminalCell,
    x: usize,
    y: usize,
) -> bool {
    let Some(previous_surface) = previous_surface else {
        return true;
    };
    let previous = previous_surface.cell(x, y);
    previous.text != cell.text || previous.fg != cell.fg || previous.width != cell.width
}

fn project_cell(
    x: usize,
    y: usize,
    width_cells: u8,
    layout: &PlaneLayout,
    plane_state: &TerminalPlaneState,
) -> Option<ProjectedBasis> {
    let left = -layout.plane_w * 0.5;
    let top = layout.plane_h * 0.5;
    let origin_local = Vec3::new(
        left + x as f32 * layout.cell_w,
        top - y as f32 * layout.cell_h,
        0.0,
    );
    let right_local = origin_local + Vec3::new(layout.cell_w * f32::from(width_cells), 0.0, 0.0);
    let bottom_local = origin_local + Vec3::new(0.0, -layout.cell_h, 0.0);

    let origin = project_point(origin_local, layout, plane_state)?;
    let right = project_point(right_local, layout, plane_state)?;
    let bottom = project_point(bottom_local, layout, plane_state)?;

    Some(ProjectedBasis {
        origin,
        horizontal: right - origin,
        vertical: bottom - origin,
    })
}

fn project_point(
    point: Vec3,
    layout: &PlaneLayout,
    plane_state: &TerminalPlaneState,
) -> Option<Vec2> {
    let rotated =
        Quat::from_rotation_y(plane_state.yaw) * Quat::from_rotation_x(plane_state.pitch) * point;
    let z = rotated.z + layout.distance;
    if z <= 1.0 {
        return None;
    }

    let scale = layout.focal_length / z;
    Some(Vec2::new(rotated.x * scale, rotated.y * scale))
}

fn apply_projected_sprite(
    sprite: &mut Sprite,
    transform: &mut Transform,
    projected: ProjectedBasis,
    z: f32,
) {
    let width = projected.horizontal.length().max(1.0);
    let height = projected.vertical.length().max(1.0);
    sprite.custom_size = Some(Vec2::new(width, height));
    transform.translation = projected.origin.extend(z);
    transform.rotation =
        Quat::from_rotation_z(projected.horizontal.y.atan2(projected.horizontal.x));
    transform.scale = Vec3::ONE;
}

fn apply_projected_text(transform: &mut Transform, projected: ProjectedBasis, z: f32) {
    let inset = projected.horizontal * 0.06 + projected.vertical * 0.08;
    transform.translation = (projected.origin + inset).extend(z);
    transform.rotation =
        Quat::from_rotation_z(projected.horizontal.y.atan2(projected.horizontal.x));
    transform.scale = Vec3::ONE;
}

fn apply_projected_cursor(
    sprite: &mut Sprite,
    transform: &mut Transform,
    projected: ProjectedBasis,
    shape: TerminalCursorShape,
    color: Color,
) {
    let width = projected.horizontal.length().max(1.0);
    let height = projected.vertical.length().max(1.0);
    let origin = projected.origin;
    let angle = projected.horizontal.y.atan2(projected.horizontal.x);

    match shape {
        TerminalCursorShape::Block => {
            sprite.custom_size = Some(Vec2::new(width, height));
            sprite.color = color.with_alpha(0.35);
            transform.translation = origin.extend(CURSOR_Z);
            transform.rotation = Quat::from_rotation_z(angle);
        }
        TerminalCursorShape::Underline => {
            sprite.custom_size = Some(Vec2::new(width, height * 0.12));
            sprite.color = color;
            transform.translation = (origin + projected.vertical * 0.88).extend(CURSOR_Z);
            transform.rotation = Quat::from_rotation_z(angle);
        }
        TerminalCursorShape::Beam => {
            sprite.custom_size = Some(Vec2::new(width * 0.08, height));
            sprite.color = color;
            transform.translation = origin.extend(CURSOR_Z);
            transform.rotation = Quat::from_rotation_z(angle);
        }
    }

    transform.scale = Vec3::ONE;
}

fn select_font_handle(text: &str, font_state: &TerminalFontState) -> Option<Handle<Font>> {
    if text.chars().any(is_emoji_like) {
        if let Some(handle) = &font_state.emoji_font {
            return Some(handle.clone());
        }
    }

    if text.chars().any(is_private_use_like) {
        if let Some(handle) = &font_state.private_use_font {
            return Some(handle.clone());
        }
    }

    font_state.primary_font.clone()
}

pub(crate) fn is_private_use_like(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

pub(crate) fn is_emoji_like(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0xFE0F | 0x200D
    )
}

fn color32_to_bevy(color: egui::Color32) -> Color {
    Color::srgba_u8(color.r(), color.g(), color.b(), color.a())
}

fn spawn_pty(cols: u16, rows: u16) -> Result<PtySession, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| format!("openpty failed: {error}"))?;

    let shell = shell_path();
    let mut command = CommandBuilder::new(shell);
    command.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("spawn_command failed: {error}"))?;

    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("take_writer failed: {error}"))?;

    Ok(PtySession {
        master: pair.master,
        writer,
        child,
    })
}

fn shell_path() -> OsString {
    match env::var_os("SHELL") {
        Some(shell) => shell,
        None => OsString::from("bash"),
    }
}

fn write_input(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}

pub(crate) fn poll_terminal_snapshots(mut terminal_manager: ResMut<TerminalManager>) {
    for terminal in terminal_manager.terminals.values_mut() {
        let receiver = match terminal.bridge.snapshot_rx.lock() {
            Ok(receiver) => receiver,
            Err(poisoned) => poisoned.into_inner(),
        };

        while let Ok(snapshot) = receiver.try_recv() {
            terminal.latest = snapshot;
            terminal.bridge.note_snapshot_applied();
        }
    }
}

impl Drop for TerminalBridge {
    fn drop(&mut self) {
        let _ = self.input_tx.send(TerminalCommand::Shutdown);
    }
}
