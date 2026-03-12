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
use bevy_terminal_shared::{
    paint_terminal, TerminalCell, TerminalCursor, TerminalCursorShape, TerminalSurface,
};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    env,
    ffi::OsString,
    io::{Read, Write},
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Mutex,
    },
    thread,
    time::Duration,
};

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 38;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "neozeus :: bevy + alacritty terminal".into(),
                resolution: (1400, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
        .insert_resource(TerminalBridge::spawn())
        .insert_resource(TerminalView::default())
        .add_systems(Startup, setup_camera)
        .add_systems(Update, (poll_terminal_snapshots, forward_keyboard_input))
        .add_systems(EguiPrimaryContextPass, ui_terminal)
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
            status: "native core: alacritty_terminal + portable-pty".into(),
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

fn ui_terminal(
    mut contexts: EguiContexts,
    bridge: Res<TerminalBridge>,
    view: Res<TerminalView>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("PoC B").strong());
            ui.separator();
            ui.label(view.latest.status.as_str());
            ui.separator();
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

    egui::CentralPanel::default().show(ctx, |ui| {
        if let Some(surface) = &view.latest.surface {
            paint_terminal(ui, surface);
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
        ctrl_sequence, keyboard_input_to_terminal_command, resolve_alacritty_color,
        xterm_indexed_rgb, TerminalCommand,
    };
    use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
    use bevy::{
        input::{
            keyboard::{Key, KeyboardInput},
            ButtonState,
        },
        prelude::*,
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
}
