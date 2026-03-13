use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    term::{cell::Flags, color::Colors, Config as TermConfig, Term},
    vte::ansi::{self, Color as AnsiColor, CursorShape, NamedColor, Rgb},
};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    input::{
        keyboard::KeyboardInput,
        mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
        ButtonState,
    },
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        settings::WgpuSettings,
        RenderPlugin,
    },
    sprite::Anchor,
    text::{
        ComputedTextBlock, CosmicFontSystem, Font, FontAtlasSet, FontHinting, Justify, LineBreak,
        LineHeight, SwashCache, TextBounds, TextColor, TextFont, TextLayoutInfo, TextPipeline,
    },
    window::PrimaryWindow,
};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    any::Any,
    collections::{BTreeSet, HashMap},
    env,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 38;
const DEFAULT_BG: egui::Color32 = egui::Color32::from_rgb(10, 10, 10);
const BASE_CELL_ASPECT: f32 = 0.6;
const TERMINAL_MARGIN: f32 = 48.0;
const CURSOR_Z: f32 = 2.0;
const TEXT_Z: f32 = 1.0;
const BG_Z: f32 = 0.0;
const DEFAULT_CELL_HEIGHT_PX: u32 = 24;
const DEFAULT_CELL_WIDTH_PX: u32 = 14;
const TERMINAL_WORLD_HEIGHT: f32 = 8.0;
const GPU_NOT_FOUND_PANIC_FRAGMENT: &str = "Unable to find a GPU!";
const DEBUG_LOG_PATH: &str = "/tmp/neozeus-debug.log";

