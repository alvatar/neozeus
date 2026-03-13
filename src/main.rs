use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    term::{cell::Flags, color::Colors, Config as TermConfig, Term},
    vte::ansi::{self, Color as AnsiColor, CursorShape, NamedColor, Rgb},
};
use bevy::{
    input::{keyboard::KeyboardInput, mouse::AccumulatedMouseMotion, ButtonState},
    prelude::*,
    sprite::Anchor,
    text::{Font, TextBounds, TextColor, TextFont},
    window::PrimaryWindow,
};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    collections::BTreeSet,
    env,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Mutex,
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

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "neozeus".into(),
                resolution: (1400, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
        .insert_resource(TerminalBridge::spawn())
        .insert_resource(TerminalView::default())
        .insert_resource(TerminalFontState::default())
        .insert_resource(TerminalPlaneState::default())
        .insert_resource(TerminalSceneState::default())
        .add_systems(Startup, setup_camera)
        .add_systems(
            Update,
            (
                poll_terminal_snapshots,
                configure_terminal_fonts,
                drag_terminal_plane,
                sync_terminal_plane,
                forward_keyboard_input,
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, ui_overlay)
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
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

#[derive(Resource)]
struct TerminalBridge {
    input_tx: Sender<TerminalCommand>,
    snapshot_rx: Mutex<Receiver<TerminalSnapshot>>,
}

impl TerminalBridge {
    fn spawn() -> Self {
        let (input_tx, input_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();

        thread::spawn(move || {
            let panic_snapshot_tx = snapshot_tx.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal_worker(input_rx, snapshot_tx)
            }));
            if let Err(payload) = result {
                let _ = panic_snapshot_tx.send(TerminalSnapshot {
                    surface: None,
                    status: format!(
                        "terminal worker panicked: {}",
                        panic_payload_to_string(payload)
                    ),
                });
            }
        });

        Self {
            input_tx,
            snapshot_rx: Mutex::new(snapshot_rx),
        }
    }

    fn send(&self, command: TerminalCommand) {
        let _ = self.input_tx.send(command);
    }
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
}

impl Default for TerminalPlaneState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            distance: 1800.0,
            focal_length: 1800.0,
        }
    }
}

#[derive(Resource, Default)]
struct TerminalSceneState {
    cols: usize,
    rows: usize,
    initialized: bool,
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

#[derive(bevy::ecs::system::SystemParam)]
struct TerminalPlaneQueries<'w, 's> {
    bg_query: Query<'w, 's, BackgroundQueryItem<'static>, With<TerminalBackgroundMarker>>,
    glyph_query: Query<'w, 's, GlyphQueryItem<'static>, With<TerminalGlyphMarker>>,
    cursor_query: Query<
        'w,
        's,
        (
            &'static mut Sprite,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        With<TerminalCursorMarker>,
    >,
}

#[derive(Clone, Copy)]
struct ProjectedBasis {
    origin: Vec2,
    horizontal: Vec2,
    vertical: Vec2,
}

fn terminal_worker(input_rx: Receiver<TerminalCommand>, snapshot_tx: Sender<TerminalSnapshot>) {
    let mut session = match spawn_pty(DEFAULT_COLS, DEFAULT_ROWS) {
        Ok(session) => session,
        Err(error) => {
            let _ = snapshot_tx.send(TerminalSnapshot {
                surface: None,
                status: format!("failed to start PTY backend: {error}"),
            });
            return;
        }
    };

    let mut reader = match session.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let _ = snapshot_tx.send(TerminalSnapshot {
                surface: None,
                status: format!("failed to attach PTY reader: {error}"),
            });
            let _ = session.child.kill();
            return;
        }
    };

