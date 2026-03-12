use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    index::{Column, Line},
    term::{Config as TermConfig, Term},
    vte::ansi,
};
use bevy::{
    input::{keyboard::KeyboardInput, ButtonState},
    prelude::*,
    window::PrimaryWindow,
};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
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
                title: "neozeus :: bevy + alacritty_terminal".into(),
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

#[derive(Clone, Default)]
struct TerminalSnapshot {
    screen: String,
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
                screen: format!("Failed to start PTY backend:\n\n{error}"),
                status: "alacritty_terminal backend failed to start".into(),
            });
            return;
        }
    };

    let mut reader = match session.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let _ = snapshot_tx.send(TerminalSnapshot {
                screen: format!("Failed to attach PTY reader:\n\n{error}"),
                status: "alacritty_terminal backend failed to attach reader".into(),
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
    let mut last_screen = String::new();
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

        let screen = visible_screen(&terminal);
        if screen != last_screen {
            last_screen.clone_from(&screen);
            let _ = snapshot_tx.send(TerminalSnapshot {
                screen,
                status: "native core: alacritty_terminal + portable-pty".into(),
            });
        }

        thread::sleep(Duration::from_millis(16));
    }

    let _ = session.child.kill();
    let _ = reader_thread.join();
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

fn visible_screen(term: &Term<VoidListener>) -> String {
    let mut output = String::new();

    for row in 0..term.screen_lines() {
        let mut line = String::new();
        for col in 0..term.columns() {
            let cell = &term.grid()[Line(row as i32)][Column(col)];
            line.push(cell.c);
            if let Some(extra) = cell.zerowidth() {
                for character in extra {
                    line.push(*character);
                }
            }
        }

        let trimmed = line.trim_end_matches(' ');
        output.push_str(trimmed);
        output.push('\n');
    }

    output
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
            ui.label("Native Rust terminal core inside Bevy.");
            ui.separator();
            ui.label("Keys: type, Enter, Backspace, Tab, arrows, Esc, Ctrl+C/D/L/U");
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
            if ui.button("top").clicked() {
                bridge.send(TerminalCommand::SendCommand("top".into()));
            }
            if ui.button("vi").clicked() {
                bridge.send(TerminalCommand::SendCommand("vi".into()));
            }
        });
    });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(10, 10, 10)))
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(view.latest.status.as_str())
                    .monospace()
                    .color(egui::Color32::from_rgb(120, 180, 120)),
            );
            ui.add_space(6.0);
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
                    ui.label(
                        egui::RichText::new(view.latest.screen.as_str())
                            .monospace()
                            .color(egui::Color32::from_rgb(210, 210, 210)),
                    );
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
        ctrl_sequence, keyboard_input_to_terminal_command, visible_screen, TerminalCommand,
    };
    use alacritty_terminal::{
        event::VoidListener,
        grid::Dimensions,
        term::{Config as TermConfig, Term},
        vte::ansi,
    };
    use bevy::{
        input::{
            keyboard::{Key, KeyboardInput},
            ButtonState,
        },
        prelude::*,
    };

    struct TestDimensions {
        cols: usize,
        rows: usize,
    }

    impl Dimensions for TestDimensions {
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
    fn visible_screen_extracts_plain_text() {
        let dimensions = TestDimensions { cols: 5, rows: 2 };
        let mut terminal = Term::new(TermConfig::default(), &dimensions, VoidListener);
        let mut parser = ansi::Processor::<ansi::StdSyncHandler>::new();
        parser.advance(&mut terminal, b"abc\r\ndef");
        let screen = visible_screen(&terminal);
        assert!(screen.contains("abc"));
        assert!(screen.contains("def"));
    }
}