fn main() {
    let _ = fs::write(DEBUG_LOG_PATH, "");
    append_debug_log("app start");
    match build_app() {
        Ok(mut app) => {
            let _ = app.run();
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn build_app() -> Result<App, String> {
    let mut app = App::new();
    let previous_hook = Arc::new(std::panic::take_hook());
    let forwarding_hook = previous_hook.clone();

    std::panic::set_hook(Box::new(move |info| {
        if panic_payload_message(info.payload()).is_some_and(is_missing_gpu_panic) {
            return;
        }
        (*forwarding_hook)(info);
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| configure_app(&mut app)));

    let restore_hook = previous_hook.clone();
    std::panic::set_hook(Box::new(move |info| (*restore_hook)(info)));

    match result {
        Ok(()) => Ok(app),
        Err(payload) => {
            if let Some(error) = format_startup_panic(payload.as_ref()) {
                Err(error)
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

fn configure_app(app: &mut App) {
    app.add_plugins(
        DefaultPlugins
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    force_fallback_adapter: true,
                    ..default()
                }
                .into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "neozeus".into(),
                    resolution: (1400, 900).into(),
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(EguiPlugin::default())
    .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
    .insert_resource(TerminalBridge::spawn())
    .insert_resource(TerminalView::default())
    .insert_resource(TerminalFontState::default())
    .insert_resource(TerminalPlaneState::default())
    .insert_resource(TerminalSceneState::default())
    .insert_resource(TerminalTextureState::default())
    .insert_resource(TerminalGlyphCache::default())
    .add_systems(Startup, setup_scene)
    .add_systems(
        Update,
        (
            poll_terminal_snapshots,
            configure_terminal_fonts,
            sync_terminal_font_helpers,
            sync_terminal_texture,
            drag_terminal_plane,
            zoom_terminal_plane,
            sync_terminal_plane_transform,
            sync_terminal_plane,
            forward_keyboard_input,
        )
            .chain(),
    )
    .add_systems(EguiPrimaryContextPass, ui_overlay);
}

fn format_startup_panic(payload: &(dyn Any + Send)) -> Option<String> {
    let message = panic_payload_message(payload)?;
    if !is_missing_gpu_panic(message) {
        return None;
    }

    Some(
        "neozeus failed to start: Bevy/WGPU could not find a usable graphics adapter. \
This environment is either headless or missing graphics/software-rendering drivers. \
Run it in a graphical session with a working GPU, or install a software renderer such as Mesa/llvmpipe."
            .to_owned(),
    )
}

fn is_missing_gpu_panic(message: &str) -> bool {
    message.contains(GPU_NOT_FOUND_PANIC_FRAGMENT)
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> Option<&str> {
    if let Some(message) = payload.downcast_ref::<String>() {
        Some(message.as_str())
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        Some(*message)
    } else {
        None
    }
}

fn setup_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut texture_state: ResMut<TerminalTextureState>,
) {
    let image_handle = images.add(create_terminal_image(UVec2::ONE));
    let material_handle = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        cull_mode: None,
        ..default()
    });

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        TerminalCameraMarker,
    ));

    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(material_handle.clone()),
        Transform::default(),
        TerminalPlaneMarker,
    ));

    let primary = commands
        .spawn((
            TerminalFontRole::Primary,
            TextFont {
                font_size: DEFAULT_CELL_HEIGHT_PX as f32 * 0.9,
                ..default()
            },
        ))
        .id();
    let private_use = commands
        .spawn((
            TerminalFontRole::PrivateUse,
            TextFont {
                font_size: DEFAULT_CELL_HEIGHT_PX as f32 * 0.9,
                ..default()
            },
        ))
        .id();
    let emoji = commands
        .spawn((
            TerminalFontRole::Emoji,
            TextFont {
                font_size: DEFAULT_CELL_HEIGHT_PX as f32 * 0.9,
                ..default()
            },
        ))
        .id();

    texture_state.image = Some(image_handle);
    texture_state.helper_entities = Some(TerminalFontEntities {
        primary,
        private_use,
        emoji,
    });
    texture_state.texture_size = UVec2::ONE;
    texture_state.cell_size = UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX);
}

fn create_terminal_image(size: UVec2) -> Image {
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

fn append_debug_log(message: impl AsRef<str>) {
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
struct TerminalDebugStats {
    key_events_seen: u64,
    commands_queued: u64,
    pty_bytes_written: u64,
    pty_bytes_read: u64,
    snapshots_sent: u64,
    snapshots_applied: u64,
    last_key: String,
    last_command: String,
    last_error: String,
}

#[derive(Resource)]
struct TerminalBridge {
    input_tx: Sender<TerminalCommand>,
    snapshot_rx: Mutex<Receiver<TerminalSnapshot>>,
    debug_stats: Arc<Mutex<TerminalDebugStats>>,
}

impl TerminalBridge {
    fn spawn() -> Self {
        let (input_tx, input_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let debug_stats = Arc::new(Mutex::new(TerminalDebugStats::default()));
        let worker_debug_stats = debug_stats.clone();

        thread::spawn(move || {
            append_debug_log("terminal worker thread spawn");
            let panic_snapshot_tx = snapshot_tx.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal_worker(input_rx, snapshot_tx, worker_debug_stats)
            }));
            if let Err(payload) = result {
                let message = panic_payload_to_string(payload);
                append_debug_log(format!("terminal worker panic: {message}"));
                let _ = panic_snapshot_tx.send(TerminalSnapshot {
                    surface: None,
                    status: format!("terminal worker panicked: {message}"),
                });
            }
        });

        Self {
            input_tx,
            snapshot_rx: Mutex::new(snapshot_rx),
            debug_stats,
        }
    }

    fn send(&self, command: TerminalCommand) {
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

    fn note_key_event(&self, event: &KeyboardInput) {
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

    fn note_snapshot_applied(&self) {
        with_debug_stats(&self.debug_stats, |stats| {
            stats.snapshots_applied += 1;
        });
    }

    fn debug_stats_snapshot(&self) -> TerminalDebugStats {
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
        TerminalCommand::Shutdown => "Shutdown",
    }
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
    }
    set_terminal_error(debug_stats, status);
}

#[derive(Resource, Default)]
struct TerminalView {
    latest: TerminalSnapshot,
}

#[derive(Resource, Default)]
struct TerminalFontState {
    report: Option<Result<TerminalFontReport, String>>,
    primary_font: Option<Handle<Font>>,
    private_use_font: Option<Handle<Font>>,
    emoji_font: Option<Handle<Font>>,
}

#[derive(Resource)]
struct TerminalPlaneState {
    yaw: f32,
    pitch: f32,
    distance: f32,
    focal_length: f32,
    offset: Vec2,
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
struct TerminalSceneState {
    cols: usize,
    rows: usize,
    initialized: bool,
    last_surface: Option<TerminalSurface>,
    last_layout_key: Option<PlaneLayoutKey>,
}

#[derive(Resource, Default)]
struct TerminalTextureState {
    image: Option<Handle<Image>>,
    helper_entities: Option<TerminalFontEntities>,
    texture_size: UVec2,
    cell_size: UVec2,
    last_surface: Option<TerminalSurface>,
}

#[derive(Resource, Default)]
struct TerminalGlyphCache {
    glyphs: HashMap<TerminalGlyphCacheKey, CachedTerminalGlyph>,
}

#[derive(Clone, Copy)]
struct TerminalFontEntities {
    primary: Entity,
    private_use: Entity,
    emoji: Entity,
}

#[derive(Component)]
struct TerminalPlaneMarker;

#[derive(Component)]
struct TerminalCameraMarker;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TerminalFontRole {
    Primary,
    PrivateUse,
    Emoji,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TerminalGlyphCacheKey {
    text: String,
    font_role: TerminalFontRole,
    width_cells: u8,
    cell_width: u32,
    cell_height: u32,
}

#[derive(Clone)]
struct CachedTerminalGlyph {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    preserve_color: bool,
}

#[derive(Clone, Default, PartialEq)]
struct TerminalSnapshot {
    surface: Option<TerminalSurface>,
    status: String,
}

enum TerminalCommand {
    InputText(String),
    InputEvent(String),
    SendCommand(String),
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
    text: String,
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
struct TerminalSurface {
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
struct TerminalFontFace {
    family: String,
    path: PathBuf,
    source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalFontReport {
    requested_family: String,
    primary: TerminalFontFace,
    fallbacks: Vec<TerminalFontFace>,
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
struct TerminalPlaneQueries<'w, 's> {
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

#[derive(bevy::ecs::system::SystemParam)]
struct TerminalTextureRenderParams<'w, 's> {
    images: ResMut<'w, Assets<Image>>,
    fonts: Res<'w, Assets<Font>>,
    text_pipeline: ResMut<'w, TextPipeline>,
    font_system: ResMut<'w, CosmicFontSystem>,
    swash_cache: ResMut<'w, SwashCache>,
    font_atlas_set: ResMut<'w, FontAtlasSet>,
    texture_atlases: ResMut<'w, Assets<TextureAtlasLayout>>,
    helper_fonts: Query<'w, 's, &'static TextFont>,
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
) {
    let mut session = match spawn_pty(DEFAULT_COLS, DEFAULT_ROWS) {
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

    let mut reader = match session.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let status = format!("failed to attach PTY reader: {error}");
            let _ = snapshot_tx.send(TerminalSnapshot {
                surface: None,
                status: status.clone(),
            });
            set_terminal_error(&debug_stats, status);
            let _ = session.child.kill();
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
        loop {
            match input_rx.try_recv() {
                Ok(TerminalCommand::InputText(text)) => {
                    let bytes = text.as_bytes();
                    append_debug_log(format!("pty write text: {} bytes", bytes.len()));
                    if let Err(error) = write_input(&mut *session.writer, bytes) {
                        send_terminal_status_snapshot(
                            &snapshot_tx,
                            &debug_stats,
                            &terminal,
                            format!("PTY write failed for text input: {error}"),
                        );
                        running = false;
                        break;
                    }
                    with_debug_stats(&debug_stats, |stats| {
                        stats.pty_bytes_written += bytes.len() as u64;
                    });
                }
                Ok(TerminalCommand::InputEvent(event)) => {
                    let bytes = event.as_bytes();
                    append_debug_log(format!("pty write input event: {} bytes", bytes.len()));
                    if let Err(error) = write_input(&mut *session.writer, bytes) {
                        send_terminal_status_snapshot(
                            &snapshot_tx,
                            &debug_stats,
                            &terminal,
                            format!("PTY write failed for input event: {error}"),
                        );
                        running = false;
                        break;
                    }
                    with_debug_stats(&debug_stats, |stats| {
                        stats.pty_bytes_written += bytes.len() as u64;
                    });
                }
                Ok(TerminalCommand::SendCommand(command)) => {
                    let payload = format!("{command}\r");
                    let bytes = payload.as_bytes();
                    append_debug_log(format!(
                        "pty write command `{command}`: {} bytes",
                        bytes.len()
                    ));
                    if let Err(error) = write_input(&mut *session.writer, bytes) {
                        send_terminal_status_snapshot(
                            &snapshot_tx,
                            &debug_stats,
                            &terminal,
                            format!("PTY write failed for command `{command}`: {error}"),
                        );
                        running = false;
                        break;
                    }
                    with_debug_stats(&debug_stats, |stats| {
                        stats.pty_bytes_written += bytes.len() as u64;
                    });
                }
                Ok(TerminalCommand::Shutdown) => {
                    running = false;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    send_terminal_status_snapshot(
                        &snapshot_tx,
                        &debug_stats,
                        &terminal,
                        "terminal input channel disconnected",
                    );
                    running = false;
                    break;
                }
            }
        }

        while let Ok(bytes) = pty_output_rx.try_recv() {
            append_debug_log(format!("pty read: {} bytes", bytes.len()));
            with_debug_stats(&debug_stats, |stats| {
                stats.pty_bytes_read += bytes.len() as u64;
            });
            parser.advance(&mut terminal, &bytes);
        }

        let reader_status = match reader_state.lock() {
            Ok(state) => state.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(status) = reader_status {
            send_terminal_status_snapshot(&snapshot_tx, &debug_stats, &terminal, status);
            running = false;
        }

        match session.child.try_wait() {
            Ok(Some(status)) => {
                send_terminal_status_snapshot(
                    &snapshot_tx,
                    &debug_stats,
                    &terminal,
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
                    format!("PTY child wait failed: {error}"),
                );
                running = false;
            }
        }

        let snapshot = TerminalSnapshot {
            surface: Some(build_surface(&terminal)),
            status: "backend: alacritty_terminal + portable-pty".into(),
        };

        if running && snapshot != last_snapshot {
            last_snapshot = snapshot.clone();
            if snapshot_tx.send(snapshot).is_ok() {
                with_debug_stats(&debug_stats, |stats| {
                    stats.snapshots_sent += 1;
                });
            }
        }

        thread::sleep(Duration::from_millis(16));
    }

    let _ = session.child.kill();
    let _ = reader_thread.join();
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

fn resolve_alacritty_color(
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

fn xterm_indexed_rgb(index: u8) -> Rgb {
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

fn configure_terminal_fonts(
    mut font_assets: ResMut<Assets<Font>>,
    mut font_state: ResMut<TerminalFontState>,
) {
    if font_state.report.is_some() {
        return;
    }

    match resolve_terminal_font_report() {
        Ok(report) => {
            match load_font_handle(&mut font_assets, &report.primary.path) {
                Ok(primary) => font_state.primary_font = Some(primary),
                Err(error) => {
                    font_state.report = Some(Err(error));
                    return;
                }
            }

            for fallback in &report.fallbacks {
                match load_font_handle(&mut font_assets, &fallback.path) {
                    Ok(handle) => {
                        if fallback.source.contains("private-use") {
                            font_state.private_use_font = Some(handle.clone());
                        }
                        if fallback.source.contains("emoji") {
                            font_state.emoji_font = Some(handle.clone());
                        }
                    }
                    Err(error) => {
                        font_state.report = Some(Err(error));
                        return;
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

fn resolve_terminal_font_report() -> Result<TerminalFontReport, String> {
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
struct KittyFontConfig {
    font_family: Option<String>,
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

fn find_kitty_config_path() -> Option<PathBuf> {
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

fn parse_kitty_config_file(
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

fn sync_terminal_font_helpers(
    font_state: Res<TerminalFontState>,
    texture_state: Res<TerminalTextureState>,
    mut helper_fonts: Query<(&TerminalFontRole, &mut TextFont)>,
) {
    if (!font_state.is_changed() && !texture_state.is_changed())
        || texture_state.helper_entities.is_none()
    {
        return;
    }

    let font_size = texture_state.cell_size.y.max(1) as f32 * 0.9;
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

fn sync_terminal_texture(
    view: Res<TerminalView>,
    font_state: Res<TerminalFontState>,
    mut texture_state: ResMut<TerminalTextureState>,
    mut glyph_cache: ResMut<TerminalGlyphCache>,
    mut render: TerminalTextureRenderParams,
) {
    let Some(surface) = &view.latest.surface else {
        texture_state.last_surface = None;
        return;
    };

    if font_state.primary_font.is_none() {
        return;
    }

    if font_state.is_changed() {
        glyph_cache.glyphs.clear();
    }

    if texture_state.last_surface.as_ref() == Some(surface) && !font_state.is_changed() {
        return;
    }

    let Some(image_handle) = texture_state.image.clone() else {
        return;
    };
    let Some(helper_entities) = texture_state.helper_entities else {
        return;
    };

    let cell_size = texture_state.cell_size;
    let texture_size = UVec2::new(
        surface.cols as u32 * cell_size.x.max(1),
        surface.rows as u32 * cell_size.y.max(1),
    );

    let mut composed = create_terminal_image(texture_size);
    repaint_terminal_image(
        &mut composed,
        surface,
        cell_size,
        helper_entities,
        &mut render,
        &mut glyph_cache,
        &font_state,
    );

    if let Some(target_image) = render.images.get_mut(&image_handle) {
        *target_image = composed;
        texture_state.texture_size = texture_size;
        texture_state.last_surface = Some(surface.clone());
    }
}

fn repaint_terminal_image(
    image: &mut Image,
    surface: &TerminalSurface,
    cell_size: UVec2,
    helper_entities: TerminalFontEntities,
    render: &mut TerminalTextureRenderParams,
    glyph_cache: &mut TerminalGlyphCache,
    font_state: &TerminalFontState,
) {
    image.clear(&[
        DEFAULT_BG.r(),
        DEFAULT_BG.g(),
        DEFAULT_BG.b(),
        DEFAULT_BG.a(),
    ]);

    for y in 0..surface.rows {
        for x in 0..surface.cols {
            let cell = surface.cell(x, y);
            let origin_x = x as u32 * cell_size.x;
            let origin_y = y as u32 * cell_size.y;
            fill_rect(image, origin_x, origin_y, cell_size.x, cell_size.y, cell.bg);

            if cell.width == 0 || cell.text.is_empty() {
                continue;
            }

            let (font_role, helper_entity, preserve_color) =
                select_terminal_font_role(&cell.text, font_state, helper_entities);
            let cache_key = TerminalGlyphCacheKey {
                text: cell.text.clone(),
                font_role,
                width_cells: cell.width,
                cell_width: cell_size.x,
                cell_height: cell_size.y,
            };

            let glyph = if let Some(glyph) = glyph_cache.glyphs.get(&cache_key) {
                glyph.clone()
            } else {
                let glyph =
                    rasterize_terminal_glyph(&cache_key, helper_entity, preserve_color, render);
                glyph_cache.glyphs.insert(cache_key.clone(), glyph.clone());
                glyph
            };

            blit_cached_glyph(image, origin_x, origin_y, &glyph, cell.fg);
        }
    }

    if let Some(cursor) = &surface.cursor {
        if cursor.visible {
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

fn rasterize_terminal_glyph(
    cache_key: &TerminalGlyphCacheKey,
    helper_entity: Entity,
    preserve_color: bool,
    render: &mut TerminalTextureRenderParams,
) -> CachedTerminalGlyph {
    let width = cache_key.cell_width * u32::from(cache_key.width_cells.max(1));
    let height = cache_key.cell_height.max(1);
    let mut pixels = vec![0; (width * height * 4) as usize];

    let Ok(text_font) = render.helper_fonts.get(helper_entity) else {
        return CachedTerminalGlyph {
            width,
            height,
            pixels,
            preserve_color,
        };
    };

    let bounds = TextBounds::new(width as f32, height as f32);
    let mut computed = ComputedTextBlock::default();
    let mut layout_info = TextLayoutInfo::default();

    if render
        .text_pipeline
        .update_buffer(
            &render.fonts,
            std::iter::once((
                helper_entity,
                0,
                cache_key.text.as_str(),
                text_font,
                Color::WHITE,
                LineHeight::RelativeToFont(1.0),
            )),
            LineBreak::NoWrap,
            Justify::Left,
            bounds,
            1.0,
            &mut computed,
            &mut render.font_system,
            FontHinting::Enabled,
        )
        .is_err()
    {
        return CachedTerminalGlyph {
            width,
            height,
            pixels,
            preserve_color,
        };
    }

    if render
        .text_pipeline
        .update_text_layout_info(
            &mut layout_info,
            render.helper_fonts.as_readonly(),
            1.0,
            &mut render.font_atlas_set,
            &mut render.texture_atlases,
            &mut render.images,
            &mut computed,
            &mut render.font_system,
            &mut render.swash_cache,
            bounds,
            Justify::Left,
        )
        .is_err()
    {
        return CachedTerminalGlyph {
            width,
            height,
            pixels,
            preserve_color,
        };
    }

    for glyph in &layout_info.glyphs {
        let Some(atlas_layout) = render.texture_atlases.get(glyph.atlas_info.texture_atlas) else {
            continue;
        };
        let Some(atlas_image) = render.images.get(glyph.atlas_info.texture) else {
            continue;
        };
        let rect = atlas_layout.textures[glyph.atlas_info.location.glyph_index];
        let dest_x = (glyph.position.x - glyph.size.x * 0.5).floor() as i32;
        let dest_y = (glyph.position.y - glyph.size.y * 0.5).floor() as i32;

        for src_y in 0..rect.height() {
            for src_x in 0..rect.width() {
                let target_x = dest_x + src_x as i32;
                let target_y = dest_y + src_y as i32;
                if target_x < 0
                    || target_y < 0
                    || target_x >= width as i32
                    || target_y >= height as i32
                {
                    continue;
                }

                let Some(src_pixel) =
                    atlas_image.pixel_bytes(UVec3::new(rect.min.x + src_x, rect.min.y + src_y, 0))
                else {
                    continue;
                };

                let source = if preserve_color {
                    [src_pixel[0], src_pixel[1], src_pixel[2], src_pixel[3]]
                } else {
                    [255, 255, 255, src_pixel[3]]
                };
                blend_over_pixel(&mut pixels, width, target_x as u32, target_y as u32, source);
            }
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

fn blend_rgba_in_place(dst: &mut [u8], source: [u8; 4]) {
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

fn sync_terminal_plane_transform(
    texture_state: Res<TerminalTextureState>,
    plane_state: Res<TerminalPlaneState>,
    mut plane_transform: Single<&mut Transform, With<TerminalPlaneMarker>>,
    mut camera_transform: Single<
        &mut Transform,
        (With<TerminalCameraMarker>, Without<TerminalPlaneMarker>),
    >,
) {
    let aspect = if texture_state.texture_size.y == 0 {
        1.0
    } else {
        texture_state.texture_size.x as f32 / texture_state.texture_size.y as f32
    };

    plane_transform.translation = plane_state.offset.extend(0.0);
    plane_transform.rotation =
        Quat::from_rotation_y(plane_state.yaw) * Quat::from_rotation_x(plane_state.pitch);
    plane_transform.scale = Vec3::new(TERMINAL_WORLD_HEIGHT * aspect, TERMINAL_WORLD_HEIGHT, 1.0);

    camera_transform.translation = Vec3::new(0.0, 0.0, plane_state.distance.max(1.0));
    camera_transform.look_at(Vec3::ZERO, Vec3::Y);
}

fn drag_terminal_plane(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut plane_state: ResMut<TerminalPlaneState>,
) {
    let delta = mouse_motion
        .read()
        .fold(Vec2::ZERO, |acc, event| acc + event.delta);

    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !primary_window.focused
        || !shift
        || !mouse_buttons.pressed(MouseButton::Middle)
        || delta == Vec2::ZERO
    {
        return;
    }

    let pan_scale = plane_state.distance / primary_window.height().max(1.0);
    plane_state.offset.x += delta.x * pan_scale;
    plane_state.offset.y -= delta.y * pan_scale;
}

fn zoom_terminal_plane(
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut plane_state: ResMut<TerminalPlaneState>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !primary_window.focused || !shift {
        return;
    }

    let zoom_delta = mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 24.0,
        }
    });

    if zoom_delta == 0.0 {
        return;
    }

    plane_state.distance = (plane_state.distance - zoom_delta * 0.8).clamp(2.0, 40.0);
    plane_state.focal_length = plane_state.distance;
}

#[allow(
    clippy::too_many_arguments,
    reason = "legacy per-cell renderer kept temporarily while texture renderer stabilizes"
)]
fn sync_terminal_plane(
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
    distance: f32,
    focal_length: f32,
}

#[derive(Clone, PartialEq)]
struct PlaneLayoutKey {
    window_width: f32,
    window_height: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
    focal_length: f32,
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

fn is_private_use_like(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

fn is_emoji_like(ch: char) -> bool {
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

fn poll_terminal_snapshots(bridge: Res<TerminalBridge>, mut view: ResMut<TerminalView>) {
    let receiver = match bridge.snapshot_rx.lock() {
        Ok(receiver) => receiver,
        Err(poisoned) => poisoned.into_inner(),
    };

    while let Ok(snapshot) = receiver.try_recv() {
        view.latest = snapshot;
        bridge.note_snapshot_applied();
    }
}

fn forward_keyboard_input(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    bridge: Res<TerminalBridge>,
    _primary_window: Single<&Window, With<PrimaryWindow>>,
) {
    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        bridge.note_key_event(event);
        if let Some(command) = keyboard_input_to_terminal_command(event, &keys) {
            bridge.send(command);
        }
    }
}

fn keyboard_input_to_terminal_command(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> Option<TerminalCommand> {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let super_key = keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight);

    if ctrl && !alt && !super_key {
        if let Some(control) = ctrl_sequence(event.key_code) {
            return Some(TerminalCommand::InputEvent(control.to_string()));
        }
    }

    match event.key_code {
        KeyCode::Enter => Some(TerminalCommand::InputEvent("\r".into())),
        KeyCode::Backspace => Some(TerminalCommand::InputEvent("\u{7f}".into())),
        KeyCode::Tab => Some(TerminalCommand::InputEvent("\t".into())),
        KeyCode::Escape => Some(TerminalCommand::InputEvent("\u{1b}".into())),
        KeyCode::ArrowUp => Some(TerminalCommand::InputEvent("\u{1b}[A".into())),
        KeyCode::ArrowDown => Some(TerminalCommand::InputEvent("\u{1b}[B".into())),
        KeyCode::ArrowRight => Some(TerminalCommand::InputEvent("\u{1b}[C".into())),
        KeyCode::ArrowLeft => Some(TerminalCommand::InputEvent("\u{1b}[D".into())),
        KeyCode::Home => Some(TerminalCommand::InputEvent("\u{1b}[H".into())),
        KeyCode::End => Some(TerminalCommand::InputEvent("\u{1b}[F".into())),
        KeyCode::PageUp => Some(TerminalCommand::InputEvent("\u{1b}[5~".into())),
        KeyCode::PageDown => Some(TerminalCommand::InputEvent("\u{1b}[6~".into())),
        KeyCode::Delete => Some(TerminalCommand::InputEvent("\u{1b}[3~".into())),
        KeyCode::Insert => Some(TerminalCommand::InputEvent("\u{1b}[2~".into())),
        _ if ctrl || alt || super_key => None,
        _ => event
            .text
            .as_ref()
            .filter(|text| !text.is_empty())
            .map(|text| TerminalCommand::InputText(text.to_string()))
            .or_else(|| match &event.logical_key {
                bevy::input::keyboard::Key::Character(text) if !text.is_empty() => {
                    Some(TerminalCommand::InputText(text.to_string()))
                }
                bevy::input::keyboard::Key::Space => Some(TerminalCommand::InputText(" ".into())),
                _ => None,
            }),
    }
}

fn ctrl_sequence(key_code: KeyCode) -> Option<&'static str> {
    match key_code {
        KeyCode::KeyA => Some("\u{1}"),
        KeyCode::KeyC => Some("\u{3}"),
        KeyCode::KeyD => Some("\u{4}"),
        KeyCode::KeyE => Some("\u{5}"),
        KeyCode::KeyL => Some("\u{c}"),
        KeyCode::KeyU => Some("\u{15}"),
        KeyCode::KeyZ => Some("\u{1a}"),
        _ => None,
    }
}

fn ui_overlay(
    mut contexts: EguiContexts,
    bridge: Res<TerminalBridge>,
    view: Res<TerminalView>,
    font_state: Res<TerminalFontState>,
    mut plane_state: ResMut<TerminalPlaneState>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("neozeus").strong());
            ui.separator();
            ui.label(view.latest.status.as_str());
            ui.separator();
            let debug = bridge.debug_stats_snapshot();
            ui.label(format!(
                "keys {} · queued {} · wr {} · rd {} · sent {} · applied {}",
                debug.key_events_seen,
                debug.commands_queued,
                debug.pty_bytes_written,
                debug.pty_bytes_read,
                debug.snapshots_sent,
                debug.snapshots_applied,
            ));
            ui.separator();
            if !debug.last_key.is_empty() {
                ui.label(format!("last key {}", debug.last_key));
                ui.separator();
            }
            if !debug.last_command.is_empty() {
                ui.label(format!("last cmd {}", debug.last_command));
                ui.separator();
            }
            if !debug.last_error.is_empty() {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("last err {}", debug.last_error),
                );
                ui.separator();
            }
            match font_state.report.as_ref() {
                Some(Ok(report)) => {
                    ui.label(format!("font: {}", report.primary.family));
                    ui.separator();
                    ui.label(format!("requested: {}", report.requested_family));
                    ui.separator();
                }
                Some(Err(error)) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("font error: {error}"));
                    ui.separator();
                }
                None => {
                    ui.label("font: loading");
                    ui.separator();
                }
            }
            ui.label(format!("zoom {:.2}", plane_state.distance));
            ui.separator();
            ui.label(format!(
                "offset {:.2},{:.2}",
                plane_state.offset.x, plane_state.offset.y
            ));
            ui.separator();
            ui.label("Shift+MMB drag: pan · Shift+wheel: zoom");
            ui.separator();
            if ui.button("reset view").clicked() {
                plane_state.yaw = 0.0;
                plane_state.pitch = 0.0;
                plane_state.distance = 10.0;
                plane_state.focal_length = 10.0;
                plane_state.offset = Vec2::ZERO;
            }
            if ui.button("pwd").clicked() {
                append_debug_log("ui button clicked: pwd");
                bridge.send(TerminalCommand::SendCommand("pwd".into()));
            }
            if ui.button("ls").clicked() {
                append_debug_log("ui button clicked: ls");
                bridge.send(TerminalCommand::SendCommand("ls".into()));
            }
            if ui.button("clear").clicked() {
                append_debug_log("ui button clicked: clear");
                bridge.send(TerminalCommand::SendCommand("clear".into()));
            }
            if ui.button("btop").clicked() {
                append_debug_log("ui button clicked: btop");
                bridge.send(TerminalCommand::SendCommand("btop".into()));
            }
            if ui.button("tmux").clicked() {
                append_debug_log("ui button clicked: tmux");
                bridge.send(TerminalCommand::SendCommand("tmux".into()));
            }
        });
    });

    Ok(())
}

impl Drop for TerminalBridge {
    fn drop(&mut self) {
        let _ = self.input_tx.send(TerminalCommand::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        blend_rgba_in_place, ctrl_sequence, find_kitty_config_path, format_startup_panic,
        is_emoji_like, is_private_use_like, keyboard_input_to_terminal_command,
        parse_kitty_config_file, resolve_alacritty_color, resolve_terminal_font_report,
        xterm_indexed_rgb, KittyFontConfig, TerminalCommand,
    };
    use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
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
        time::{SystemTime, UNIX_EPOCH},
    };

    fn pressed_text(key_code: KeyCode, text: Option<&str>) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key: Key::Character(text.unwrap_or("").into()),
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    #[test]
    fn ctrl_sequence_maps_common_shortcuts() {
        assert_eq!(ctrl_sequence(KeyCode::KeyC), Some("\u{3}"));
        assert_eq!(ctrl_sequence(KeyCode::KeyL), Some("\u{c}"));
        assert_eq!(ctrl_sequence(KeyCode::Enter), None);
    }

    #[test]
    fn plain_text_uses_text_payload() {
        let keys = ButtonInput::<KeyCode>::default();
        let event = pressed_text(KeyCode::KeyA, Some("a"));
        let command = keyboard_input_to_terminal_command(&event, &keys);
        match command {
            Some(TerminalCommand::InputText(text)) => assert_eq!(text, "a"),
            _ => panic!("expected text input command"),
        }
    }

    #[test]
    fn indexed_color_has_expected_blue_cube_entry() {
        let rgb = xterm_indexed_rgb(21);
        assert_eq!((rgb.r, rgb.g, rgb.b), (0, 0, 255));
    }

    #[test]
    fn alpha_blend_preserves_transparent_glyph_background() {
        let mut pixel = [0, 0, 0, 0];
        blend_rgba_in_place(&mut pixel, [255, 255, 255, 0]);
        assert_eq!(pixel, [0, 0, 0, 0]);

        blend_rgba_in_place(&mut pixel, [255, 255, 255, 128]);
        assert_eq!(pixel[3], 128);
    }

    #[test]
    fn named_cursor_color_resolves() {
        let color = resolve_alacritty_color(
            AnsiColor::Named(NamedColor::Cursor),
            &Default::default(),
            true,
        );
        assert_eq!((color.r(), color.g(), color.b()), (82, 173, 112));
    }

    #[test]
    fn parses_font_family_from_included_kitty_config() {
        let dir = temp_dir("neozeus-kitty-font-test");
        let main = dir.join("kitty.conf");
        let included = dir.join("fonts.conf");
        fs::write(&included, "font_family JetBrains Mono Nerd Font\n")
            .expect("failed to write include config");
        fs::write(&main, "include fonts.conf\n").expect("failed to write main config");

        let mut visited = BTreeSet::new();
        let mut config = KittyFontConfig::default();
        parse_kitty_config_file(&main, &mut visited, &mut config)
            .expect("failed to parse kitty config");

        assert_eq!(
            config.font_family.as_deref(),
            Some("JetBrains Mono Nerd Font")
        );
    }

    #[test]
    fn current_host_has_no_user_kitty_config() {
        assert_eq!(find_kitty_config_path(), None);
    }

    #[test]
    fn resolves_effective_terminal_font_stack_on_host() {
        let report = resolve_terminal_font_report().expect("failed to resolve terminal fonts");
        assert_eq!(report.requested_family, "monospace");
        assert_eq!(report.primary.family, "Adwaita Mono");
        assert!(report.primary.path.is_file());
        assert!(report
            .fallbacks
            .iter()
            .any(|face| face.family.contains("Nerd Font")));
    }

    #[test]
    fn detects_special_font_ranges() {
        assert!(is_private_use_like('\u{e0b0}'));
        assert!(is_emoji_like('🚀'));
        assert!(!is_private_use_like('a'));
    }

    #[test]
    fn formats_missing_gpu_startup_panics_as_user_facing_errors() {
        let error = format_startup_panic(&"Unable to find a GPU! renderer init failed")
            .expect("missing gpu panic should be formatted");
        assert!(error.contains("could not find a usable graphics adapter"));
        assert!(format_startup_panic(&"some other panic").is_none());
    }
}
