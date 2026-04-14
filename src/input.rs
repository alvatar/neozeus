mod clipboard_support;
mod direct_terminal_input;
mod modal_dialogs;
mod shortcut_bindings;
mod terminal_pointer;
mod terminal_selection_logic;

use clipboard_support::{stop_primary_selection_owner, write_linux_primary_selection_text};
use direct_terminal_input::terminal_is_interactive;
use shortcut_bindings::has_plain_modifiers;
use terminal_pointer::{
    terminal_page_scroll_rows, terminal_panel_contains_cursor, terminal_panel_screen_rect,
    topmost_terminal_panel_at_cursor,
};

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
    shared::linux_display::LinuxDisplayEnvironment,
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

#[cfg(test)]
pub(crate) fn should_spawn_terminal_globally(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    shortcut_bindings::should_spawn_terminal_globally(event, keys)
}

#[cfg(test)]
pub(crate) fn should_kill_active_terminal(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> bool {
    shortcut_bindings::should_kill_active_terminal(event, keys)
}

#[cfg(test)]
pub(crate) fn should_exit_application(event: &KeyboardInput, keys: &ButtonInput<KeyCode>) -> bool {
    shortcut_bindings::should_exit_application(event, keys)
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while shortcut logic lives in a submodule"
)]
pub(crate) fn handle_global_terminal_spawn_shortcut(
    messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    agent_catalog: Res<AgentCatalog>,
    selection: Option<Res<crate::hud::AgentListSelection>>,
    app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    redraws: MessageWriter<RequestRedraw>,
) {
    shortcut_bindings::handle_global_terminal_spawn_shortcut(
        messages,
        keys,
        primary_window,
        agent_catalog,
        selection,
        app_session,
        input_capture,
        redraws,
    )
}

pub(crate) fn is_plain_shortcut_key(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
    key_code: KeyCode,
) -> bool {
    shortcut_bindings::is_plain_shortcut_key(event, keys, key_code)
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while shortcut logic lives in a submodule"
)]
pub(crate) fn handle_terminal_lifecycle_shortcuts(
    messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    selection: Option<Res<crate::hud::AgentListSelection>>,
    app_commands: MessageWriter<AppCommand>,
    app_exits: MessageWriter<AppExit>,
    redraws: MessageWriter<RequestRedraw>,
) {
    shortcut_bindings::handle_terminal_lifecycle_shortcuts(
        messages,
        keys,
        app_session,
        input_capture,
        selection,
        app_commands,
        app_exits,
        redraws,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while terminal pointer logic lives in a submodule"
)]
pub(crate) fn focus_terminal_on_panel_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    runtime_index: Res<AgentRuntimeIndex>,
    app_commands: MessageWriter<AppCommand>,
) {
    terminal_pointer::focus_terminal_on_panel_click(
        mouse_buttons,
        primary_window,
        layout_state,
        app_session,
        input_capture,
        panels,
        runtime_index,
        app_commands,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while terminal pointer logic lives in a submodule"
)]
pub(crate) fn hide_terminal_on_background_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    focus_state: Res<TerminalFocusState>,
    app_commands: MessageWriter<AppCommand>,
) {
    terminal_pointer::hide_terminal_on_background_click(
        mouse_buttons,
        primary_window,
        layout_state,
        app_session,
        input_capture,
        panels,
        focus_state,
        app_commands,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while terminal pointer logic lives in a submodule"
)]
pub(crate) fn drag_terminal_view(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_motion: MessageReader<MouseMotion>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    presentation_store: Res<TerminalPresentationStore>,
    layout_state: Res<HudLayoutState>,
    view_state: ResMut<TerminalViewState>,
    pointer_state: ResMut<TerminalPointerState>,
) {
    terminal_pointer::drag_terminal_view(
        mouse_buttons,
        keys,
        mouse_motion,
        primary_window,
        terminal_manager,
        focus_state,
        presentation_store,
        layout_state,
        view_state,
        pointer_state,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while terminal pointer logic lives in a submodule"
)]
pub(crate) fn scroll_terminal_with_mouse_wheel(
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    input_capture: Res<HudInputCaptureState>,
    pointer_state: ResMut<TerminalPointerState>,
    mouse_wheel: MessageReader<MouseWheel>,
) {
    terminal_pointer::scroll_terminal_with_mouse_wheel(
        keys,
        primary_window,
        layout_state,
        terminal_manager,
        focus_state,
        input_capture,
        pointer_state,
        mouse_wheel,
    )
}

