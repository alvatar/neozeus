mod modal_dialogs;

use crate::{
    aegis::{AegisPolicyStore, DEFAULT_AEGIS_PROMPT},
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::{
        AegisCommand, AgentCommand as AppAgentCommand, AppCommand, AppSessionState,
        CloneAgentDialogField, ComposerCommand, ComposerRequest, CreateAgentDialogField,
        CreateAgentKind, OwnedTmuxCommand, RenameAgentDialogField, TaskCommand as AppTaskCommand,
    },
    composer::{
        aegis_dialog_target_at, clone_agent_dialog_target_at, create_agent_dialog_target_at,
        message_box_action_at, message_box_rect, rename_agent_dialog_target_at,
        task_dialog_action_at, task_dialog_rect, AegisDialogTarget, CloneAgentDialogTarget,
        CreateAgentDialogTarget, MessageDialogFocus, RenameAgentDialogTarget, TaskDialogFocus,
    },
    hud::{HudInputCaptureState, HudLayoutState},
    terminals::{
        terminal_texture_screen_size, ActiveTerminalContentState, TerminalCommand,
        TerminalDisplayMode, TerminalFocusState, TerminalId, TerminalManager, TerminalPanel,
        TerminalPointerState, TerminalPresentation, TerminalPresentationStore, TerminalViewState,
        TerminalViewportPoint,
    },
    text_selection::{
        extract_terminal_selection_text, resolved_terminal_selection_surface,
        PrimarySelectionOwnerState, PrimarySelectionState, TerminalSelectionPoint,
        TerminalTextSelectionDragSource, TerminalTextSelectionOwner, TerminalTextSelectionState,
    },
};
use bevy::{
    app::AppExit,
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
        ButtonState,
    },
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use bevy_egui::EguiClipboard;
use std::{
    io::Write,
    process::{Command, Stdio},
};

/// Reads the three modifier families that matter to NeoZeus shortcut handling.
///
/// The return value is `(ctrl, alt, super)` and deliberately merges left/right variants so the rest
/// of the input code can reason about logical modifiers instead of physical keys.
fn has_plain_modifiers(keys: &ButtonInput<KeyCode>) -> (bool, bool, bool) {
    (
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight),
    )
}

/// Recognizes the exact `Ctrl+Enter` chord used to toggle direct-input mode.
///
/// The check is intentionally strict: the event must be a key press for `Enter`, Ctrl must be down,
/// and Alt/Super must both be absent so the shortcut cannot collide with terminal input or desktop
/// bindings.
fn is_plain_ctrl_enter(event: &KeyboardInput, ctrl: bool, alt: bool, super_key: bool) -> bool {
    event.state == ButtonState::Pressed
        && event.key_code == KeyCode::Enter
        && ctrl
        && !alt
        && !super_key
}

/// Hides the exact runtime-state predicate used when deciding whether keyboard input may be routed
/// into a terminal.
///
/// Keeping the call behind a local helper makes the intent at call sites clearer and gives this file
/// one place to change if the project's idea of "interactive" ever becomes stricter than the raw
/// runtime state's helper.
fn terminal_is_interactive(terminal: &crate::terminals::TerminalRuntimeState) -> bool {
    terminal.is_interactive()
}

/// Decides whether a keyboard event means "spawn a normal terminal".
///
/// The binding is intentionally plain `z` on key press with no Ctrl/Alt/Super modifiers. The helper
/// does not emit commands itself; it just encapsulates the binding policy so systems and tests can
/// share the same rule.
pub(crate) fn should_spawn_terminal_globally(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::KeyZ {
        return false;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    !(ctrl || alt || super_key)
}

/// Decides whether a keyboard event means "open the clone-agent dialog".
pub(crate) fn should_open_clone_agent_dialog(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::KeyC {
        return false;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    !(ctrl || alt || super_key)
}

/// Decides whether a keyboard event should kill the currently active terminal session.
///
/// The shortcut is a plain `Ctrl+k` press. Like the other `should_*` helpers, this function only
/// classifies the event; lifecycle side effects happen in the higher-level system.
pub(crate) fn should_kill_active_terminal(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::KeyK {
        return false;
    }
    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    ctrl && !alt && !super_key
}

/// Decides whether a keyboard event should exit the whole application.
///
/// NeoZeus uses plain `F10` with no modifiers for this so the exit path stays orthogonal to terminal
/// key handling and to the modal editor shortcuts.
pub(crate) fn should_exit_application(event: &KeyboardInput, keys: &ButtonInput<KeyCode>) -> bool {
    if event.state != ButtonState::Pressed || event.key_code != KeyCode::F10 {
        return false;
    }
    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    !(ctrl || alt || super_key)
}

#[allow(
    clippy::too_many_arguments,
    reason = "global shortcut handling needs window focus, selection, catalog, session, and redraw output together"
)]
/// Watches unfocused-by-modal keyboard input for the global create-agent shortcut.
///
/// The system exits early whenever the primary window is unfocused or a HUD modal currently owns the
/// keyboard. Otherwise it scans the frame's keyboard events and opens the create-agent dialog on the
/// plain global spawn binding.
pub(crate) fn handle_global_terminal_spawn_shortcut(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    agent_catalog: Res<AgentCatalog>,
    selection: Option<Res<crate::hud::AgentListSelection>>,
    mut app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    if app_session.keyboard_capture_active(&input_capture) || !primary_window.focused {
        return;
    }

    let default_selection = crate::hud::AgentListSelection::None;
    let selection = selection.as_deref().unwrap_or(&default_selection);

    for event in messages.read() {
        if should_spawn_terminal_globally(event, &keys) {
            app_session.create_agent_dialog.open(CreateAgentKind::Pi);
            redraws.write(RequestRedraw);
            break;
        }
        if should_open_clone_agent_dialog(event, &keys) {
            let crate::hud::AgentListSelection::Agent(agent_id) = *selection else {
                continue;
            };
            if agent_catalog.kind(agent_id) != Some(crate::agents::AgentKind::Pi)
                || agent_catalog.clone_source_session_path(agent_id).is_none()
            {
                continue;
            }
            let current_label = agent_catalog.label(agent_id).unwrap_or("AGENT");
            app_session.clone_agent_dialog.open(agent_id, current_label);
            redraws.write(RequestRedraw);
            break;
        }
    }
}

/// Applies the global lifecycle shortcuts that are allowed outside modal text entry.
///
/// The system is intentionally small and imperative: if a modal has keyboard capture, do nothing;
/// otherwise scan the frame's key presses for `F10` to exit or `Ctrl+k` to kill the currently
/// selected row. Exit short-circuits the loop because the rest of the frame does not matter once
/// shutdown is requested.
pub(crate) fn handle_terminal_lifecycle_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    selection: Option<Res<crate::hud::AgentListSelection>>,
    mut app_commands: MessageWriter<AppCommand>,
    mut app_exits: MessageWriter<AppExit>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    if app_session.keyboard_capture_active(&input_capture) {
        return;
    }

    let default_selection = crate::hud::AgentListSelection::None;
    let selection = selection.as_deref().unwrap_or(&default_selection);

    for event in messages.read() {
        if should_exit_application(event, &keys) {
            app_exits.write(AppExit::Success);
            break;
        }
        if should_kill_active_terminal(event, &keys) {
            match *selection {
                crate::hud::AgentListSelection::OwnedTmux(_) => {
                    app_commands.write(AppCommand::OwnedTmux(OwnedTmuxCommand::KillSelected));
                }
                crate::hud::AgentListSelection::Agent(_) => {
                    app_commands.write(AppCommand::Agent(AppAgentCommand::KillSelected));
                }
                crate::hud::AgentListSelection::None => {}
            }
        }
    }
}

