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
    image.data = None;
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
    pending_damage: Option<TerminalDamage>,
    surface_revision: u64,
    uploaded_revision: u64,
    texture_state: TerminalTextureState,
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
    update_rx: Mutex<Receiver<TerminalUpdate>>,
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
                    cpu_pixels: vec![
                        DEFAULT_BG.r(),
                        DEFAULT_BG.g(),
                        DEFAULT_BG.b(),
                        DEFAULT_BG.a(),
                    ],
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
        let (update_tx, update_rx) = mpsc::channel();
        let debug_stats = Arc::new(Mutex::new(TerminalDebugStats::default()));
        let worker_debug_stats = debug_stats.clone();
        let worker_event_loop_proxy = event_loop_proxy.clone();

        thread::spawn(move || {
            append_debug_log("terminal worker thread spawn");
            let panic_update_tx = update_tx.clone();
            let panic_event_loop_proxy = worker_event_loop_proxy.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal_worker(
                    input_rx,
                    update_tx,
                    worker_debug_stats,
                    worker_event_loop_proxy,
                )
            }));
            if let Err(payload) = result {
                let message = panic_payload_to_string(payload);
                append_debug_log(format!("terminal worker panic: {message}"));
                let _ = panic_update_tx.send(TerminalUpdate::Status {
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
            update_rx: Mutex::new(update_rx),
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

fn send_terminal_status_update(
    update_tx: &Sender<TerminalUpdate>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    terminal: &Term<VoidListener>,
    event_loop_proxy: &EventLoopProxy<WinitUserEvent>,
    status: impl Into<String>,
) {
    let status = status.into();
    append_debug_log(format!("status snapshot: {status}"));
    if update_tx
        .send(TerminalUpdate::Status {
            surface: Some(build_surface(terminal)),
            status: status.clone(),
        })
        .is_ok()
    {
        with_debug_stats(debug_stats, |stats| {
            stats.snapshots_sent += 1;
        });
        let _ = event_loop_proxy.send_event(WinitUserEvent::WakeUp);
    }
    set_terminal_error(debug_stats, status);
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
pub(crate) struct TerminalTextureState {
    pub(crate) image: Option<Handle<Image>>,
    pub(crate) helper_entities: Option<TerminalFontEntities>,
    pub(crate) texture_size: UVec2,
    pub(crate) cell_size: UVec2,
    pub(crate) cpu_pixels: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalTextureUpload {
    pub(crate) image: Handle<Image>,
    pub(crate) origin_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bytes_per_row: u32,
    pub(crate) data: Vec<u8>,
}

#[derive(Resource, Clone, Default)]
pub(crate) struct TerminalGpuUploadQueue(Arc<Mutex<VecDeque<TerminalTextureUpload>>>);

impl TerminalGpuUploadQueue {
    fn replace_pending_for_image(
        &self,
        image: &Handle<Image>,
        uploads: impl IntoIterator<Item = TerminalTextureUpload>,
    ) {
        let Ok(mut pending) = self.0.lock() else {
            return;
        };
        pending.retain(|upload| upload.image != *image);
        pending.extend(uploads);
    }

    fn take_pending(&self) -> VecDeque<TerminalTextureUpload> {
        match self.0.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }

    fn prepend_pending(&self, uploads: VecDeque<TerminalTextureUpload>) {
        if uploads.is_empty() {
            return;
        }
        let Ok(mut pending) = self.0.lock() else {
            return;
        };
        let mut uploads = uploads;
        uploads.append(&mut pending);
        *pending = uploads;
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> Vec<TerminalTextureUpload> {
        match self.0.lock() {
            Ok(pending) => pending.iter().cloned().collect(),
            Err(poisoned) => poisoned.into_inner().iter().cloned().collect(),
        }
    }
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

#[derive(Component)]
pub(crate) struct TerminalHudSurfaceMarker;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TerminalFontRole {
    Primary,
    PrivateUse,
    Emoji,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalGlyphCacheKey {
    pub(crate) content: TerminalCellContent,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalDamage {
    #[default]
    Full,
    Rows(Vec<usize>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TerminalFrameUpdate {
    pub(crate) surface: TerminalSurface,
    pub(crate) damage: TerminalDamage,
    pub(crate) status: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TerminalUpdate {
    Frame(TerminalFrameUpdate),
    Status {
        status: String,
        surface: Option<TerminalSurface>,
    },
}

type LatestTerminalStatus = (String, Option<TerminalSurface>);
type DrainedTerminalUpdates = (
    Option<TerminalFrameUpdate>,
    Option<LatestTerminalStatus>,
    u64,
);

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

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum TerminalCellContent {
    #[default]
    Empty,
    Single(char),
    InlineSmall([char; 2], u8),
    Heap(Arc<str>),
}

impl TerminalCellContent {
    fn from_parts(base: char, extra: Option<&[char]>) -> Self {
        let Some(extra) = extra else {
            return Self::Single(base);
        };
        match extra {
            [] => Self::Single(base),
            [first] => Self::InlineSmall([base, *first], 2),
            _ => {
                let mut text = String::with_capacity(1 + extra.len());
                text.push(base);
                for character in extra {
                    text.push(*character);
                }
                Self::Heap(Arc::<str>::from(text))
            }
        }
    }

    fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    fn any_char(&self, mut predicate: impl FnMut(char) -> bool) -> bool {
        match self {
            Self::Empty => false,
            Self::Single(ch) => predicate(*ch),
            Self::InlineSmall(chars, len) => chars[..usize::from(*len)]
                .iter()
                .copied()
                .any(&mut predicate),
            Self::Heap(text) => text.chars().any(predicate),
        }
    }

    fn to_owned_string(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Single(ch) => ch.to_string(),
            Self::InlineSmall(chars, len) => chars[..usize::from(*len)].iter().collect(),
            Self::Heap(text) => text.as_ref().to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TerminalCell {
    pub(crate) content: TerminalCellContent,
    fg: egui::Color32,
    bg: egui::Color32,
    width: u8,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            content: TerminalCellContent::Empty,
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
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
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

    #[cfg(test)]
    pub(crate) fn set_text_cell(&mut self, x: usize, y: usize, text: &str) {
        let mut chars = text.chars();
        let Some(base) = chars.next() else {
            self.set_cell(x, y, TerminalCell::default());
            return;
        };
        let extra = chars.collect::<Vec<_>>();
        self.set_cell(
            x,
            y,
            TerminalCell {
                content: TerminalCellContent::from_parts(base, Some(&extra)),
                ..Default::default()
            },
        );
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

const PTY_OUTPUT_WAIT_TIMEOUT: Duration = Duration::from_millis(16);
const PTY_OUTPUT_BATCH_WINDOW: Duration = Duration::from_millis(4);
const PTY_OUTPUT_BATCH_BYTES: usize = 128 * 1024;

fn apply_pty_bytes(
    parser: &mut ansi::Processor<ansi::StdSyncHandler>,
    terminal: &mut Term<VoidListener>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    bytes: &[u8],
) {
    with_debug_stats(debug_stats, |stats| {
        stats.pty_bytes_read += bytes.len() as u64;
    });
    parser.advance(terminal, bytes);
}

pub(crate) fn compute_terminal_damage(
    previous_surface: Option<&TerminalSurface>,
    surface: &TerminalSurface,
) -> TerminalDamage {
    let Some(previous_surface) = previous_surface else {
        return TerminalDamage::Full;
    };
    if previous_surface.cols != surface.cols || previous_surface.rows != surface.rows {
        return TerminalDamage::Full;
    }

    let mut dirty_rows = Vec::new();
    for y in 0..surface.rows {
        let start = y * surface.cols;
        let end = start + surface.cols;
        if previous_surface.cells[start..end] != surface.cells[start..end] {
            dirty_rows.push(y);
        }
    }

    if previous_surface.cursor != surface.cursor {
        if let Some(cursor) = previous_surface.cursor.as_ref() {
            if cursor.visible && cursor.y < surface.rows && !dirty_rows.contains(&cursor.y) {
                dirty_rows.push(cursor.y);
            }
        }
        if let Some(cursor) = surface.cursor.as_ref() {
            if cursor.visible && cursor.y < surface.rows && !dirty_rows.contains(&cursor.y) {
                dirty_rows.push(cursor.y);
            }
        }
    }

    if dirty_rows.len() >= surface.rows {
        TerminalDamage::Full
    } else {
        dirty_rows.sort_unstable();
        TerminalDamage::Rows(dirty_rows)
    }
}

fn send_terminal_frame_update(
    update_tx: &Sender<TerminalUpdate>,
    debug_stats: &Arc<Mutex<TerminalDebugStats>>,
    event_loop_proxy: &EventLoopProxy<WinitUserEvent>,
    previous_surface: Option<&TerminalSurface>,
    surface: TerminalSurface,
    status: String,
) {
    let damage = compute_terminal_damage(previous_surface, &surface);
    if matches!(damage, TerminalDamage::Rows(ref rows) if rows.is_empty()) {
        return;
    }
    if update_tx
        .send(TerminalUpdate::Frame(TerminalFrameUpdate {
            surface,
            damage,
            status,
        }))
        .is_ok()
    {
        with_debug_stats(debug_stats, |stats| {
            stats.snapshots_sent += 1;
        });
        let _ = event_loop_proxy.send_event(WinitUserEvent::WakeUp);
    }
}

fn terminal_worker(
    input_rx: Receiver<TerminalCommand>,
    update_tx: Sender<TerminalUpdate>,
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
            let _ = update_tx.send(TerminalUpdate::Status {
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
            let _ = update_tx.send(TerminalUpdate::Status {
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
    let mut previous_surface: Option<TerminalSurface> = None;
    let mut running = true;

    while running {
        let mut received_output = false;
        let mut batched_output_bytes = 0usize;
        match pty_output_rx.recv_timeout(PTY_OUTPUT_WAIT_TIMEOUT) {
            Ok(bytes) => {
                batched_output_bytes += bytes.len();
                apply_pty_bytes(&mut parser, &mut terminal, &debug_stats, &bytes);
                received_output = true;

                let batch_deadline = std::time::Instant::now() + PTY_OUTPUT_BATCH_WINDOW;
                loop {
                    while batched_output_bytes < PTY_OUTPUT_BATCH_BYTES {
                        let Ok(bytes) = pty_output_rx.try_recv() else {
                            break;
                        };
                        batched_output_bytes += bytes.len();
                        apply_pty_bytes(&mut parser, &mut terminal, &debug_stats, &bytes);
                    }

                    if batched_output_bytes >= PTY_OUTPUT_BATCH_BYTES {
                        break;
                    }

                    let Some(remaining) =
                        batch_deadline.checked_duration_since(std::time::Instant::now())
                    else {
                        break;
                    };
                    if remaining.is_zero() {
                        break;
                    }

                    match pty_output_rx.recv_timeout(remaining) {
                        Ok(bytes) => {
                            batched_output_bytes += bytes.len();
                            apply_pty_bytes(&mut parser, &mut terminal, &debug_stats, &bytes);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            send_terminal_status_update(
                                &update_tx,
                                &debug_stats,
                                &terminal,
                                &event_loop_proxy,
                                "PTY reader channel disconnected",
                            );
                            running = false;
                            break;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                send_terminal_status_update(
                    &update_tx,
                    &debug_stats,
                    &terminal,
                    &event_loop_proxy,
                    "PTY reader channel disconnected",
                );
                running = false;
            }
        }

        while let Ok(event) = input_status_rx.try_recv() {
            match event {
                InputThreadEvent::WriteResult(Ok(())) => {}
                InputThreadEvent::WriteResult(Err(status)) => {
                    send_terminal_status_update(
                        &update_tx,
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
                    let surface = build_surface(&terminal);
                    send_terminal_frame_update(
                        &update_tx,
                        &debug_stats,
                        &event_loop_proxy,
                        previous_surface.as_ref(),
                        surface.clone(),
                        "backend: alacritty_terminal + portable-pty".into(),
                    );
                    previous_surface = Some(surface);
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
            send_terminal_status_update(
                &update_tx,
                &debug_stats,
                &terminal,
                &event_loop_proxy,
                status,
            );
            running = false;
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                send_terminal_status_update(
                    &update_tx,
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
                send_terminal_status_update(
                    &update_tx,
                    &debug_stats,
                    &terminal,
                    &event_loop_proxy,
                    format!("PTY child wait failed: {error}"),
                );
                running = false;
            }
        }

        if received_output && running {
            let surface = build_surface(&terminal);
            send_terminal_frame_update(
                &update_tx,
                &debug_stats,
                &event_loop_proxy,
                previous_surface.as_ref(),
                surface.clone(),
                "backend: alacritty_terminal + portable-pty".into(),
            );
            previous_surface = Some(surface);
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

        let content = if indexed.cell.flags.contains(Flags::HIDDEN)
            || indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            || indexed.cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            TerminalCellContent::Empty
        } else {
            TerminalCellContent::from_parts(indexed.cell.c, indexed.cell.zerowidth())
        };

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
                content,
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
    upload_queue: Res<TerminalGpuUploadQueue>,
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
            terminal.pending_damage = None;
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
        }

        let cell_size = terminal.texture_state.cell_size;
        let texture_size = UVec2::new(
            surface.cols as u32 * cell_size.x.max(1),
            surface.rows as u32 * cell_size.y.max(1),
        );
        let has_pending_surface = terminal.surface_revision != terminal.uploaded_revision;
        let mut full_redraw =
            font_state.is_changed() || terminal.texture_state.texture_size != texture_size;
        let mut dirty_rows = if full_redraw {
            (0..surface.rows).collect::<Vec<_>>()
        } else if has_pending_surface {
            match terminal
                .pending_damage
                .as_ref()
                .unwrap_or(&TerminalDamage::Full)
            {
                TerminalDamage::Full => {
                    full_redraw = true;
                    (0..surface.rows).collect::<Vec<_>>()
                }
                TerminalDamage::Rows(rows) => rows.clone(),
            }
        } else {
            Vec::new()
        };

        if dirty_rows.is_empty() {
            continue;
        }

        if let Some(target_image) = images.get_mut(&image_handle) {
            if target_image.texture_descriptor.size.width != texture_size.x
                || target_image.texture_descriptor.size.height != texture_size.y
            {
                *target_image = create_terminal_image(texture_size);
                terminal.texture_state.cpu_pixels = vec![
                    DEFAULT_BG.r(),
                    DEFAULT_BG.g(),
                    DEFAULT_BG.b(),
                    DEFAULT_BG.a(),
                ];
                terminal.texture_state.cpu_pixels.resize(
                    (texture_size.x * texture_size.y * 4) as usize,
                    DEFAULT_BG.a(),
                );
                for pixel in terminal.texture_state.cpu_pixels.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[
                        DEFAULT_BG.r(),
                        DEFAULT_BG.g(),
                        DEFAULT_BG.b(),
                        DEFAULT_BG.a(),
                    ]);
                }
                full_redraw = true;
                dirty_rows = (0..surface.rows).collect();
            }

            if terminal.texture_state.cpu_pixels.len()
                != (texture_size.x * texture_size.y * 4) as usize
            {
                terminal
                    .texture_state
                    .cpu_pixels
                    .resize((texture_size.x * texture_size.y * 4) as usize, 0);
                full_redraw = true;
                dirty_rows = (0..surface.rows).collect();
            }

            if full_redraw {
                clear_terminal_pixels(&mut terminal.texture_state.cpu_pixels);
            }

            let compose_started = std::time::Instant::now();
            repaint_terminal_pixels(
                &mut terminal.texture_state.cpu_pixels,
                texture_size.x,
                surface,
                &dirty_rows,
                cell_size,
                helper_entities,
                &mut text_renderer,
                &mut glyph_cache,
                &font_state,
            );
            let compose_elapsed = compose_started.elapsed();
            with_debug_stats(&terminal.bridge.debug_stats, |stats| {
                stats.compose_micros += compose_elapsed.as_micros() as u64;
                stats.dirty_rows_uploaded += dirty_rows.len() as u64;
            });
            queue_terminal_uploads(
                &upload_queue,
                &image_handle,
                texture_size,
                &terminal.texture_state.cpu_pixels,
                &dirty_rows,
            );
            if env::var_os("NEOZEUS_DUMP_TEXTURE").is_some() {
                target_image.data = Some(terminal.texture_state.cpu_pixels.clone());
                let _ = dump_terminal_image_ppm(target_image, Path::new(DEBUG_TEXTURE_DUMP_PATH));
                target_image.data = None;
            }
            terminal.texture_state.texture_size = texture_size;
            terminal.uploaded_revision = terminal.surface_revision;
            terminal.pending_damage = None;
        } else {
            append_debug_log("texture sync: target image missing in assets");
        }
    }
}

fn clear_terminal_pixels(buffer: &mut [u8]) {
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[
            DEFAULT_BG.r(),
            DEFAULT_BG.g(),
            DEFAULT_BG.b(),
            DEFAULT_BG.a(),
        ]);
    }
}

pub(crate) fn queue_terminal_uploads(
    upload_queue: &TerminalGpuUploadQueue,
    image: &Handle<Image>,
    texture_size: UVec2,
    pixels: &[u8],
    dirty_rows: &[usize],
) {
    if dirty_rows.is_empty() {
        return;
    }

    let bytes_per_row = texture_size.x * 4;
    let mut uploads = Vec::new();
    let mut index = 0;
    while index < dirty_rows.len() {
        let start_row = dirty_rows[index] as u32;
        let mut end_index = index + 1;
        while end_index < dirty_rows.len() && dirty_rows[end_index] == dirty_rows[end_index - 1] + 1
        {
            end_index += 1;
        }
        let end_row = dirty_rows[end_index - 1] as u32;
        let height = end_row - start_row + 1;
        let start = start_row as usize * bytes_per_row as usize;
        let end = (end_row as usize + 1) * bytes_per_row as usize;
        uploads.push(TerminalTextureUpload {
            image: image.clone(),
            origin_y: start_row,
            width: texture_size.x,
            height,
            bytes_per_row,
            data: pixels[start..end].to_vec(),
        });
        index = end_index;
    }

    upload_queue.replace_pending_for_image(image, uploads);
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal row repaint needs renderer/cache/font state together"
)]
fn repaint_terminal_pixels(
    buffer: &mut [u8],
    texture_width: u32,
    surface: &TerminalSurface,
    rows: &[usize],
    cell_size: UVec2,
    helper_entities: TerminalFontEntities,
    text_renderer: &mut TerminalTextRenderer,
    glyph_cache: &mut TerminalGlyphCache,
    font_state: &TerminalFontState,
) {
    let stride = texture_width as usize * 4;

    for &y in rows {
        if y >= surface.rows {
            continue;
        }

        for x in 0..surface.cols {
            let cell = surface.cell(x, y);
            let origin_x = x as u32 * cell_size.x;
            let origin_y = y as u32 * cell_size.y;
            fill_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                cell_size.x,
                cell_size.y,
                cell.bg,
            );

            if cell.width == 0 || cell.content.is_empty() {
                continue;
            }

            let (font_role, _helper_entity, preserve_color) =
                select_terminal_font_role(&cell.content, font_state, helper_entities);
            let cache_key = TerminalGlyphCacheKey {
                content: cell.content.clone(),
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
                blit_cached_glyph_in_buffer(buffer, stride, origin_x, origin_y, glyph, cell.fg);
            }
        }
    }

    if let Some(cursor) = &surface.cursor {
        if cursor.visible && rows.binary_search(&cursor.y).is_ok() {
            draw_cursor_in_buffer(buffer, stride, cursor, cell_size);
        }
    }
}

fn select_terminal_font_role(
    content: &TerminalCellContent,
    font_state: &TerminalFontState,
    helper_entities: TerminalFontEntities,
) -> (TerminalFontRole, Entity, bool) {
    if content.any_char(is_emoji_like) && font_state.emoji_font.is_some() {
        return (TerminalFontRole::Emoji, helper_entities.emoji, true);
    }

    if content.any_char(is_private_use_like) && font_state.private_use_font.is_some() {
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
        let text = cache_key.content.to_owned_string();
        borrowed.set_text(text.as_str(), &attrs, CtShaping::Advanced, None);
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

fn blit_cached_glyph_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    origin_x: u32,
    origin_y: u32,
    glyph: &CachedTerminalGlyph,
    fg: egui::Color32,
) {
    let max_height = buffer.len() / stride;
    for y in 0..glyph.height as usize {
        let target_y = origin_y as usize + y;
        if target_y >= max_height {
            break;
        }
        let dst_row = &mut buffer[target_y * stride..(target_y + 1) * stride];
        let src_row =
            &glyph.pixels[y * glyph.width as usize * 4..(y + 1) * glyph.width as usize * 4];
        for x in 0..glyph.width as usize {
            let src = &src_row[x * 4..x * 4 + 4];
            if src[3] == 0 {
                continue;
            }

            let source = if glyph.preserve_color {
                [src[0], src[1], src[2], src[3]]
            } else {
                [fg.r(), fg.g(), fg.b(), src[3]]
            };
            let dst_start = (origin_x as usize + x) * 4;
            if dst_start + 4 > dst_row.len() {
                break;
            }
            blend_rgba_in_place(&mut dst_row[dst_start..dst_start + 4], source);
        }
    }
}

fn fill_rect_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: egui::Color32,
) {
    let pixel = [color.r(), color.g(), color.b(), color.a()];
    let max_height = buffer.len() / stride;
    for row in y as usize..(y as usize).saturating_add(height as usize).min(max_height) {
        let row_slice = &mut buffer[row * stride..(row + 1) * stride];
        let start = x as usize * 4;
        let end = ((x + width) as usize * 4).min(row_slice.len());
        if start >= end {
            continue;
        }
        for dst in row_slice[start..end].chunks_exact_mut(4) {
            dst.copy_from_slice(&pixel);
        }
    }
}

fn draw_cursor_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    cursor: &TerminalCursor,
    cell_size: UVec2,
) {
    let origin_x = cursor.x as u32 * cell_size.x;
    let origin_y = cursor.y as u32 * cell_size.y;
    let color = [cursor.color.r(), cursor.color.g(), cursor.color.b(), 160];

    match cursor.shape {
        TerminalCursorShape::Block => {
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                cell_size.x,
                cell_size.y,
                color,
            );
        }
        TerminalCursorShape::Underline => {
            let height = (cell_size.y / 8).max(1);
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y + cell_size.y.saturating_sub(height),
                cell_size.x,
                height,
                [cursor.color.r(), cursor.color.g(), cursor.color.b(), 255],
            );
        }
        TerminalCursorShape::Beam => {
            let width = (cell_size.x / 10).max(1);
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                width,
                cell_size.y,
                [cursor.color.r(), cursor.color.g(), cursor.color.b(), 255],
            );
        }
    }
}

fn fill_alpha_rect_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    let max_height = buffer.len() / stride;
    for row in y as usize..(y as usize).saturating_add(height as usize).min(max_height) {
        let row_slice = &mut buffer[row * stride..(row + 1) * stride];
        let start = x as usize * 4;
        let end = ((x + width) as usize * 4).min(row_slice.len());
        if start >= end {
            continue;
        }
        for dst in row_slice[start..end].chunks_exact_mut(4) {
            blend_rgba_in_place(dst, color);
        }
    }
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

const HUD_SIDE_RESERVED: f32 = 72.0;
const HUD_TOP_RESERVED: f32 = 140.0;
const HUD_BOTTOM_RESERVED: f32 = 64.0;
const HUD_FRAME_PADDING: Vec2 = Vec2::new(18.0, 18.0);

fn window_scale_factor(window: &Window) -> f32 {
    window.scale_factor().max(f32::EPSILON)
}

fn logical_to_physical_size(size: Vec2, window: &Window) -> Vec2 {
    size * window_scale_factor(window)
}

fn physical_to_logical_size(size: Vec2, window: &Window) -> Vec2 {
    size / window_scale_factor(window)
}

pub(crate) fn pixel_perfect_cell_size(cols: usize, rows: usize, window: &Window) -> UVec2 {
    let base_texture_width = (cols as u32).max(1) as f32 * DEFAULT_CELL_WIDTH_PX as f32;
    let base_texture_height = (rows as u32).max(1) as f32 * DEFAULT_CELL_HEIGHT_PX as f32;
    let fit_size_physical = logical_to_physical_size(
        Vec2::new(
            (window.width() - HUD_SIDE_RESERVED * 2.0 - HUD_FRAME_PADDING.x * 2.0).max(64.0),
            (window.height() - HUD_TOP_RESERVED - HUD_BOTTOM_RESERVED - HUD_FRAME_PADDING.y * 2.0)
                .max(64.0),
        ),
        window,
    );
    let raster_scale = (fit_size_physical.x / base_texture_width)
        .min(fit_size_physical.y / base_texture_height)
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
    let scale_factor = window_scale_factor(window);
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
    snap_to_pixel_grid(Vec2::new(0.0, (top + bottom) * 0.5 - 8.0), window)
}

fn hud_surface_size(terminal_size: Vec2) -> Vec2 {
    terminal_size + HUD_FRAME_PADDING * 2.0
}

pub(crate) fn pixel_perfect_terminal_logical_size(
    texture_state: &TerminalTextureState,
    window: &Window,
) -> Vec2 {
    physical_to_logical_size(
        Vec2::new(
            texture_state.texture_size.x.max(1) as f32,
            texture_state.texture_size.y.max(1) as f32,
        ),
        window,
    )
}

pub(crate) fn terminal_texture_screen_size(
    texture_state: &TerminalTextureState,
    plane_state: &TerminalPlaneState,
    window: &Window,
    pixel_perfect: bool,
) -> Vec2 {
    if pixel_perfect {
        return pixel_perfect_terminal_logical_size(texture_state, window);
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
        let hud_size =
            pixel_perfect_terminal_logical_size(&terminal.texture_state, &primary_window);
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

pub(crate) fn sync_terminal_hud_surface(
    terminal_manager: Res<TerminalManager>,
    panels: Query<(&TerminalPanel, &TerminalPresentation)>,
    mut hud_surface: Single<
        (&mut Transform, &mut Sprite, &mut Visibility),
        With<TerminalHudSurfaceMarker>,
    >,
) {
    let (transform, sprite, visibility) = &mut *hud_surface;
    let Some(active_id) = terminal_manager.active_id else {
        **visibility = Visibility::Hidden;
        return;
    };
    let Some(terminal) = terminal_manager.terminals.get(&active_id) else {
        **visibility = Visibility::Hidden;
        return;
    };
    if terminal.display_mode != TerminalDisplayMode::PixelPerfect {
        **visibility = Visibility::Hidden;
        return;
    }
    let Some((_, presentation)) = panels.iter().find(|(panel, _)| panel.id == active_id) else {
        **visibility = Visibility::Hidden;
        return;
    };

    **visibility = Visibility::Visible;
    sprite.custom_size = Some(hud_surface_size(presentation.current_size));
    sprite.color = Color::srgba(0.03, 0.03, 0.04, 0.94 * presentation.current_alpha);
    transform.translation = presentation
        .current_position
        .extend(presentation.current_z - 0.1);
    transform.rotation = Quat::IDENTITY;
    transform.scale = Vec3::ONE;
}

pub(crate) fn flush_terminal_gpu_uploads(
    upload_queue: Res<TerminalGpuUploadQueue>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_queue: Res<RenderQueue>,
) {
    let mut pending = upload_queue.take_pending();
    let mut deferred = VecDeque::new();

    while let Some(upload) = pending.pop_front() {
        let Some(gpu_image) = gpu_images.get(&upload.image) else {
            deferred.push_back(upload);
            continue;
        };
        render_queue.write_texture(
            TexelCopyTextureInfo {
                texture: &gpu_image.texture,
                mip_level: 0,
                origin: Origin3d {
                    x: 0,
                    y: upload.origin_y,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            &upload.data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(upload.bytes_per_row),
                rows_per_image: None,
            },
            Extent3d {
                width: upload.width,
                height: upload.height,
                depth_or_array_layers: 1,
            },
        );
    }

    upload_queue.prepend_pending(deferred);
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

pub(crate) fn drain_terminal_updates(
    receiver: &Receiver<TerminalUpdate>,
) -> DrainedTerminalUpdates {
    let mut latest_frame = None;
    let mut latest_status = None;
    let mut dropped_frames = 0u64;
    while let Ok(update) = receiver.try_recv() {
        match update {
            TerminalUpdate::Frame(frame) => {
                if latest_frame.is_some() {
                    dropped_frames += 1;
                }
                latest_frame = Some(frame);
            }
            TerminalUpdate::Status { status, surface } => {
                latest_status = Some((status, surface));
            }
        }
    }
    (latest_frame, latest_status, dropped_frames)
}

pub(crate) fn poll_terminal_snapshots(mut terminal_manager: ResMut<TerminalManager>) {
    for terminal in terminal_manager.terminals.values_mut() {
        let receiver = match terminal.bridge.update_rx.lock() {
            Ok(receiver) => receiver,
            Err(poisoned) => poisoned.into_inner(),
        };

        let (latest_frame, latest_status, dropped_frames) = drain_terminal_updates(&receiver);
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