pub(crate) fn zoom_terminal_view(
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mouse_wheel: MessageReader<MouseWheel>,
    view_state: ResMut<TerminalViewState>,
) {
    terminal_pointer::zoom_terminal_view(keys, primary_window, mouse_wheel, view_state)
}

#[cfg(test)]
pub(crate) fn paste_into_create_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    clipboard_support::paste_into_create_agent_dialog(app_session, window, cursor, text)
}

#[cfg(test)]
pub(crate) fn paste_into_clone_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    clipboard_support::paste_into_clone_agent_dialog(app_session, window, cursor, text)
}

#[cfg(test)]
pub(crate) fn paste_into_rename_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    clipboard_support::paste_into_rename_agent_dialog(app_session, window, cursor, text)
}

#[cfg(test)]
pub(crate) fn paste_into_aegis_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    clipboard_support::paste_into_aegis_dialog(app_session, window, cursor, text)
}

#[cfg(test)]
pub(crate) fn paste_into_message_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    clipboard_support::paste_into_message_dialog(app_session, window, cursor, text)
}

#[cfg(test)]
pub(crate) fn paste_into_task_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    clipboard_support::paste_into_task_dialog(app_session, window, cursor, text)
}

#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input test helper surface stable while paste logic lives in a submodule"
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
    clipboard_support::paste_into_direct_input_terminal(
        window,
        cursor,
        layout_state,
        terminal_manager,
        focus_state,
        input_capture,
        panels,
        text,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while clipboard/paste logic lives in a submodule"
)]
pub(crate) fn handle_middle_click_paste(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    clipboard: Option<ResMut<EguiClipboard>>,
    redraws: MessageWriter<RequestRedraw>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
) {
    clipboard_support::handle_middle_click_paste(
        mouse_buttons,
        primary_window,
        layout_state,
        terminal_manager,
        focus_state,
        app_session,
        input_capture,
        clipboard,
        redraws,
        panels,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while text-selection logic lives in a submodule"
)]
pub(crate) fn handle_terminal_text_selection(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    active_terminal_content: Res<ActiveTerminalContentState>,
    terminal_text_selection: ResMut<TerminalTextSelectionState>,
    agent_list_text_selection: ResMut<crate::text_selection::AgentListTextSelectionState>,
    redraws: MessageWriter<RequestRedraw>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
) {
    terminal_selection_logic::handle_terminal_text_selection(
        mouse_buttons,
        primary_window,
        layout_state,
        terminal_manager,
        active_terminal_content,
        terminal_text_selection,
        agent_list_text_selection,
        redraws,
        panels,
    )
}