/// Tests whether a window-space cursor position lies inside a terminal panel's current on-screen
/// rectangle.
///
/// Terminal presentations are stored around the scene center, while the cursor arrives in window
/// coordinates with an upper-left origin. The function converts the panel's centered presentation
/// rectangle into the same window coordinate system and then performs a simple bounds check.
fn terminal_panel_screen_rect(
    window: &Window,
    presentation: &TerminalPresentation,
) -> (Vec2, Vec2) {
    let min = Vec2::new(
        window.width() * 0.5 + presentation.current_position.x - presentation.current_size.x * 0.5,
        window.height() * 0.5 - presentation.current_position.y - presentation.current_size.y * 0.5,
    );
    let max = min + presentation.current_size;
    (min, max)
}

fn terminal_panel_contains_cursor(
    window: &Window,
    presentation: &TerminalPresentation,
    cursor: Vec2,
) -> bool {
    let (min, max) = terminal_panel_screen_rect(window, presentation);
    cursor.x >= min.x && cursor.x <= max.x && cursor.y >= min.y && cursor.y <= max.y
}

/// Finds the frontmost visible terminal panel under the cursor.
///
/// The query is filtered in three steps: hidden panels are ignored, the cursor must land inside the
/// panel rectangle, and ties are resolved by the current presentation `z` so clicking overlapping
/// panels always targets the one visually on top.
fn topmost_terminal_panel_at_cursor(
    window: &Window,
    panels: &Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    cursor: Vec2,
) -> Option<TerminalPanel> {
    panels
        .iter()
        .filter(|(_, _, visibility)| **visibility == Visibility::Visible)
        .filter(|(_, presentation, _)| terminal_panel_contains_cursor(window, presentation, cursor))
        .max_by(|(_, left, _), (_, right, _)| left.current_z.total_cmp(&right.current_z))
        .map(|(panel, _, _)| *panel)
}

fn terminal_selection_point_from_cursor(
    window: &Window,
    presentation: &TerminalPresentation,
    surface: &crate::terminals::TerminalSurface,
    cursor: Vec2,
) -> TerminalSelectionPoint {
    let (min, max) = terminal_panel_screen_rect(window, presentation);
    let clamped_x = cursor.x.clamp(min.x, max.x.max(min.x + 1.0) - 1.0);
    let clamped_y = cursor.y.clamp(min.y, max.y.max(min.y + 1.0) - 1.0);
    let local_x = (clamped_x - min.x).max(0.0);
    let local_y = (clamped_y - min.y).max(0.0);
    let cell_w = (presentation.current_size.x / surface.cols.max(1) as f32).max(1.0);
    let cell_h = (presentation.current_size.y / surface.rows.max(1) as f32).max(1.0);
    TerminalSelectionPoint {
        col: ((local_x / cell_w).floor() as usize).min(surface.cols.saturating_sub(1)),
        row: ((local_y / cell_h).floor() as usize).min(surface.rows.saturating_sub(1)),
    }
}

fn viewport_point(selection_point: TerminalSelectionPoint) -> TerminalViewportPoint {
    TerminalViewportPoint {
        col: selection_point.col,
        row: selection_point.row,
    }
}

fn clear_live_terminal_selection(
    terminal_manager: &TerminalManager,
    terminal_text_selection: &mut TerminalTextSelectionState,
) {
    if let Some(terminal_id) = terminal_text_selection.live_terminal_id() {
        if let Some(bridge) = terminal_manager
            .get(terminal_id)
            .map(|terminal| &terminal.bridge)
        {
            bridge.send(TerminalCommand::ClearSelection);
        }
    }
    terminal_text_selection.clear_selection();
}

#[allow(
    clippy::too_many_arguments,
    reason = "pointer focus routing now consults explicit input ownership before translating clicks into intents"
)]
/// Turns a left-click on a visible terminal panel into focus + isolate intents.
///
/// The system deliberately refuses to act while a modal is open, while the window is unfocused, or
/// when the click lands on a HUD module. Only genuine background clicks on a terminal panel are
/// promoted into `FocusTerminal` and `HideAllButTerminal` intents.
pub(crate) fn focus_terminal_on_panel_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    runtime_index: Res<AgentRuntimeIndex>,
    mut app_commands: MessageWriter<AppCommand>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if app_session.modal_input_owner(&input_capture)
        || !mouse_buttons.just_pressed(MouseButton::Left)
        || !primary_window.focused
    {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return;
    }
    let Some(panel) = topmost_terminal_panel_at_cursor(&primary_window, &panels, cursor) else {
        return;
    };
    let Some(agent_id) = runtime_index.agent_for_terminal(panel.id) else {
        return;
    };
    app_commands.write(AppCommand::Agent(AppAgentCommand::Inspect(agent_id)));
}

#[allow(
    clippy::too_many_arguments,
    reason = "background-click clear needs input, focus, visibility, view, and persistence resources together"
)]
/// Clears terminal focus when the user clicks on empty background space.
///
/// This is the inverse of panel focusing: if the click is not blocked by a modal, does not hit a HUD
/// module, and does not land on any visible terminal panel, the active terminal is cleared. The
/// function also resets visibility to `ShowAll`, clears per-terminal view focus, reconciles direct
/// input capture, and marks session persistence dirty so the unfocused state can be saved.
pub(crate) fn hide_terminal_on_background_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    focus_state: Res<TerminalFocusState>,
    mut app_commands: MessageWriter<AppCommand>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if app_session.modal_input_owner(&input_capture)
        || !mouse_buttons.just_pressed(MouseButton::Left)
        || !primary_window.focused
    {
        return;
    }
    let Some(_) = focus_state.active_id() else {
        return;
    };
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return;
    }
    if topmost_terminal_panel_at_cursor(&primary_window, &panels, cursor).is_some() {
        return;
    }
    app_commands.write(AppCommand::Agent(AppAgentCommand::ClearFocus));
}

#[allow(
    clippy::too_many_arguments,
    reason = "mouse drag needs input, geometry, pointer state, and terminal bridge"
)]
/// Handles middle-mouse dragging for either viewport panning or terminal scrollback.
///
/// The mode split is deliberate:
/// - `Shift + middle-drag` pans the presented terminal by mutating the view offset directly.
/// - plain `middle-drag` is translated into line-based scrollback commands sent to the active
///   terminal bridge.
///
/// For scrollback, the function converts pixel motion into logical terminal lines using the current
/// presented cell height and carries sub-line remainder in [`TerminalPointerState`] so slow drags do
/// not lose precision.
pub(crate) fn paste_into_create_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    match create_agent_dialog_target_at(window, cursor) {
        Some(CreateAgentDialogTarget::NameField) => {
            app_session.create_agent_dialog.focus = CreateAgentDialogField::Name;
            app_session.create_agent_dialog.cwd_field.clear_completion();
            app_session.create_agent_dialog.error = None;
            let text = crate::agents::uppercase_agent_label_text(text);
            app_session
                .create_agent_dialog
                .name_field
                .insert_text(&text)
        }
        Some(CreateAgentDialogTarget::StartingFolderField) => {
            app_session.create_agent_dialog.focus = CreateAgentDialogField::StartingFolder;
            app_session.create_agent_dialog.error = None;
            app_session
                .create_agent_dialog
                .cwd_field
                .mutate_text(|field| field.insert_text(text))
        }
        _ => false,
    }
}

