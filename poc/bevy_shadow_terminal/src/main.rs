use bevy::{
    input::{keyboard::KeyboardInput, ButtonState},
    prelude::*,
    window::PrimaryWindow,
};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use bevy_terminal_shared::{
    install_terminal_fonts, paint_terminal, TerminalCell, TerminalCursor, TerminalCursorShape,
    TerminalFontReport, TerminalSurface,
};
use shadow_terminal::{
    shadow_terminal::Config,
    steppable_terminal::{Input, SteppableTerminal},
    wezterm_term::color::{ColorPalette, SrgbaTuple},
};
use std::{
    env,
    ffi::OsString,
    sync::{
        mpsc::{self, Receiver, Sender, TryRecvError},
        Mutex,
    },
    thread,
    time::Duration,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "neozeus :: bevy + shadow terminal".into(),
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

fn terminal_worker(input_rx: Receiver<TerminalCommand>, snapshot_tx: Sender<TerminalSnapshot>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime for terminal worker");

    runtime.block_on(async move {
        let config = Config {
            width: 120,
            height: 38,
            command: shell_command(),
            ..Config::default()
        };

        let mut terminal = match SteppableTerminal::start(config).await {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = snapshot_tx.send(TerminalSnapshot {
                    surface: None,
                    status: format!("failed to start terminal backend: {error:?}"),
                });
                return;
            }
        };

        let mut last_snapshot = TerminalSnapshot::default();
        let mut running = true;

        while running {
            loop {
                match input_rx.try_recv() {
                    Ok(TerminalCommand::InputText(text)) => {
                        let _ = terminal.send_input(Input::Characters(text));
                    }
                    Ok(TerminalCommand::InputEvent(event)) => {
                        let _ = terminal.send_input(Input::Event(event));
                    }
                    Ok(TerminalCommand::SendCommand(command)) => {
                        let _ = terminal.send_command(&command);
                    }
                    Ok(TerminalCommand::Shutdown) => {
                        let _ = terminal.kill();
                        running = false;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        let _ = terminal.kill();
                        running = false;
                        break;
                    }
                }
            }

            let _ = terminal.render_all_output().await;

            let snapshot = TerminalSnapshot {
                surface: Some(build_surface(&mut terminal)),
                status: "native core: shadow-terminal / wezterm-term".into(),
            };

            if snapshot != last_snapshot {
                last_snapshot = snapshot.clone();
                let _ = snapshot_tx.send(snapshot);
            }

            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    });
}

fn build_surface(terminal: &mut SteppableTerminal) -> TerminalSurface {
    let palette = ColorPalette::default();
    let size = terminal.shadow_terminal.terminal.get_size();
    let cols = size.cols;
    let rows = size.rows;
    let mut surface = TerminalSurface::new(cols, rows);

    for y in 0..rows {
        for x in 0..cols {
            let maybe_cell = terminal
                .shadow_terminal
                .terminal
                .screen_mut()
                .get_cell(x, y as i64)
                .cloned();
            let Some(cell) = maybe_cell else {
                continue;
            };

            let attrs = cell.attrs();
            let mut fg = color32_from_srgba(palette.resolve_fg(attrs.foreground()));
            let mut bg = color32_from_srgba(palette.resolve_bg(attrs.background()));
            if attrs.reverse() {
                std::mem::swap(&mut fg, &mut bg);
            }

            let text = if attrs.invisible() {
                String::new()
            } else {
                cell.str().to_owned()
            };

            surface.set_cell(
                x,
                y,
                TerminalCell {
                    text,
                    fg,
                    bg,
                    width: cell.width() as u8,
                },
            );
        }
    }

    let cursor = terminal.shadow_terminal.terminal.cursor_pos();
    surface.cursor = Some(TerminalCursor {
        x: cursor.x.min(cols.saturating_sub(1)),
        y: (cursor.y.max(0) as usize).min(rows.saturating_sub(1)),
        shape: TerminalCursorShape::Block,
        visible: true,
        color: color32_from_srgba(palette.cursor_bg),
    });
    surface.title = Some(terminal.shadow_terminal.terminal.get_title().to_owned());
    surface
}

fn color32_from_srgba(color: SrgbaTuple) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (color.0.clamp(0.0, 1.0) * 255.0) as u8,
        (color.1.clamp(0.0, 1.0) * 255.0) as u8,
        (color.2.clamp(0.0, 1.0) * 255.0) as u8,
        (color.3.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

fn shell_command() -> Vec<OsString> {
    match env::var_os("SHELL") {
        Some(shell) => vec![shell],
        None => vec![OsString::from("bash")],
    }
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
    if font_state.report.is_some() {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;
    font_state.report = Some(install_terminal_fonts(ctx));
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
            ui.label(egui::RichText::new("PoC A").strong());
            ui.separator();
            ui.label(view.latest.status.as_str());
            ui.separator();
            match font_state.report.as_ref() {
                Some(Ok(report)) => {
                    ui.label(format!("font: {}", report.primary.family));
                    ui.separator();
                    ui.label(format!("source: {}", report.primary.source));
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
            if let Some(surface) = &view.latest.surface {
                if let Some(title) = &surface.title {
                    ui.label(format!("title: {title}"));
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

    let use_custom_font = matches!(font_state.report.as_ref(), Some(Ok(_)));

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
        color32_from_srgba, ctrl_sequence, keyboard_input_to_terminal_command, TerminalCommand,
    };
    use bevy::{
        input::{
            keyboard::{Key, KeyboardInput},
            ButtonState,
        },
        prelude::*,
    };
    use shadow_terminal::wezterm_term::color::SrgbaTuple;

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
    fn srgba_maps_to_color32() {
        let color = color32_from_srgba(SrgbaTuple(1.0, 0.5, 0.0, 1.0));
        assert_eq!(color.r(), 255);
        assert_eq!(color.g(), 127);
        assert_eq!(color.b(), 0);
    }
}