pub(crate) fn sync_primary_selection_from_ui_text_selection(
    terminal_manager: Res<TerminalManager>,
    terminal_text_selection: Res<TerminalTextSelectionState>,
    agent_list_text_selection: Res<crate::text_selection::AgentListTextSelectionState>,
    primary_selection: ResMut<PrimarySelectionState>,
    owner: ResMut<PrimarySelectionOwnerState>,
) {
    terminal_selection_logic::sync_primary_selection_from_ui_text_selection(
        terminal_manager,
        terminal_text_selection,
        agent_list_text_selection,
        primary_selection,
        owner,
    )
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn read_linux_primary_selection_text_with(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
    run_command: impl FnMut(&str, &[&str]) -> Option<Vec<u8>>,
) -> Option<String> {
    clipboard_support::read_linux_primary_selection_text_with(
        session_type,
        wayland_display,
        display,
        run_command,
    )
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn write_linux_primary_selection_text_with(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
    text: &str,
    run_command: impl FnMut(&str, &[&str], &str) -> bool,
) -> bool {
    clipboard_support::write_linux_primary_selection_text_with(
        session_type,
        wayland_display,
        display,
        text,
        run_command,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep crate::input entrypoints stable while direct terminal input logic lives in a submodule"
)]
pub(crate) fn handle_terminal_direct_input_keyboard(
    messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    app_session: ResMut<AppSessionState>,
    input_capture: ResMut<HudInputCaptureState>,
    redraws: MessageWriter<RequestRedraw>,
) {
    direct_terminal_input::handle_terminal_direct_input_keyboard(
        messages,
        keys,
        primary_window,
        terminal_manager,
        focus_state,
        app_session,
        input_capture,
        redraws,
    )
}

#[cfg(test)]
pub(crate) fn keyboard_input_to_terminal_command(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> Option<TerminalCommand> {
    direct_terminal_input::keyboard_input_to_terminal_command(event, keys)
}

#[cfg(test)]
pub(crate) fn ctrl_sequence(key_code: KeyCode) -> Option<&'static str> {
    direct_terminal_input::ctrl_sequence(key_code)
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
fn handle_text_editor_vertical_motion(
    editor: &mut crate::composer::TextEditorState,
    wrapped_visible_cols: Option<usize>,
    down: bool,
) -> bool {
    match (wrapped_visible_cols, down) {
        (Some(cols), true) => editor.move_down_wrapped(cols),
        (Some(cols), false) => editor.move_up_wrapped(cols),
        (None, true) => editor.move_down(),
        (None, false) => editor.move_up(),
    }
}

fn handle_text_editor_event(
    editor: &mut crate::composer::TextEditorState,
    event: &KeyboardInput,
    ctrl: bool,
    alt: bool,
    super_key: bool,
    wrapped_visible_cols: Option<usize>,
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
            KeyCode::KeyN => handle_text_editor_vertical_motion(editor, wrapped_visible_cols, true),
            KeyCode::KeyU => editor.kill_all(),
            KeyCode::KeyO => editor.open_line(),
            KeyCode::KeyP => {
                handle_text_editor_vertical_motion(editor, wrapped_visible_cols, false)
            }
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
            KeyCode::ArrowUp => {
                handle_text_editor_vertical_motion(editor, wrapped_visible_cols, false)
            }
            KeyCode::ArrowDown => {
                handle_text_editor_vertical_motion(editor, wrapped_visible_cols, true)
            }
            KeyCode::Home => editor.move_line_start(),
            KeyCode::End => editor.move_line_end(),
            KeyCode::Tab => editor.insert_text("\t"),
            _ => message_box_event_text(event).is_some_and(|text| editor.insert_text(&text)),
        }
    } else {
        false
    }
}

fn message_box_key_modifiers(keys: &ButtonInput<KeyCode>) -> modal_dialogs::KeyModifiers {
    let (ctrl, alt, super_key) = has_plain_modifiers(keys);
    modal_dialogs::KeyModifiers {
        ctrl,
        alt,
        super_key,
        shift: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
    }
}

fn current_clipboard_text(clipboard: Option<&mut EguiClipboard>) -> Option<String> {
    clipboard
        .and_then(EguiClipboard::get_text)
        .filter(|text| !text.is_empty())
}

fn write_emitted_commands(
    app_commands: &mut MessageWriter<AppCommand>,
    emitted_commands: Vec<AppCommand>,
) {
    for command in emitted_commands {
        app_commands.write(command);
    }
}

fn redraw_if_needed(needs_redraw: bool, redraws: &mut MessageWriter<RequestRedraw>) {
    if needs_redraw {
        redraws.write(RequestRedraw);
    }
}

fn run_modal_handler_loop(
    messages: &mut MessageReader<KeyboardInput>,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
    mut handle_event: impl FnMut(&KeyboardInput, &mut Vec<AppCommand>) -> modal_dialogs::ModalKeyResult,
) {
    let mut needs_redraw = false;
    let mut emitted_commands = Vec::new();
    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let outcome = handle_event(event, &mut emitted_commands);
        needs_redraw |= outcome.needs_redraw;
        if outcome.stop {
            break;
        }
    }
    write_emitted_commands(app_commands, emitted_commands);
    redraw_if_needed(needs_redraw, redraws);
}