pub(crate) fn paste_into_clone_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    match clone_agent_dialog_target_at(window, cursor) {
        Some(CloneAgentDialogTarget::NameField) => {
            app_session.clone_agent_dialog.focus = CloneAgentDialogField::Name;
            app_session.clone_agent_dialog.error = None;
            let text = crate::agents::uppercase_agent_label_text(text);
            app_session.clone_agent_dialog.name_field.insert_text(&text)
        }
        _ => false,
    }
}

pub(crate) fn paste_into_rename_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    match rename_agent_dialog_target_at(window, cursor) {
        Some(RenameAgentDialogTarget::NameField) => {
            app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
            app_session.rename_agent_dialog.error = None;
            let text = crate::agents::uppercase_agent_label_text(text);
            app_session
                .rename_agent_dialog
                .name_field
                .insert_text(&text)
        }
        _ => false,
    }
}

pub(crate) fn paste_into_aegis_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    match aegis_dialog_target_at(window, cursor) {
        Some(AegisDialogTarget::PromptField) => {
            app_session.aegis_dialog.focus = crate::app::AegisDialogField::Prompt;
            app_session.aegis_dialog.error = None;
            app_session.aegis_dialog.prompt_field.insert_text(text)
        }
        _ => false,
    }
}

pub(crate) fn paste_into_message_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    if message_box_action_at(window, cursor).is_some() || !message_box_rect(window).contains(cursor)
    {
        return false;
    }
    app_session.composer.message_dialog_focus = MessageDialogFocus::Editor;
    app_session.composer.message_editor.insert_text(text)
}

pub(crate) fn paste_into_task_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    if task_dialog_action_at(window, cursor).is_some() || !task_dialog_rect(window).contains(cursor)
    {
        return false;
    }
    app_session.composer.task_dialog_focus = TaskDialogFocus::Editor;
    app_session.composer.task_editor.insert_text(text)
}

#[cfg(target_os = "linux")]
fn read_linux_primary_selection_text_with(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
    mut run_command: impl FnMut(&str, &[&str]) -> Option<Vec<u8>>,
) -> Option<String> {
    let prefer_wayland = wayland_display.is_some()
        || session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland"));
    let prefer_x11 =
        display.is_some() || session_type.is_some_and(|value| value.eq_ignore_ascii_case("x11"));
    let mut candidates = Vec::new();
    if prefer_wayland {
        candidates.push(("wl-paste", ["--primary", "--no-newline"].as_slice()));
    }
    if prefer_x11 {
        candidates.push(("xclip", ["-selection", "primary", "-o"].as_slice()));
        candidates.push(("xsel", ["--primary", "--output"].as_slice()));
    }
    if !prefer_wayland && !prefer_x11 {
        candidates.push(("wl-paste", ["--primary", "--no-newline"].as_slice()));
        candidates.push(("xclip", ["-selection", "primary", "-o"].as_slice()));
        candidates.push(("xsel", ["--primary", "--output"].as_slice()));
    }

    candidates.into_iter().find_map(|(program, args)| {
        let output = run_command(program, args)?;
        if output.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&output).into_owned())
    })
}

#[cfg(target_os = "linux")]
fn read_linux_primary_selection_text() -> Option<String> {
    read_linux_primary_selection_text_with(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY")
            .as_deref()
            .and_then(|value| value.to_str()),
        std::env::var_os("DISPLAY")
            .as_deref()
            .and_then(|value| value.to_str()),
        |program, args| {
            let output = Command::new(program).args(args).output().ok()?;
            if !output.status.success() {
                return None;
            }
            Some(output.stdout)
        },
    )
}

#[cfg(target_os = "linux")]
fn write_linux_primary_selection_text_with(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
    text: &str,
    mut run_command: impl FnMut(&str, &[&str], &str) -> bool,
) -> bool {
    let prefer_wayland = wayland_display.is_some()
        || session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland"));
    let prefer_x11 =
        display.is_some() || session_type.is_some_and(|value| value.eq_ignore_ascii_case("x11"));
    let mut candidates = Vec::new();
    if prefer_wayland {
        candidates.push(("wl-copy", ["--primary", "--type", "text/plain"].as_slice()));
    }
    if prefer_x11 {
        candidates.push(("xclip", ["-selection", "primary", "-in"].as_slice()));
        candidates.push(("xsel", ["--primary", "--input"].as_slice()));
    }
    if !prefer_wayland && !prefer_x11 {
        candidates.push(("wl-copy", ["--primary", "--type", "text/plain"].as_slice()));
        candidates.push(("xclip", ["-selection", "primary", "-in"].as_slice()));
        candidates.push(("xsel", ["--primary", "--input"].as_slice()));
    }

    candidates
        .into_iter()
        .any(|(program, args)| run_command(program, args, text))
}

#[cfg(target_os = "linux")]
fn stop_primary_selection_owner(owner: &mut PrimarySelectionOwnerState) {
    if let Some(mut child) = owner.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(target_os = "linux")]
fn write_linux_primary_selection_text(owner: &mut PrimarySelectionOwnerState, text: &str) -> bool {
    let success = write_linux_primary_selection_text_with(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY")
            .as_deref()
            .and_then(|value| value.to_str()),
        std::env::var_os("DISPLAY")
            .as_deref()
            .and_then(|value| value.to_str()),
        text,
        |program, args, text| {
            stop_primary_selection_owner(owner);
            let Ok(mut child) = Command::new(program)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                return false;
            };
            let Some(mut stdin) = child.stdin.take() else {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            };
            if stdin.write_all(text.as_bytes()).is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            drop(stdin);
            owner.child = Some(child);
            true
        },
    );
    if !success {
        stop_primary_selection_owner(owner);
    }
    success
}

fn middle_click_paste_text(clipboard: Option<&mut EguiClipboard>) -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Some(text) = read_linux_primary_selection_text().filter(|text| !text.is_empty()) {
        return Some(text);
    }

    clipboard.and_then(|clipboard| clipboard.get_text().filter(|text| !text.is_empty()))
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal middle-click paste needs layout, focus, hit-testing, runtime, and text together"
)]
pub(crate) fn paste_into_direct_input_terminal(
    window: &Window,
    cursor: Vec2,
    layout_state: &HudLayoutState,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    input_capture: &HudInputCaptureState,
    panels: &Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    text: &str,
) -> bool {
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return false;
    }

    let Some(target_terminal) = input_capture.direct_input_terminal else {
        return false;
    };
    if Some(target_terminal) != focus_state.active_id() {
        return false;
    }
    let Some(panel) = topmost_terminal_panel_at_cursor(window, panels, cursor) else {
        return false;
    };
    if panel.id != target_terminal {
        return false;
    }
    let Some(terminal) = terminal_manager.get(target_terminal) else {
        return false;
    };
    if !terminal_is_interactive(&terminal.snapshot.runtime) {
        return false;
    }
    terminal
        .bridge
        .send(TerminalCommand::InputText(text.to_owned()));
    true
}

