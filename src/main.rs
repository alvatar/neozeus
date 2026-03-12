use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    term::{cell::Flags, color::Colors, Config as TermConfig, Term},
    vte::ansi::{self, Color as AnsiColor, CursorShape, NamedColor, Rgb},
};
use bevy::{
    input::{keyboard::KeyboardInput, ButtonState},
    prelude::*,
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
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 38;
const TERMINAL_FONT_FAMILY_NAME: &str = "neozeus-terminal";
const FONT_METRIC_SAMPLE_SIZE: f32 = 16.0;
const DEFAULT_BG: egui::Color32 = egui::Color32::from_rgb(10, 10, 10);

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
        .add_systems(Startup, setup_camera)
        .add_systems(Update, (poll_terminal_snapshots, forward_keyboard_input))
        .add_systems(
            EguiPrimaryContextPass,
            (configure_terminal_fonts, ui_terminal).chain(),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
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

        thread::spawn(move || terminal_worker(input_rx, snapshot_tx));

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
    custom_font_ready: bool,
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

fn install_terminal_fonts(ctx: &egui::Context) -> Result<TerminalFontReport, String> {
    let report = resolve_terminal_font_report()?;
    let mut definitions = egui::FontDefinitions::default();
    let mut family_chain = Vec::new();

    insert_font_face(&mut definitions, &report.primary, &mut family_chain)?;
    for fallback in &report.fallbacks {
        insert_font_face(&mut definitions, fallback, &mut family_chain)?;
    }

    let family = egui::FontFamily::Name(Arc::from(TERMINAL_FONT_FAMILY_NAME));
    definitions
        .families
        .insert(family.clone(), family_chain.clone());

    let monospace = definitions
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default();
    for name in family_chain.iter().rev() {
        monospace.retain(|existing| existing != name);
        monospace.insert(0, name.clone());
    }

    ctx.set_fonts(definitions);
    Ok(report)
}

fn insert_font_face(
    definitions: &mut egui::FontDefinitions,
    face: &TerminalFontFace,
    family_chain: &mut Vec<String>,
) -> Result<(), String> {
    let key = format!("{}#{}", face.family, face.path.display());
    if definitions.font_data.contains_key(&key) {
        family_chain.push(key);
        return Ok(());
    }

    let bytes = fs::read(&face.path)
        .map_err(|error| format!("failed to read font {}: {error}", face.path.display()))?;
    definitions
        .font_data
        .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
    family_chain.push(key);
    Ok(())
}

fn paint_terminal(ui: &mut egui::Ui, surface: &TerminalSurface, use_custom_font: bool) {
    let available = ui.available_size();
    let desired = egui::Vec2::new(available.x.max(64.0), available.y.max(64.0));
    let (response, painter) = ui.allocate_painter(desired, egui::Sense::click());
    let outer_rect = response.rect;

    painter.rect_filled(outer_rect, 0.0, DEFAULT_BG);

    if surface.cols == 0 || surface.rows == 0 {
        return;
    }

    let font_family = if use_custom_font {
        egui::FontFamily::Name(Arc::from(TERMINAL_FONT_FAMILY_NAME))
    } else {
        egui::FontFamily::Monospace
    };

    let sample_font = egui::FontId::new(FONT_METRIC_SAMPLE_SIZE, font_family.clone());
    let sample_galley = painter.layout_no_wrap("M".to_owned(), sample_font, egui::Color32::WHITE);
    let sample_size = sample_galley.size();
    let glyph_w = sample_size.x.max(1.0);
    let glyph_h = sample_size.y.max(1.0);
    let cell_aspect = (glyph_w / glyph_h).clamp(0.3, 1.0);

    let cell_h = (outer_rect.height() / surface.rows as f32)
        .min(outer_rect.width() / (surface.cols as f32 * cell_aspect))
        .max(1.0);
    let cell_w = (cell_h * cell_aspect).max(1.0);
    let grid_size = egui::Vec2::new(cell_w * surface.cols as f32, cell_h * surface.rows as f32);
    let grid_min = egui::Pos2::new(
        outer_rect.left() + (outer_rect.width() - grid_size.x) * 0.5,
        outer_rect.top() + (outer_rect.height() - grid_size.y) * 0.5,
    );
    let grid_rect = egui::Rect::from_min_size(grid_min, grid_size);

    let font_scale = (cell_w / glyph_w).min(cell_h / glyph_h) * 0.98;
    let font = egui::FontId::new((FONT_METRIC_SAMPLE_SIZE * font_scale).max(6.0), font_family);

    for y in 0..surface.rows {
        for x in 0..surface.cols {
            let cell = surface.cell(x, y);
            let min = egui::Pos2::new(
                grid_rect.left() + x as f32 * cell_w,
                grid_rect.top() + y as f32 * cell_h,
            );
            let width = if cell.width <= 1 {
                cell_w
            } else {
                cell_w * f32::from(cell.width)
            };
            let cell_rect = egui::Rect::from_min_size(min, egui::Vec2::new(width, cell_h));
            painter.rect_filled(cell_rect, 0.0, cell.bg);

            if cell.width == 0 || cell.text.is_empty() {
                continue;
            }

            let galley = painter.layout_no_wrap(cell.text.clone(), font.clone(), cell.fg);
            let text_pos = egui::Pos2::new(
                cell_rect.min.x,
                cell_rect.center().y - galley.size().y * 0.5,
            );
            painter
                .with_clip_rect(cell_rect)
                .galley(text_pos, galley, cell.fg);
        }
    }

    if let Some(cursor) = &surface.cursor {
        if cursor.visible && cursor.x < surface.cols && cursor.y < surface.rows {
            let min = egui::Pos2::new(
                grid_rect.left() + cursor.x as f32 * cell_w,
                grid_rect.top() + cursor.y as f32 * cell_h,
            );
            let cursor_rect =
                egui::Rect::from_min_size(min, egui::Vec2::new(cell_w.max(1.0), cell_h.max(1.0)));
            match cursor.shape {
                TerminalCursorShape::Block => {
                    painter.rect_stroke(
                        cursor_rect.shrink(1.0),
                        0.0,
                        egui::Stroke::new(1.5, cursor.color),
                        egui::StrokeKind::Outside,
                    );
                }
                TerminalCursorShape::Underline => {
                    painter.line_segment(
                        [cursor_rect.left_bottom(), cursor_rect.right_bottom()],
                        egui::Stroke::new(2.0, cursor.color),
                    );
                }
                TerminalCursorShape::Beam => {
                    painter.line_segment(
                        [cursor_rect.left_top(), cursor_rect.left_bottom()],
                        egui::Stroke::new(2.0, cursor.color),
                    );
                }
            }
        }
    }
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
    let receiver = bridge
        .snapshot_rx
        .lock()
        .expect("terminal snapshot receiver mutex poisoned");

    while let Ok(snapshot) = receiver.try_recv() {
        view.latest = snapshot;
    }
}

fn forward_keyboard_input(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    bridge: Res<TerminalBridge>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
) {
    if !primary_window.focused {
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

fn configure_terminal_fonts(
    mut contexts: EguiContexts,
    mut font_state: ResMut<TerminalFontState>,
) -> Result {
    if font_state.report.is_none() {
        let ctx = contexts.ctx_mut()?;
        font_state.report = Some(install_terminal_fonts(ctx));
        font_state.custom_font_ready = false;
        return Ok(());
    }

    if !font_state.custom_font_ready {
        font_state.custom_font_ready = matches!(font_state.report.as_ref(), Some(Ok(_)));
    }

    Ok(())
}

fn ui_terminal(
    mut contexts: EguiContexts,
    bridge: Res<TerminalBridge>,
    view: Res<TerminalView>,
    font_state: Res<TerminalFontState>,
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

    let use_custom_font = font_state.custom_font_ready;

    egui::CentralPanel::default().show(ctx, |ui| {
        if let Some(surface) = &view.latest.surface {
            paint_terminal(ui, surface, use_custom_font);
        } else {
            ui.label("terminal not available");
        }
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
        ctrl_sequence, find_kitty_config_path, keyboard_input_to_terminal_command,
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
}