fn handle_reset_dialog_events(
    app_session: &mut AppSessionState,
    messages: &mut MessageReader<KeyboardInput>,
    modifiers: modal_dialogs::KeyModifiers,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> bool {
    if !app_session.reset_dialog.visible {
        return false;
    }
    run_modal_handler_loop(
        messages,
        app_commands,
        redraws,
        |event, emitted_commands| {
            modal_dialogs::handle_reset_dialog_key(app_session, event, modifiers, emitted_commands)
        },
    );
    true
}

fn handle_aegis_dialog_events(
    app_session: &mut AppSessionState,
    primary_window: &Window,
    messages: &mut MessageReader<KeyboardInput>,
    modifiers: modal_dialogs::KeyModifiers,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> bool {
    if !app_session.aegis_dialog.visible {
        return false;
    }
    let visible_cols = crate::composer::aegis_visible_cols(primary_window);
    run_modal_handler_loop(
        messages,
        app_commands,
        redraws,
        |event, emitted_commands| {
            modal_dialogs::handle_aegis_dialog_key(
                app_session,
                event,
                modifiers,
                visible_cols,
                emitted_commands,
            )
        },
    );
    true
}

fn handle_rename_dialog_events(
    app_session: &mut AppSessionState,
    messages: &mut MessageReader<KeyboardInput>,
    modifiers: modal_dialogs::KeyModifiers,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> bool {
    if !app_session.rename_agent_dialog.visible {
        return false;
    }
    run_modal_handler_loop(
        messages,
        app_commands,
        redraws,
        |event, emitted_commands| {
            modal_dialogs::handle_rename_agent_dialog_key(
                app_session,
                event,
                modifiers,
                emitted_commands,
            )
        },
    );
    true
}

fn handle_clone_dialog_events(
    app_session: &mut AppSessionState,
    messages: &mut MessageReader<KeyboardInput>,
    modifiers: modal_dialogs::KeyModifiers,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> bool {
    if !app_session.clone_agent_dialog.visible {
        return false;
    }
    run_modal_handler_loop(
        messages,
        app_commands,
        redraws,
        |event, emitted_commands| {
            modal_dialogs::handle_clone_agent_dialog_key(
                app_session,
                event,
                modifiers,
                emitted_commands,
            )
        },
    );
    true
}

fn handle_create_dialog_events(
    app_session: &mut AppSessionState,
    messages: &mut MessageReader<KeyboardInput>,
    modifiers: modal_dialogs::KeyModifiers,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> bool {
    if !app_session.create_agent_dialog.visible {
        return false;
    }
    run_modal_handler_loop(
        messages,
        app_commands,
        redraws,
        |event, emitted_commands| {
            modal_dialogs::handle_create_agent_dialog_key(
                app_session,
                event,
                modifiers,
                emitted_commands,
            )
        },
    );
    true
}

fn sync_message_editor_clipboard_ingress(
    app_session: &AppSessionState,
    clipboard: Option<&mut EguiClipboard>,
    clipboard_ingress: &mut modal_dialogs::MessageDialogClipboardIngressState,
) {
    let current_clipboard_text = current_clipboard_text(clipboard);
    clipboard_ingress.sync_visibility(
        app_session.composer.message_editor.visible,
        current_clipboard_text.as_deref(),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "message editor path needs clipboard ingress, visible cols, commands, redraws, and session state together"
)]
fn handle_message_editor_events(
    app_session: &mut AppSessionState,
    primary_window: &Window,
    messages: &mut MessageReader<KeyboardInput>,
    modifiers: modal_dialogs::KeyModifiers,
    clipboard: Option<&mut EguiClipboard>,
    clipboard_ingress: &mut modal_dialogs::MessageDialogClipboardIngressState,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> bool {
    if !app_session.composer.message_editor.visible {
        return false;
    }
    let visible_cols = crate::composer::message_box_visible_cols(primary_window);
    let mut clipboard = clipboard;
    run_modal_handler_loop(
        messages,
        app_commands,
        redraws,
        |event, emitted_commands| {
            let current_clipboard_text = current_clipboard_text(clipboard.as_deref_mut());
            clipboard_ingress.handle_key(
                app_session,
                event,
                modifiers,
                visible_cols,
                current_clipboard_text.as_deref(),
                emitted_commands,
            )
        },
    );
    true
}

fn handle_task_editor_events(
    app_session: &mut AppSessionState,
    primary_window: &Window,
    messages: &mut MessageReader<KeyboardInput>,
    modifiers: modal_dialogs::KeyModifiers,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
) -> bool {
    if !app_session.composer.task_editor.visible {
        return false;
    }
    let visible_cols = crate::composer::task_dialog_visible_cols(primary_window);
    run_modal_handler_loop(
        messages,
        app_commands,
        redraws,
        |event, emitted_commands| {
            modal_dialogs::handle_task_dialog_key(
                app_session,
                event,
                modifiers,
                visible_cols,
                emitted_commands,
            )
        },
    );
    true
}

#[allow(
    clippy::too_many_arguments,
    reason = "plain terminal shortcuts still need runtime selection, dialogs, commands, and redraws together"
)]
fn handle_plain_terminal_shortcuts(
    app_session: &mut AppSessionState,
    messages: &mut MessageReader<KeyboardInput>,
    modifiers: modal_dialogs::KeyModifiers,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    runtime_index: &AgentRuntimeIndex,
    agent_catalog: &AgentCatalog,
    aegis_policy: &AegisPolicyStore,
    app_commands: &mut MessageWriter<AppCommand>,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    let Some(active_id) = focus_state.active_id() else {
        return;
    };
    let Some(active_terminal) = terminal_manager.get(active_id) else {
        return;
    };
    if !terminal_is_interactive(&active_terminal.snapshot.runtime) {
        return;
    }
    let page_rows = terminal_page_scroll_rows(terminal_manager, active_id);
    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        if modifiers.ctrl && !modifiers.alt && !modifiers.super_key {
            match event.key_code {
                KeyCode::KeyT => {
                    if let Some(agent_id) = runtime_index.agent_for_terminal(active_id) {
                        app_commands
                            .write(AppCommand::Task(AppTaskCommand::ClearDone { agent_id }));
                    }
                    break;
                }
                KeyCode::KeyV => {
                    if let Some(terminal) = terminal_manager.get(active_id) {
                        terminal
                            .bridge
                            .send(TerminalCommand::ScrollDisplay(-page_rows));
                    }
                    break;
                }
                _ => {}
            }
        }

        if modifiers.alt
            && !modifiers.ctrl
            && !modifiers.super_key
            && event.key_code == KeyCode::KeyV
        {
            if let Some(terminal) = terminal_manager.get(active_id) {
                terminal
                    .bridge
                    .send(TerminalCommand::ScrollDisplay(page_rows));
            }
            break;
        }

        if modifiers.ctrl || modifiers.alt || modifiers.super_key || modifiers.shift {
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
            KeyCode::KeyP => {
                if let Some(agent_id) = runtime_index.agent_for_terminal(active_id) {
                    app_commands.write(AppCommand::Agent(AppAgentCommand::TogglePaused(agent_id)));
                }
                break;
            }
            _ => {}
        }
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
    aegis_policy: Res<AegisPolicyStore>,
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
    if input_capture.direct_input_terminal.is_some() {
        return;
    }

    let modifiers = message_box_key_modifiers(&keys);

    if handle_reset_dialog_events(
        &mut app_session,
        &mut messages,
        modifiers,
        &mut app_commands,
        &mut redraws,
    ) {
        return;
    }
    if handle_aegis_dialog_events(
        &mut app_session,
        &primary_window,
        &mut messages,
        modifiers,
        &mut app_commands,
        &mut redraws,
    ) {
        return;
    }
    if handle_rename_dialog_events(
        &mut app_session,
        &mut messages,
        modifiers,
        &mut app_commands,
        &mut redraws,
    ) {
        return;
    }
    if handle_clone_dialog_events(
        &mut app_session,
        &mut messages,
        modifiers,
        &mut app_commands,
        &mut redraws,
    ) {
        return;
    }
    if handle_create_dialog_events(
        &mut app_session,
        &mut messages,
        modifiers,
        &mut app_commands,
        &mut redraws,
    ) {
        return;
    }

    sync_message_editor_clipboard_ingress(
        &app_session,
        clipboard.as_deref_mut(),
        &mut clipboard_ingress,
    );
    if handle_message_editor_events(
        &mut app_session,
        &primary_window,
        &mut messages,
        modifiers,
        clipboard.as_deref_mut(),
        &mut clipboard_ingress,
        &mut app_commands,
        &mut redraws,
    ) {
        return;
    }
    if handle_task_editor_events(
        &mut app_session,
        &primary_window,
        &mut messages,
        modifiers,
        &mut app_commands,
        &mut redraws,
    ) {
        return;
    }

    handle_plain_terminal_shortcuts(
        &mut app_session,
        &mut messages,
        modifiers,
        &_terminal_manager,
        &focus_state,
        &runtime_index,
        &agent_catalog,
        &aegis_policy,
        &mut app_commands,
        &mut redraws,
    );
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