#[allow(
    clippy::too_many_arguments,
    reason = "middle-click paste needs clipboard, writable target hit-testing, terminal state, and redraws together"
)]
pub(crate) fn handle_middle_click_paste(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    mut app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    mut clipboard: Option<ResMut<EguiClipboard>>,
    mut redraws: MessageWriter<RequestRedraw>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
) {
    if !primary_window.focused || !mouse_buttons.just_pressed(MouseButton::Middle) {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    let Some(text) = middle_click_paste_text(clipboard.as_deref_mut()) else {
        return;
    };

    let pasted_into_dialog = if app_session.create_agent_dialog.visible {
        paste_into_create_agent_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.clone_agent_dialog.visible {
        paste_into_clone_agent_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.rename_agent_dialog.visible {
        paste_into_rename_agent_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.aegis_dialog.visible {
        paste_into_aegis_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.composer.message_editor.visible {
        paste_into_message_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.composer.task_editor.visible {
        paste_into_task_dialog(&mut app_session, &primary_window, cursor, &text)
    } else {
        false
    };
    if pasted_into_dialog {
        redraws.write(RequestRedraw);
        return;
    }

    let _ = paste_into_direct_input_terminal(
        &primary_window,
        cursor,
        &layout_state,
        &terminal_manager,
        &focus_state,
        &input_capture,
        &panels,
        &text,
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal text selection needs cursor hit-testing, panel geometry, surface state, and redraws together"
)]
pub(crate) fn handle_terminal_text_selection(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    active_terminal_content: Res<ActiveTerminalContentState>,
    mut terminal_text_selection: ResMut<TerminalTextSelectionState>,
    mut agent_list_text_selection: ResMut<crate::text_selection::AgentListTextSelectionState>,
    mut redraws: MessageWriter<RequestRedraw>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
) {
    if !primary_window.focused {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        if mouse_buttons.just_released(MouseButton::Left) {
            terminal_text_selection.clear_drag();
        }
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        if mouse_buttons.just_released(MouseButton::Left) {
            terminal_text_selection.clear_drag();
        }
        return;
    }

    let panel_hit = panels
        .iter()
        .filter(|(_, _, visibility)| **visibility == Visibility::Visible)
        .filter(|(_, presentation, _)| {
            terminal_panel_contains_cursor(&primary_window, presentation, cursor)
        })
        .max_by(|(_, left, _), (_, right, _)| left.current_z.total_cmp(&right.current_z))
        .map(|(panel, presentation, _)| (*panel, *presentation));

    if mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some((panel, presentation)) = panel_hit.as_ref() {
            if let Some(resolved_surface) = resolved_terminal_selection_surface(
                &terminal_manager,
                &active_terminal_content,
                panel.id,
            ) {
                clear_live_terminal_selection(&terminal_manager, &mut terminal_text_selection);
                agent_list_text_selection.clear_selection();
                let anchor = terminal_selection_point_from_cursor(
                    &primary_window,
                    presentation,
                    resolved_surface.surface,
                    cursor,
                );
                match resolved_surface.token {
                    crate::text_selection::TerminalSelectionSurfaceToken::Snapshot(_) => {
                        terminal_text_selection.begin_live_drag(panel.id, anchor);
                    }
                    crate::text_selection::TerminalSelectionSurfaceToken::ActiveOverride(_) => {
                        terminal_text_selection.begin_override_drag(panel.id, anchor);
                    }
                }
                redraws.write(RequestRedraw);
            }
        }
        return;
    }

    if let Some(drag) = terminal_text_selection.drag {
        if mouse_buttons.pressed(MouseButton::Left) {
            if let Some((panel, presentation)) = panel_hit.as_ref() {
                if panel.id == drag.terminal_id {
                    if let Some(resolved_surface) = resolved_terminal_selection_surface(
                        &terminal_manager,
                        &active_terminal_content,
                        panel.id,
                    ) {
                        let focus = terminal_selection_point_from_cursor(
                            &primary_window,
                            presentation,
                            resolved_surface.surface,
                            cursor,
                        );
                        match (drag.source, resolved_surface.token) {
                            (
                                TerminalTextSelectionDragSource::LiveTerminal,
                                crate::text_selection::TerminalSelectionSurfaceToken::Snapshot(_),
                            ) => {
                                if extract_terminal_selection_text(
                                    resolved_surface.surface,
                                    drag.anchor,
                                    focus,
                                )
                                .is_some()
                                {
                                    terminal_text_selection.adopt_live_selection_owner(panel.id);
                                    if let Some(bridge) = terminal_manager
                                        .get(panel.id)
                                        .map(|terminal| &terminal.bridge)
                                    {
                                        bridge.send(TerminalCommand::SetSelection {
                                            anchor: viewport_point(drag.anchor),
                                            focus: viewport_point(focus),
                                        });
                                    }
                                }
                            }
                            (
                                TerminalTextSelectionDragSource::OverrideSurface,
                                crate::text_selection::TerminalSelectionSurfaceToken::ActiveOverride(_),
                            ) => {
                                if let Some(text) = extract_terminal_selection_text(
                                    resolved_surface.surface,
                                    drag.anchor,
                                    focus,
                                ) {
                                    terminal_text_selection.set_override_selection(
                                        panel.id,
                                        drag.anchor,
                                        focus,
                                        text,
                                        resolved_surface.token,
                                    );
                                    redraws.write(RequestRedraw);
                                }
                            }
                            _ => {}
                        }
                        return;
                    }
                }
            }
        }
        if mouse_buttons.just_released(MouseButton::Left) {
            terminal_text_selection.clear_drag();
        }
    }
}

pub(crate) fn sync_primary_selection_from_ui_text_selection(
    terminal_manager: Res<TerminalManager>,
    terminal_text_selection: Res<TerminalTextSelectionState>,
    agent_list_text_selection: Res<crate::text_selection::AgentListTextSelectionState>,
    mut primary_selection: ResMut<PrimarySelectionState>,
    mut owner: ResMut<PrimarySelectionOwnerState>,
) {
    if !terminal_manager.is_changed()
        && !terminal_text_selection.is_changed()
        && !agent_list_text_selection.is_changed()
    {
        return;
    }

    let live_terminal_selection = match terminal_text_selection.owner() {
        Some(TerminalTextSelectionOwner::LiveTerminal(terminal_id)) => terminal_manager
            .get(terminal_id)
            .and_then(|terminal| terminal.snapshot.surface.as_ref())
            .and_then(|surface| surface.selected_text.as_deref())
            .map(|text| (terminal_id, text)),
        Some(TerminalTextSelectionOwner::OverrideSurface(terminal_id)) => terminal_text_selection
            .override_selection_for(terminal_id)
            .map(|selection| (terminal_id, selection.text.as_str())),
        None => None,
    };

    let changed = if let Some((terminal_id, text)) = live_terminal_selection {
        primary_selection.set_terminal_selection(terminal_id, text)
    } else if let Some(selection) = agent_list_text_selection.selection() {
        primary_selection.set_agent_list_selection(&selection.text)
    } else {
        primary_selection.clear()
    };
    if !changed {
        return;
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(text) = primary_selection.text() {
            let _ = write_linux_primary_selection_text(&mut owner, text);
        } else {
            stop_primary_selection_owner(&mut owner);
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "mouse drag needs input, geometry, pointer state, and terminal bridge"
)]
/// Handles middle-mouse dragging for either viewport panning or terminal scrollback.
///
/// The mode split is deliberate:
/// - `Shift + middle-drag` pans the presented terminal by mutating the view offset directly.
/// - plain `middle-drag` is translated into line-based scrollback commands sent to the active
///   terminal bridge.
///
/// For scrollback, the function converts pixel motion into logical terminal lines using the current
/// presented cell height and carries sub-line remainder in [`TerminalPointerState`] so slow drags do
/// not lose precision.
pub(crate) fn drag_terminal_view(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    presentation_store: Res<TerminalPresentationStore>,
    layout_state: Res<HudLayoutState>,
    mut view_state: ResMut<TerminalViewState>,
    mut pointer_state: ResMut<TerminalPointerState>,
) {
    // Aggregate all motion events for the frame so drag behavior is framerate-independent.
    let delta = mouse_motion
        .read()
        .fold(Vec2::ZERO, |acc, event| acc + event.delta);

    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let middle_pressed = mouse_buttons.pressed(MouseButton::Middle);
    if !primary_window.focused || !middle_pressed || delta == Vec2::ZERO {
        pointer_state.scroll_drag_remainder_px = 0.0;
        return;
    }

    if shift {
        pointer_state.scroll_drag_remainder_px = 0.0;
        view_state.apply_offset_delta(focus_state.active_id(), Vec2::new(delta.x, -delta.y));
        return;
    }

    let Some(texture_state) = presentation_store.active_texture_state(focus_state.active_id())
    else {
        pointer_state.scroll_drag_remainder_px = 0.0;
        return;
    };
    let pixel_perfect = presentation_store.active_display_mode(focus_state.active_id())
        == Some(TerminalDisplayMode::PixelPerfect);
    let screen_size = terminal_texture_screen_size(
        texture_state,
        &view_state,
        &primary_window,
        &layout_state,
        pixel_perfect,
    );
    let screen_cell_height = if texture_state.cell_size.y == 0 || texture_state.texture_size.y == 0
    {
        1.0
    } else {
        screen_size.y * (texture_state.cell_size.y as f32 / texture_state.texture_size.y as f32)
    }
    .max(1.0);

    // Keep fractional drag distance between frames so one slow drag across multiple frames still
    // eventually produces the correct number of scroll lines.
    pointer_state.scroll_drag_remainder_px += delta.y;
    let lines = (-pointer_state.scroll_drag_remainder_px / screen_cell_height).trunc() as i32;
    if lines != 0 {
        pointer_state.scroll_drag_remainder_px += lines as f32 * screen_cell_height;
        if let Some(bridge) = focus_state.active_bridge(&terminal_manager) {
            bridge.send(TerminalCommand::ScrollDisplay(lines));
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "wheel scrolling needs window focus, HUD ownership, focus, capture, terminal routing, and pointer accumulation together"
)]
/// Routes ordinary mouse-wheel input into terminal scrollback for either the focused terminal or the
/// direct-input target terminal.
///
/// `Shift + wheel` is reserved for zoom and is ignored here. HUD-owned regions are also ignored so
/// module scrolling keeps priority when the cursor is over HUD content.
const TERMINAL_PAGE_SCROLL_FALLBACK_ROWS: i32 = 20;

fn terminal_page_scroll_rows(terminal_manager: &TerminalManager, terminal_id: TerminalId) -> i32 {
    terminal_manager
        .get(terminal_id)
        .and_then(|terminal| terminal.snapshot.surface.as_ref())
        .map(|surface| surface.rows.saturating_sub(1))
        .filter(|rows| *rows > 0)
        .and_then(|rows| i32::try_from(rows).ok())
        .unwrap_or(TERMINAL_PAGE_SCROLL_FALLBACK_ROWS)
}

#[allow(
    clippy::too_many_arguments,
    reason = "wheel scrolling needs keyboard, window focus, HUD hit-testing, terminal routing, and pointer remainder state together"
)]
pub(crate) fn scroll_terminal_with_mouse_wheel(
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    input_capture: Res<HudInputCaptureState>,
    mut pointer_state: ResMut<TerminalPointerState>,
    mut mouse_wheel: MessageReader<MouseWheel>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !primary_window.focused || shift {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return;
    }

    let target_terminal = input_capture
        .direct_input_terminal
        .or_else(|| focus_state.active_id());
    let Some(bridge) = target_terminal
        .and_then(|terminal_id| terminal_manager.get(terminal_id))
        .map(|terminal| &terminal.bridge)
    else {
        return;
    };

    let wheel_delta_lines = mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 24.0,
        }
    });
    if wheel_delta_lines == 0.0 {
        return;
    }

    pointer_state.wheel_scroll_remainder_lines += wheel_delta_lines;
    let lines = pointer_state.wheel_scroll_remainder_lines.trunc() as i32;
    if lines == 0 {
        return;
    }
    pointer_state.wheel_scroll_remainder_lines -= lines as f32;
    bridge.send(TerminalCommand::ScrollDisplay(lines));
}

/// Applies shift-wheel zoom to the shared terminal view distance.
///
/// Only focused-window `Shift + wheel` input is treated as zoom. Mouse-wheel units are normalized to
/// a common scale and then applied to `view_state.distance`, which is clamped so the camera cannot be
/// zoomed into nonsense or pushed arbitrarily far away.
pub(crate) fn zoom_terminal_view(
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut view_state: ResMut<TerminalViewState>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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

    view_state.distance = (view_state.distance - zoom_delta * 0.8).clamp(2.0, 40.0);
}

#[allow(
    clippy::too_many_arguments,
    reason = "direct terminal input needs keyboard, focus, modal capture, terminal state, and redraws together"
)]
/// Routes keyboard input into the active terminal when direct-input mode is toggled on.
///
/// The system has two jobs. First, it watches for the `Ctrl+Enter` toggle that opens or closes
/// direct-input mode. Second, while the mode is active, it converts keyboard events into terminal
/// commands and sends them through the terminal bridge. If the target terminal disappears or stops
/// being interactive, the mode is closed immediately and the HUD is asked to redraw so the visual
/// framing stays in sync.
pub(crate) fn handle_terminal_direct_input_keyboard(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    mut app_session: ResMut<AppSessionState>,
    mut input_capture: ResMut<HudInputCaptureState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    if !primary_window.focused || app_session.modal_input_owner(&input_capture) {
        return;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(&keys);
    let had_direct_input = input_capture.direct_input_terminal.is_some();
    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
    if had_direct_input && input_capture.direct_input_terminal.is_none() {
        redraws.write(RequestRedraw);
    }

    if let Some(target_terminal) = input_capture.direct_input_terminal {
        // Direct-input mode is sticky across frames, but it is not allowed to outlive the terminal
        // it targets. Revalidate the target before forwarding any key events.
        let Some(terminal) = terminal_manager.get(target_terminal) else {
            input_capture.close_direct_terminal_input();
            redraws.write(RequestRedraw);
            return;
        };
        if !terminal_is_interactive(&terminal.snapshot.runtime) {
            input_capture.close_direct_terminal_input();
            redraws.write(RequestRedraw);
            return;
        }
        let mut mode_changed = false;
        for event in messages.read() {
            if is_plain_ctrl_enter(event, ctrl, alt, super_key) {
                let _ = input_capture
                    .toggle_direct_terminal_input(&mut app_session.composer, target_terminal);
                mode_changed = true;
                break;
            }
            if !ctrl && !alt && !super_key {
                match event.key_code {
                    KeyCode::End => {
                        let bottom_delta = terminal
                            .snapshot
                            .surface
                            .as_ref()
                            .and_then(|surface| i32::try_from(surface.display_offset).ok())
                            .map(|offset| -offset)
                            .unwrap_or(0);
                        if bottom_delta != 0 {
                            terminal.bridge.note_key_event(event);
                            terminal
                                .bridge
                                .send(TerminalCommand::ScrollDisplay(bottom_delta));
                        }
                        continue;
                    }
                    KeyCode::PageUp => {
                        terminal.bridge.note_key_event(event);
                        terminal.bridge.send(TerminalCommand::ScrollDisplay(
                            terminal_page_scroll_rows(&terminal_manager, target_terminal),
                        ));
                        continue;
                    }
                    KeyCode::PageDown => {
                        terminal.bridge.note_key_event(event);
                        terminal.bridge.send(TerminalCommand::ScrollDisplay(
                            -terminal_page_scroll_rows(&terminal_manager, target_terminal),
                        ));
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(command) = keyboard_input_to_terminal_command(event, &keys) {
                terminal.bridge.note_key_event(event);
                terminal.bridge.send(command);
            }
        }
        if mode_changed {
            redraws.write(RequestRedraw);
        }
        return;
    }

    let Some(active_id) = focus_state.active_id() else {
        return;
    };
    let Some(active_terminal) = terminal_manager.get(active_id) else {
        return;
    };
    if !terminal_is_interactive(&active_terminal.snapshot.runtime) {
        return;
    }
    for event in messages.read() {
        if !is_plain_ctrl_enter(event, ctrl, alt, super_key) {
            continue;
        }
        let _ = input_capture.toggle_direct_terminal_input(&mut app_session.composer, active_id);
        redraws.write(RequestRedraw);
        break;
    }
}

/// Extracts printable text from a keyboard event for the HUD text editors.
///
/// Bevy may provide text either through `event.text` or through `logical_key` for character-like
/// keys. This helper prefers the explicit text payload, falls back to the logical key when needed,
/// and special-cases space so the modal editor can accept ordinary typing without reimplementing
/// keyboard-layout details.
fn message_box_event_text(event: &KeyboardInput) -> Option<String> {
    event
        .text
        .as_ref()
        .filter(|text| !text.is_empty())
        .map(|text| text.to_string())
        .or_else(|| match &event.logical_key {
            Key::Character(text) if !text.is_empty() => Some(text.to_string()),
            Key::Space => Some(" ".to_owned()),
            _ => None,
        })
}

/// Applies one keyboard event to the Emacs-like HUD text editor state machine.
///
/// The editor supports three input bands:
/// - plain keys for movement, insertion, and simple editing,
/// - Ctrl bindings for line/region-oriented editing commands,
/// - Alt bindings for word-oriented motion and kill-ring operations.
///
/// The function returns whether the editor state changed so the caller can request redraw only when
/// necessary.
fn handle_text_editor_event(
    editor: &mut crate::composer::TextEditorState,
    event: &KeyboardInput,
    ctrl: bool,
    alt: bool,
    super_key: bool,
) -> bool {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    if ctrl && !alt && !super_key {
        match event.key_code {
            KeyCode::Space => editor.set_mark(),
            KeyCode::KeyA => editor.move_line_start(),
            KeyCode::KeyB => editor.move_left(),
            KeyCode::KeyD => editor.delete_forward_char(),
            KeyCode::KeyE => editor.move_line_end(),
            KeyCode::KeyF => editor.move_right(),
            KeyCode::KeyH => editor.delete_backward_char(),
            KeyCode::KeyJ => editor.newline_and_indent(),
            KeyCode::KeyK => editor.kill_to_end_of_line(),
            KeyCode::KeyN => editor.move_down(),
            KeyCode::KeyU => editor.kill_all(),
            KeyCode::KeyO => editor.open_line(),
            KeyCode::KeyP => editor.move_up(),
            KeyCode::KeyW => editor.kill_region(),
            KeyCode::KeyY => editor.yank(),
            _ => false,
        }
    } else if alt && !ctrl && !super_key {
        match event.key_code {
            KeyCode::Backspace => editor.kill_word_backward(),
            KeyCode::KeyB => editor.move_word_backward(),
            KeyCode::KeyD => editor.kill_word_forward(),
            KeyCode::KeyF => editor.move_word_forward(),
            KeyCode::KeyW => editor.copy_region(),
            KeyCode::KeyY => editor.yank_pop(),
            _ => false,
        }
    } else if !(ctrl || alt || super_key) {
        match event.key_code {
            KeyCode::Enter => editor.insert_newline(),
            KeyCode::Backspace => editor.delete_backward_char(),
            KeyCode::Delete => editor.delete_forward_char(),
            KeyCode::ArrowLeft => editor.move_left(),
            KeyCode::ArrowRight => editor.move_right(),
            KeyCode::ArrowUp => editor.move_up(),
            KeyCode::ArrowDown => editor.move_down(),
            KeyCode::Home => editor.move_line_start(),
            KeyCode::End => editor.move_line_end(),
            KeyCode::Tab => editor.insert_text("\t"),
            _ => message_box_event_text(event).is_some_and(|text| editor.insert_text(&text)),
        }
    } else {
        false
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "dialog keyboard handling needs input, focus, notes, terminal state, HUD state, commands, and redraws together"
)]
/// Handles keyboard input for the message box, the task dialog, and the shortcuts that open them.
///
/// The function is ordered as a small state machine:
/// 1. ignore everything when the window is unfocused,
/// 2. give direct-input mode priority and do nothing if it is active,
/// 3. if the message box is open, treat keys as editor/send/close commands,
/// 4. else if the task dialog is open, treat keys as editor/task-management commands,
/// 5. else, interpret plain terminal shortcuts such as opening the message box, opening the task
///    dialog, or consuming the next task.
///
/// That explicit ordering prevents global shortcuts from firing while a modal editor owns the same
/// keystrokes.
pub(crate) fn handle_terminal_message_box_keyboard(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    _terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    runtime_index: Res<AgentRuntimeIndex>,
    agent_catalog: Res<AgentCatalog>,
    aegis_policy: Option<Res<AegisPolicyStore>>,
    mut app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    mut clipboard: Option<ResMut<EguiClipboard>>,
    mut clipboard_ingress: Local<modal_dialogs::MessageDialogClipboardIngressState>,
    mut app_commands: MessageWriter<AppCommand>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    if !primary_window.focused {
        return;
    }

    let (ctrl, alt, super_key) = has_plain_modifiers(&keys);

    if input_capture.direct_input_terminal.is_some() {
        return;
    }

    let modifiers = modal_dialogs::KeyModifiers {
        ctrl,
        alt,
        super_key,
        shift: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
    };

    if app_session.aegis_dialog.visible {
        let mut needs_redraw = false;
        let mut emitted_commands = Vec::new();
        for event in messages.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }
            let outcome = modal_dialogs::handle_aegis_dialog_key(
                &mut app_session,
                event,
                modifiers,
                &mut emitted_commands,
            );
            needs_redraw |= outcome.needs_redraw;
            if outcome.stop {
                break;
            }
        }
        for command in emitted_commands {
            app_commands.write(command);
        }
        if needs_redraw {
            redraws.write(RequestRedraw);
        }
        return;
    }

    if app_session.rename_agent_dialog.visible {
        let mut needs_redraw = false;
        let mut emitted_commands = Vec::new();
        for event in messages.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }
            let outcome = modal_dialogs::handle_rename_agent_dialog_key(
                &mut app_session,
                event,
                modifiers,
                &mut emitted_commands,
            );
            needs_redraw |= outcome.needs_redraw;
            if outcome.stop {
                break;
            }
        }
        for command in emitted_commands {
            app_commands.write(command);
        }
        if needs_redraw {
            redraws.write(RequestRedraw);
        }
        return;
    }

    if app_session.clone_agent_dialog.visible {
        let mut needs_redraw = false;
        let mut emitted_commands = Vec::new();
        for event in messages.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }
            let outcome = modal_dialogs::handle_clone_agent_dialog_key(
                &mut app_session,
                event,
                modifiers,
                &mut emitted_commands,
            );
            needs_redraw |= outcome.needs_redraw;
            if outcome.stop {
                break;
            }
        }
        for command in emitted_commands {
            app_commands.write(command);
        }
        if needs_redraw {
            redraws.write(RequestRedraw);
        }
        return;
    }

    if app_session.create_agent_dialog.visible {
        let mut needs_redraw = false;
        let mut emitted_commands = Vec::new();
        for event in messages.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }
            let outcome = modal_dialogs::handle_create_agent_dialog_key(
                &mut app_session,
                event,
                modifiers,
                &mut emitted_commands,
            );
            needs_redraw |= outcome.needs_redraw;
            if outcome.stop {
                break;
            }
        }
        for command in emitted_commands {
            app_commands.write(command);
        }
        if needs_redraw {
            redraws.write(RequestRedraw);
        }
        return;
    }

    let current_clipboard_text = clipboard
        .as_deref_mut()
        .and_then(EguiClipboard::get_text)
        .filter(|text| !text.is_empty());
    clipboard_ingress.sync_visibility(
        app_session.composer.message_editor.visible,
        current_clipboard_text.as_deref(),
    );

    if app_session.composer.message_editor.visible {
        let mut needs_redraw = false;
        let mut emitted_commands = Vec::new();
        for event in messages.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }
            let current_clipboard_text = clipboard
                .as_deref_mut()
                .and_then(EguiClipboard::get_text)
                .filter(|text| !text.is_empty());
            let outcome = clipboard_ingress.handle_key(
                &mut app_session,
                event,
                modifiers,
                current_clipboard_text.as_deref(),
                &mut emitted_commands,
            );
            needs_redraw |= outcome.needs_redraw;
            if outcome.stop {
                break;
            }
        }
        for command in emitted_commands {
            app_commands.write(command);
        }
        if needs_redraw {
            redraws.write(RequestRedraw);
        }
        return;
    }

    if app_session.composer.task_editor.visible {
        let mut needs_redraw = false;
        let mut emitted_commands = Vec::new();
        for event in messages.read() {
            if event.state != ButtonState::Pressed {
                continue;
            }
            let outcome = modal_dialogs::handle_task_dialog_key(
                &mut app_session,
                event,
                modifiers,
                &mut emitted_commands,
            );
            needs_redraw |= outcome.needs_redraw;
            if outcome.stop {
                break;
            }
        }
        for command in emitted_commands {
            app_commands.write(command);
        }
        if needs_redraw {
            redraws.write(RequestRedraw);
        }
        return;
    }

    let default_aegis_policy = AegisPolicyStore::default();
    let aegis_policy = aegis_policy.as_deref().unwrap_or(&default_aegis_policy);

    let Some(active_id) = focus_state.active_id() else {
        return;
    };
    let Some(active_terminal) = _terminal_manager.get(active_id) else {
        return;
    };
    if !terminal_is_interactive(&active_terminal.snapshot.runtime) {
        return;
    }
    let page_rows = terminal_page_scroll_rows(&_terminal_manager, active_id);
    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        if ctrl && !alt && !super_key {
            match event.key_code {
                KeyCode::KeyT => {
                    if let Some(agent_id) = runtime_index.agent_for_terminal(active_id) {
                        app_commands
                            .write(AppCommand::Task(AppTaskCommand::ClearDone { agent_id }));
                    }
                    break;
                }
                KeyCode::KeyV => {
                    if let Some(terminal) = _terminal_manager.get(active_id) {
                        terminal
                            .bridge
                            .send(TerminalCommand::ScrollDisplay(-page_rows));
                    }
                    break;
                }
                _ => {}
            }
        }

        if alt && !ctrl && !super_key && event.key_code == KeyCode::KeyV {
            if let Some(terminal) = _terminal_manager.get(active_id) {
                terminal
                    .bridge
                    .send(TerminalCommand::ScrollDisplay(page_rows));
            }
            break;
        }

        if ctrl || alt || super_key {
            continue;
        }

        match event.key_code {
            KeyCode::Enter => {
                if let Some(agent_id) = runtime_index.agent_for_terminal(active_id) {
                    app_commands.write(AppCommand::Composer(ComposerCommand::Open(
                        ComposerRequest {
                            mode: crate::composer::ComposerMode::Message { agent_id },
                        },
                    )));
                }
                break;
            }
            KeyCode::KeyT => {
                if let Some(agent_id) = runtime_index.agent_for_terminal(active_id) {
                    app_commands.write(AppCommand::Composer(ComposerCommand::Open(
                        ComposerRequest {
                            mode: crate::composer::ComposerMode::TaskEdit { agent_id },
                        },
                    )));
                }
                break;
            }
            KeyCode::KeyR => {
                if let Some(agent_id) = runtime_index.agent_for_terminal(active_id) {
                    let current_label =
                        agent_catalog.label(agent_id).unwrap_or_default().to_owned();
                    app_session
                        .rename_agent_dialog
                        .open(agent_id, &current_label);
                    redraws.write(RequestRedraw);
                }
                break;
            }
            KeyCode::KeyA => {
                if let Some(agent_id) = runtime_index.agent_for_terminal(active_id) {
                    if let Some(agent_uid) = agent_catalog.uid(agent_id) {
                        if aegis_policy.is_enabled(agent_uid) {
                            app_commands
                                .write(AppCommand::Aegis(AegisCommand::Disable { agent_id }));
                        } else {
                            let prompt_text = aegis_policy
                                .prompt_text(agent_uid)
                                .unwrap_or(DEFAULT_AEGIS_PROMPT);
                            app_session.aegis_dialog.open(agent_id, prompt_text);
                            redraws.write(RequestRedraw);
                        }
                    }
                }
                break;
            }
            KeyCode::KeyN => {
                if let Some(agent_id) = runtime_index.agent_for_terminal(active_id) {
                    app_commands.write(AppCommand::Task(AppTaskCommand::ConsumeNext { agent_id }));
                }
                break;
            }
            _ => {}
        }
    }
}