    let (pty_output_tx, pty_output_rx) = mpsc::channel::<Vec<u8>>();
    let reader_thread = thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if pty_output_tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
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
                    if write_input(&mut *session.writer, text.as_bytes()).is_err() {
                        running = false;
                        break;
                    }
                }
                Ok(TerminalCommand::InputEvent(event)) => {
                    if write_input(&mut *session.writer, event.as_bytes()).is_err() {
                        running = false;
                        break;
                    }
                }
                Ok(TerminalCommand::SendCommand(command)) => {
                    let payload = format!("{command}\r");
                    if write_input(&mut *session.writer, payload.as_bytes()).is_err() {
                        running = false;
                        break;
                    }
                }
                Ok(TerminalCommand::Shutdown) => {
                    running = false;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    running = false;
                    break;
                }
            }
        }

        while let Ok(bytes) = pty_output_rx.try_recv() {
            parser.advance(&mut terminal, &bytes);
        }

        let snapshot = TerminalSnapshot {
            surface: Some(build_surface(&terminal)),
            status: "backend: alacritty_terminal + portable-pty".into(),
        };

        if snapshot != last_snapshot {
            last_snapshot = snapshot.clone();
            let _ = snapshot_tx.send(snapshot);
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

fn drag_terminal_plane(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut plane_state: ResMut<TerminalPlaneState>,
) {
    if !mouse_buttons.pressed(MouseButton::Middle) {
        return;
    }

    let delta = mouse_motion.delta;
    if delta == Vec2::ZERO {
        return;
    }

    plane_state.yaw += delta.x * 0.005;
    plane_state.pitch = (plane_state.pitch - delta.y * 0.005).clamp(-1.1, 1.1);
}

fn sync_terminal_plane(
    mut commands: Commands,
    view: Res<TerminalView>,
    window: Single<&Window, With<PrimaryWindow>>,
    plane_state: Res<TerminalPlaneState>,
    mut scene_state: ResMut<TerminalSceneState>,
    font_state: Res<TerminalFontState>,
    mut queries: TerminalPlaneQueries,
) {
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
        return;
    }

    let layout = compute_plane_layout(surface, *window, &plane_state);

    for (_, index, mut sprite, mut transform, mut visibility) in &mut queries.bg_query {
        let cell = surface.cell(index.x, index.y);
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
        if cell.width == 0 || cell.text.is_empty() {
            *visibility = Visibility::Hidden;
            continue;
        }

        if let Some(projected) = project_cell(index.x, index.y, cell.width, &layout, &plane_state) {
            *visibility = Visibility::Visible;
            *text = Text2d::new(cell.text.clone());
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

    if let Ok((mut sprite, mut transform, mut visibility)) = queries.cursor_query.single_mut() {
        if let Some(cursor) = &surface.cursor {
            if cursor.visible {
                if let Some(projected) = project_cell(cursor.x, cursor.y, 1, &layout, &plane_state)
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
    }
}

fn forward_keyboard_input(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    bridge: Res<TerminalBridge>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
) {
    if !primary_window.focused || mouse_buttons.pressed(MouseButton::Middle) {
        return;
    }

    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

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
            .map(|text| TerminalCommand::InputText(text.to_string())),
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
            ui.label(format!("yaw {:.2}", plane_state.yaw));
            ui.separator();
            ui.label(format!("pitch {:.2}", plane_state.pitch));
            ui.separator();
            ui.label("MMB drag: tilt terminal plane");
            ui.separator();
            if ui.button("reset tilt").clicked() {
                plane_state.yaw = 0.0;
                plane_state.pitch = 0.0;
            }
            if ui.button("pwd").clicked() {
                bridge.send(TerminalCommand::SendCommand("pwd".into()));
            }
            if ui.button("ls").clicked() {
                bridge.send(TerminalCommand::SendCommand("ls".into()));
            }
            if ui.button("clear").clicked() {
                bridge.send(TerminalCommand::SendCommand("clear".into()));
            }
            if ui.button("btop").clicked() {
                bridge.send(TerminalCommand::SendCommand("btop".into()));
            }
            if ui.button("tmux").clicked() {
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
        ctrl_sequence, find_kitty_config_path, is_emoji_like, is_private_use_like,
        keyboard_input_to_terminal_command, parse_kitty_config_file, resolve_alacritty_color,
        resolve_terminal_font_report, xterm_indexed_rgb, KittyFontConfig, TerminalCommand,
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
}