/// Converts a raw Bevy keyboard event into the terminal command NeoZeus should send to the PTY.
///
/// Control chords are translated first through [`ctrl_sequence`], because terminals expect the ASCII
/// control bytes rather than the printable character. Special navigation keys are then mapped to the
/// conventional escape sequences, and finally ordinary text input is emitted as `InputText`. Any
/// event involving unsupported modifier combinations is dropped.
pub(crate) fn keyboard_input_to_terminal_command(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> Option<TerminalCommand> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if event.state != ButtonState::Pressed {
        return None;
    }

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
                Key::Character(text) if !text.is_empty() => {
                    Some(TerminalCommand::InputText(text.to_string()))
                }
                Key::Space => Some(TerminalCommand::InputText(" ".into())),
                _ => None,
            }),
    }
}

/// Maps letter keys to the control-byte sequences terminals conventionally expect.
///
/// This covers the classic ASCII control range (`Ctrl+A` through `Ctrl+Z`) and intentionally returns
/// string slices because the rest of the input pipeline already sends terminal events as strings.
pub(crate) fn ctrl_sequence(key_code: KeyCode) -> Option<&'static str> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    match key_code {
        KeyCode::KeyA => Some("\u{1}"),
        KeyCode::KeyB => Some("\u{2}"),
        KeyCode::KeyC => Some("\u{3}"),
        KeyCode::KeyD => Some("\u{4}"),
        KeyCode::KeyE => Some("\u{5}"),
        KeyCode::KeyF => Some("\u{6}"),
        KeyCode::KeyG => Some("\u{7}"),
        KeyCode::KeyH => Some("\u{8}"),
        KeyCode::KeyI => Some("\u{9}"),
        KeyCode::KeyJ => Some("\n"),
        KeyCode::KeyK => Some("\u{b}"),
        KeyCode::KeyL => Some("\u{c}"),
        KeyCode::KeyM => Some("\r"),
        KeyCode::KeyN => Some("\u{e}"),
        KeyCode::KeyO => Some("\u{f}"),
        KeyCode::KeyP => Some("\u{10}"),
        KeyCode::KeyQ => Some("\u{11}"),
        KeyCode::KeyR => Some("\u{12}"),
        KeyCode::KeyS => Some("\u{13}"),
        KeyCode::KeyT => Some("\u{14}"),
        KeyCode::KeyU => Some("\u{15}"),
        KeyCode::KeyV => Some("\u{16}"),
        KeyCode::KeyW => Some("\u{17}"),
        KeyCode::KeyX => Some("\u{18}"),
        KeyCode::KeyY => Some("\u{19}"),
        KeyCode::KeyZ => Some("\u{1a}"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::sync_primary_selection_from_ui_text_selection;
    #[cfg(target_os = "linux")]
    use super::{read_linux_primary_selection_text_with, write_linux_primary_selection_text_with};
    use crate::terminals::TerminalManager;
    use crate::text_selection::{
        AgentListTextSelectionState, PrimarySelectionSource, PrimarySelectionState,
        TerminalTextSelectionState,
    };
    use bevy::{ecs::system::RunSystemOnce, prelude::World};

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_primary_selection_prefers_wayland_primary_over_clipboard_tools() {
        let mut calls = Vec::new();
        let text = read_linux_primary_selection_text_with(
            Some("wayland"),
            Some("wayland-1"),
            Some(":0"),
            |program, args| {
                calls.push((
                    program.to_owned(),
                    args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
                ));
                (program == "wl-paste").then(|| b"primary text".to_vec())
            },
        );

        assert_eq!(text.as_deref(), Some("primary text"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "wl-paste");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_primary_selection_falls_back_to_xclip_when_wayland_primary_is_unavailable() {
        let mut calls = Vec::new();
        let text = read_linux_primary_selection_text_with(
            Some("wayland"),
            Some("wayland-1"),
            Some(":0"),
            |program, args| {
                calls.push((
                    program.to_owned(),
                    args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
                ));
                match program {
                    "wl-paste" => None,
                    "xclip" => Some(b"x11 primary".to_vec()),
                    _ => None,
                }
            },
        );

        assert_eq!(text.as_deref(), Some("x11 primary"));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "wl-paste");
        assert_eq!(calls[1].0, "xclip");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_primary_selection_write_prefers_wayland_before_x11_tools() {
        let mut calls = Vec::new();
        let success = write_linux_primary_selection_text_with(
            Some("wayland"),
            Some("wayland-1"),
            Some(":0"),
            "copied text",
            |program, args, text| {
                calls.push((
                    program.to_owned(),
                    args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
                    text.to_owned(),
                ));
                program == "wl-copy"
            },
        );

        assert!(success);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "wl-copy");
        assert_eq!(calls[0].2, "copied text");
    }

    #[test]
    fn primary_selection_sync_prefers_terminal_selection_over_agent_list_selection() {
        let mut world = World::default();
        let mut terminal_selection = TerminalTextSelectionState::default();
        let mut terminal_manager = TerminalManager::default();
        let (bridge, _mailbox) = crate::tests::test_bridge();
        let terminal_id = terminal_manager.create_terminal(bridge);
        terminal_selection.adopt_live_selection_owner(terminal_id);
        terminal_manager
            .get_mut(terminal_id)
            .expect("terminal should exist")
            .snapshot
            .surface = Some({
            let mut surface = crate::terminals::TerminalSurface::new(4, 1);
            surface.selected_text = Some("ABC".into());
            surface
        });
        let mut agent_list_selection = AgentListTextSelectionState::default();
        agent_list_selection.set_selection(
            crate::hud::AgentListRowKey::Agent(crate::agents::AgentId(1)),
            crate::hud::AgentListRowKey::Agent(crate::agents::AgentId(1)),
            "ROW".into(),
        );
        world.insert_resource(terminal_manager);
        world.insert_resource(terminal_selection);
        world.insert_resource(agent_list_selection);
        world.insert_resource(PrimarySelectionState::default());
        world.insert_resource(crate::text_selection::PrimarySelectionOwnerState::default());

        let _ = world.run_system_once(sync_primary_selection_from_ui_text_selection);

        let selection = world.resource::<PrimarySelectionState>();
        assert_eq!(
            selection.source(),
            Some(PrimarySelectionSource::Terminal(terminal_id))
        );
        assert_eq!(selection.text(), Some("ABC"));
    }

    #[test]
    fn primary_selection_sync_clears_when_ui_selections_are_empty() {
        let mut world = World::default();
        let mut primary_selection = PrimarySelectionState::default();
        assert!(primary_selection.set_agent_list_selection("ROW"));
        world.insert_resource(TerminalManager::default());
        world.insert_resource(TerminalTextSelectionState::default());
        world.insert_resource(AgentListTextSelectionState::default());
        world.insert_resource(primary_selection);
        world.insert_resource(crate::text_selection::PrimarySelectionOwnerState::default());

        let _ = world.run_system_once(sync_primary_selection_from_ui_text_selection);

        let selection = world.resource::<PrimarySelectionState>();
        assert_eq!(selection.source(), None);
        assert_eq!(selection.text(), None);
    }
}
