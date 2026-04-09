use crate::{
    app::{
        AegisDialogField, AppCommand, AppSessionState, CloneAgentDialogField, ComposerCommand,
        CreateAgentDialogField, RenameAgentDialogField,
    },
    composer::{MessageDialogFocus, TaskDialogFocus},
};
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::KeyCode,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) struct KeyModifiers {
    pub(super) ctrl: bool,
    pub(super) alt: bool,
    pub(super) super_key: bool,
    pub(super) shift: bool,
}

impl KeyModifiers {
    fn plain(self) -> bool {
        !(self.ctrl || self.alt || self.super_key)
    }

    fn ctrl_only(self) -> bool {
        self.ctrl && !self.alt && !self.super_key
    }

    fn alt_only(self) -> bool {
        self.alt && !self.ctrl && !self.super_key
    }
}

// Some external text tools update the clipboard and then emit a bogus trailing Escape instead of
// delivering a real paste event. Keep the compatibility window narrow so only that immediate tail
// noise is swallowed; later manual Escape presses must still cancel the dialog normally.
const MESSAGE_DIALOG_CLIPBOARD_ESCAPE_GRACE: Duration = Duration::from_millis(75);

#[derive(Clone, Copy, Default)]
pub(super) struct ModalKeyResult {
    pub(super) needs_redraw: bool,
    pub(super) stop: bool,
}

impl ModalKeyResult {
    fn redraw() -> Self {
        Self {
            needs_redraw: true,
            stop: false,
        }
    }

    fn redraw_and_stop() -> Self {
        Self {
            needs_redraw: true,
            stop: true,
        }
    }

    fn stop() -> Self {
        Self {
            needs_redraw: false,
            stop: true,
        }
    }
}

enum DialogShellAction {
    Escape,
    Tab { reverse: bool },
    None,
}

fn dialog_shell_action(event: &KeyboardInput, modifiers: KeyModifiers) -> DialogShellAction {
    if event.key_code == KeyCode::Escape {
        return DialogShellAction::Escape;
    }

    if modifiers.plain() && event.key_code == KeyCode::Tab {
        return DialogShellAction::Tab {
            reverse: modifiers.shift,
        };
    }

    DialogShellAction::None
}

fn finish_dialog_change(
    changed: bool,
    clear_error: bool,
    error: &mut Option<String>,
) -> ModalKeyResult {
    if changed {
        if clear_error {
            *error = None;
        }
        ModalKeyResult::redraw()
    } else {
        ModalKeyResult::default()
    }
}

/// Handles one key press while the create-agent dialog owns keyboard capture.
pub(super) fn handle_create_agent_dialog_key(
    app_session: &mut AppSessionState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    emitted_commands: &mut Vec<AppCommand>,
) -> ModalKeyResult {
    match dialog_shell_action(event, modifiers) {
        DialogShellAction::Escape => {
            app_session.create_agent_dialog.close();
            return ModalKeyResult::redraw_and_stop();
        }
        DialogShellAction::Tab { reverse } => {
            app_session.create_agent_dialog.cycle_focus(reverse);
            return ModalKeyResult::redraw();
        }
        DialogShellAction::None => {}
    }

    let (changed, clear_error) = match app_session.create_agent_dialog.focus {
        CreateAgentDialogField::Name => (
            handle_agent_name_field_event(
                &mut app_session.create_agent_dialog.name_field,
                event,
                modifiers,
            ),
            true,
        ),
        CreateAgentDialogField::Kind => {
            if modifiers.plain() && event.key_code == KeyCode::Space {
                app_session
                    .create_agent_dialog
                    .set_kind(app_session.create_agent_dialog.kind.next());
                (true, false)
            } else {
                (false, false)
            }
        }
        CreateAgentDialogField::StartingFolder => {
            if modifiers.ctrl_only() && event.key_code == KeyCode::Space {
                (
                    app_session
                        .create_agent_dialog
                        .cwd_field
                        .start_or_cycle_completion(modifiers.shift),
                    true,
                )
            } else if modifiers.plain() && event.key_code == KeyCode::Enter {
                (
                    app_session
                        .create_agent_dialog
                        .cwd_field
                        .accept_completion(),
                    true,
                )
            } else {
                let changed = app_session
                    .create_agent_dialog
                    .cwd_field
                    .mutate_text(|field| handle_text_field_event(field, event, modifiers));
                (changed, true)
            }
        }
        CreateAgentDialogField::CreateButton => {
            if modifiers.plain() && matches!(event.key_code, KeyCode::Enter | KeyCode::Space) {
                if let Some(command) = app_session.create_agent_dialog.build_create_command() {
                    emitted_commands.push(command);
                }
                (true, false)
            } else {
                (false, false)
            }
        }
    };

    finish_dialog_change(
        changed,
        clear_error,
        &mut app_session.create_agent_dialog.error,
    )
}

/// Handles one key press while the clone-agent dialog owns keyboard capture.
pub(super) fn handle_clone_agent_dialog_key(
    app_session: &mut AppSessionState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    emitted_commands: &mut Vec<AppCommand>,
) -> ModalKeyResult {
    match dialog_shell_action(event, modifiers) {
        DialogShellAction::Escape => {
            app_session.clone_agent_dialog.close();
            return ModalKeyResult::redraw_and_stop();
        }
        DialogShellAction::Tab { reverse } => {
            app_session.clone_agent_dialog.cycle_focus(reverse);
            return ModalKeyResult::redraw();
        }
        DialogShellAction::None => {}
    }

    let (changed, clear_error) = match app_session.clone_agent_dialog.focus {
        CloneAgentDialogField::Name => (
            handle_agent_name_field_event(
                &mut app_session.clone_agent_dialog.name_field,
                event,
                modifiers,
            ),
            true,
        ),
        CloneAgentDialogField::Workdir => {
            if modifiers.plain() && matches!(event.key_code, KeyCode::Enter | KeyCode::Space) {
                app_session.clone_agent_dialog.toggle_workdir();
                (true, false)
            } else {
                (false, false)
            }
        }
        CloneAgentDialogField::CloneButton => {
            if modifiers.plain() && matches!(event.key_code, KeyCode::Enter | KeyCode::Space) {
                if let Some(command) = app_session.clone_agent_dialog.build_clone_command() {
                    emitted_commands.push(command);
                }
                (true, false)
            } else {
                (false, false)
            }
        }
    };

    finish_dialog_change(
        changed,
        clear_error,
        &mut app_session.clone_agent_dialog.error,
    )
}

/// Handles one key press while the rename-agent dialog owns keyboard capture.
pub(super) fn handle_rename_agent_dialog_key(
    app_session: &mut AppSessionState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    emitted_commands: &mut Vec<AppCommand>,
) -> ModalKeyResult {
    match dialog_shell_action(event, modifiers) {
        DialogShellAction::Escape => {
            app_session.rename_agent_dialog.close();
            return ModalKeyResult::redraw_and_stop();
        }
        DialogShellAction::Tab { reverse } => {
            app_session.rename_agent_dialog.cycle_focus(reverse);
            return ModalKeyResult::redraw();
        }
        DialogShellAction::None => {}
    }

    let (changed, clear_error) = match app_session.rename_agent_dialog.focus {
        RenameAgentDialogField::Name => (
            handle_agent_name_field_event(
                &mut app_session.rename_agent_dialog.name_field,
                event,
                modifiers,
            ),
            true,
        ),
        RenameAgentDialogField::RenameButton => {
            if modifiers.plain() && matches!(event.key_code, KeyCode::Enter | KeyCode::Space) {
                if let Some(command) = app_session.rename_agent_dialog.build_rename_command() {
                    emitted_commands.push(command);
                }
                (true, false)
            } else {
                (false, false)
            }
        }
    };

    finish_dialog_change(
        changed,
        clear_error,
        &mut app_session.rename_agent_dialog.error,
    )
}

pub(super) fn handle_aegis_dialog_key(
    app_session: &mut AppSessionState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    wrapped_visible_cols: usize,
    emitted_commands: &mut Vec<AppCommand>,
) -> ModalKeyResult {
    match dialog_shell_action(event, modifiers) {
        DialogShellAction::Escape => {
            app_session.aegis_dialog.close();
            return ModalKeyResult::redraw_and_stop();
        }
        DialogShellAction::Tab { reverse } => {
            app_session.aegis_dialog.cycle_focus(reverse);
            return ModalKeyResult::redraw();
        }
        DialogShellAction::None => {}
    }

    let (changed, clear_error) = match app_session.aegis_dialog.focus {
        AegisDialogField::Prompt => (
            super::handle_text_editor_event(
                &mut app_session.aegis_dialog.prompt_editor,
                event,
                modifiers.ctrl,
                modifiers.alt,
                modifiers.super_key,
                Some(wrapped_visible_cols),
            ),
            true,
        ),
        AegisDialogField::EnableButton => {
            if modifiers.plain() && matches!(event.key_code, KeyCode::Enter | KeyCode::Space) {
                if let Some(command) = app_session.aegis_dialog.build_enable_command() {
                    emitted_commands.push(command);
                }
                (true, false)
            } else {
                (false, false)
            }
        }
    };

    finish_dialog_change(changed, clear_error, &mut app_session.aegis_dialog.error)
}

/// Message-box-local compatibility state for clipboard-backed text ingress.
#[derive(Default)]
pub(crate) struct MessageDialogClipboardIngressState {
    was_visible: bool,
    last_clipboard_text: Option<String>,
    modifier_prelude_active: bool,
    clipboard_changed_since_modifier_prelude: bool,
    suppress_escape_until: Option<Instant>,
}

impl MessageDialogClipboardIngressState {
    pub(super) fn sync_visibility(&mut self, visible: bool, current_clipboard_text: Option<&str>) {
        if !visible {
            *self = Self::default();
            return;
        }
        if !self.was_visible {
            self.was_visible = true;
            self.last_clipboard_text = current_clipboard_text.map(str::to_owned);
            self.modifier_prelude_active = false;
            self.clipboard_changed_since_modifier_prelude = false;
            self.suppress_escape_until = None;
        }
    }

    pub(super) fn handle_key(
        &mut self,
        app_session: &mut AppSessionState,
        event: &KeyboardInput,
        modifiers: KeyModifiers,
        wrapped_visible_cols: usize,
        current_clipboard_text: Option<&str>,
        emitted_commands: &mut Vec<AppCommand>,
    ) -> ModalKeyResult {
        if message_dialog_requests_clipboard_paste(event, modifiers) {
            return current_clipboard_text.map_or(ModalKeyResult::stop(), |text| {
                handle_message_dialog_clipboard_text(app_session, text)
            });
        }

        match self.handle_event(event, current_clipboard_text, Instant::now()) {
            ClipboardIngressDecision::InsertClipboard => current_clipboard_text
                .map_or(ModalKeyResult::stop(), |text| {
                    handle_message_dialog_clipboard_text(app_session, text)
                }),
            ClipboardIngressDecision::Swallow => ModalKeyResult::stop(),
            ClipboardIngressDecision::None => handle_message_dialog_key(
                app_session,
                event,
                modifiers,
                wrapped_visible_cols,
                emitted_commands,
            ),
        }
    }

    fn handle_event(
        &mut self,
        event: &KeyboardInput,
        current_clipboard_text: Option<&str>,
        now: Instant,
    ) -> ClipboardIngressDecision {
        if let Some(until) = self.suppress_escape_until {
            if now <= until && event.key_code == KeyCode::Escape {
                self.suppress_escape_until = None;
                return ClipboardIngressDecision::Swallow;
            }
            if now > until {
                self.suppress_escape_until = None;
            }
        }

        let clipboard_changed =
            current_clipboard_text.map(str::to_owned) != self.last_clipboard_text;
        if clipboard_changed {
            self.last_clipboard_text = current_clipboard_text.map(str::to_owned);
        }

        if self.modifier_prelude_active && clipboard_changed {
            self.clipboard_changed_since_modifier_prelude = true;
        }

        if is_modifier_key(event.key_code) {
            self.modifier_prelude_active = true;
            self.clipboard_changed_since_modifier_prelude |= clipboard_changed;
            return ClipboardIngressDecision::None;
        }

        if event.key_code == KeyCode::Escape
            && self.modifier_prelude_active
            && self.clipboard_changed_since_modifier_prelude
            && current_clipboard_text.is_some()
        {
            self.modifier_prelude_active = false;
            self.clipboard_changed_since_modifier_prelude = false;
            self.suppress_escape_until = Some(now + MESSAGE_DIALOG_CLIPBOARD_ESCAPE_GRACE);
            return ClipboardIngressDecision::InsertClipboard;
        }

        self.modifier_prelude_active = false;
        self.clipboard_changed_since_modifier_prelude = false;
        ClipboardIngressDecision::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardIngressDecision {
    None,
    InsertClipboard,
    Swallow,
}

fn is_modifier_key(key_code: KeyCode) -> bool {
    matches!(
        key_code,
        KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
    )
}

/// Returns whether this message-dialog key event explicitly requests clipboard paste.
pub(super) fn message_dialog_requests_clipboard_paste(
    event: &KeyboardInput,
    modifiers: KeyModifiers,
) -> bool {
    if modifiers.alt || modifiers.super_key {
        return false;
    }
    matches!(event.key_code, KeyCode::Paste)
        || matches!(event.logical_key, Key::Paste)
        || (modifiers.ctrl
            && (matches!(&event.logical_key, Key::Character(text) if text.eq_ignore_ascii_case("v"))
                || event
                    .text
                    .as_ref()
                    .is_some_and(|text| text.eq_ignore_ascii_case("v"))))
        || (modifiers.shift && !modifiers.ctrl && event.key_code == KeyCode::Insert)
}

/// Applies clipboard text ingress to the message dialog when the editor has focus.
pub(super) fn handle_message_dialog_clipboard_text(
    app_session: &mut AppSessionState,
    text: &str,
) -> ModalKeyResult {
    if !matches!(
        app_session.composer.message_dialog_focus,
        MessageDialogFocus::Editor
    ) {
        return ModalKeyResult::default();
    }
    ModalKeyResult {
        needs_redraw: app_session.composer.message_editor.insert_text(text),
        stop: true,
    }
}

/// Handles one key press while the message dialog owns keyboard capture.
pub(super) fn handle_message_dialog_key(
    app_session: &mut AppSessionState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    wrapped_visible_cols: usize,
    emitted_commands: &mut Vec<AppCommand>,
) -> ModalKeyResult {
    if modifiers.ctrl_only() && event.key_code == KeyCode::KeyS {
        if !app_session.composer.message_editor.text.trim().is_empty() {
            emitted_commands.push(AppCommand::Composer(ComposerCommand::Submit));
        }
        return ModalKeyResult::stop();
    }

    match dialog_shell_action(event, modifiers) {
        DialogShellAction::Escape => {
            emitted_commands.push(AppCommand::Composer(ComposerCommand::Cancel));
            return ModalKeyResult::stop();
        }
        DialogShellAction::Tab { reverse } => {
            app_session.composer.cycle_message_dialog_focus(reverse);
            return ModalKeyResult::redraw();
        }
        DialogShellAction::None => {}
    }

    match app_session.composer.message_dialog_focus {
        MessageDialogFocus::Editor => handle_text_editor_result(super::handle_text_editor_event(
            &mut app_session.composer.message_editor,
            event,
            modifiers.ctrl,
            modifiers.alt,
            modifiers.super_key,
            Some(wrapped_visible_cols),
        )),
        MessageDialogFocus::AppendButton => {
            if modifiers.plain() && matches!(event.key_code, KeyCode::Enter | KeyCode::Space) {
                if let Some(command) = app_session
                    .composer
                    .message_box_action_command(crate::composer::MessageBoxAction::AppendTask)
                {
                    emitted_commands.push(command);
                }
                ModalKeyResult::redraw()
            } else {
                ModalKeyResult::default()
            }
        }
        MessageDialogFocus::PrependButton => {
            if modifiers.plain() && matches!(event.key_code, KeyCode::Enter | KeyCode::Space) {
                if let Some(command) = app_session
                    .composer
                    .message_box_action_command(crate::composer::MessageBoxAction::PrependTask)
                {
                    emitted_commands.push(command);
                }
                ModalKeyResult::redraw()
            } else {
                ModalKeyResult::default()
            }
        }
    }
}

/// Handles one key press while the task dialog owns keyboard capture.
pub(super) fn handle_task_dialog_key(
    app_session: &mut AppSessionState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    wrapped_visible_cols: usize,
    emitted_commands: &mut Vec<AppCommand>,
) -> ModalKeyResult {
    if modifiers.ctrl_only() && event.key_code == KeyCode::KeyT {
        if let Some(agent_id) = app_session.composer.current_agent() {
            emitted_commands.push(AppCommand::Task(crate::app::TaskCommand::ClearDone {
                agent_id,
            }));
        }
        return ModalKeyResult::default();
    }

    match dialog_shell_action(event, modifiers) {
        DialogShellAction::Escape => {
            emitted_commands.push(AppCommand::Composer(ComposerCommand::Submit));
            return ModalKeyResult::stop();
        }
        DialogShellAction::Tab { reverse } => {
            app_session.composer.cycle_task_dialog_focus(reverse);
            return ModalKeyResult::redraw();
        }
        DialogShellAction::None => {}
    }

    match app_session.composer.task_dialog_focus {
        TaskDialogFocus::Editor => handle_text_editor_result(super::handle_text_editor_event(
            &mut app_session.composer.task_editor,
            event,
            modifiers.ctrl,
            modifiers.alt,
            modifiers.super_key,
            Some(wrapped_visible_cols),
        )),
        TaskDialogFocus::ClearDoneButton => {
            if modifiers.plain() && matches!(event.key_code, KeyCode::Enter | KeyCode::Space) {
                if let Some(command) = app_session
                    .composer
                    .task_dialog_action_command(crate::composer::TaskDialogAction::ClearDone)
                {
                    emitted_commands.push(command);
                }
                ModalKeyResult::redraw()
            } else {
                ModalKeyResult::default()
            }
        }
    }
}

fn handle_text_editor_result(changed: bool) -> ModalKeyResult {
    if changed {
        ModalKeyResult::redraw()
    } else {
        ModalKeyResult::default()
    }
}

/// Applies one keyboard event to a single-line HUD text field.
///
/// This keeps the form-control behavior intentionally small: no multiline editing, no selection,
/// and no generic `Tab` handling because dialogs reserve `Tab` for focus traversal.
fn handle_text_field_event_impl(
    field: &mut crate::app::TextFieldState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    uppercase_inserted_text: bool,
) -> bool {
    if modifiers.ctrl_only() {
        match event.key_code {
            KeyCode::KeyA => field.move_start(),
            KeyCode::KeyB => field.move_left(),
            KeyCode::KeyD => field.delete_forward_char(),
            KeyCode::KeyE => field.move_end(),
            KeyCode::KeyF => field.move_right(),
            KeyCode::KeyH => field.delete_backward_char(),
            KeyCode::KeyK => field.kill_to_end(),
            KeyCode::KeyU => field.kill_all(),
            _ => false,
        }
    } else if modifiers.alt_only() {
        match event.key_code {
            KeyCode::Backspace => field.kill_word_backward(),
            KeyCode::KeyB => field.move_word_backward(),
            KeyCode::KeyD => field.kill_word_forward(),
            KeyCode::KeyF => field.move_word_forward(),
            _ => false,
        }
    } else if modifiers.plain() {
        match event.key_code {
            KeyCode::Backspace => field.delete_backward_char(),
            KeyCode::Delete => field.delete_forward_char(),
            KeyCode::ArrowLeft => field.move_left(),
            KeyCode::ArrowRight => field.move_right(),
            KeyCode::Home => field.move_start(),
            KeyCode::End => field.move_end(),
            KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::Enter | KeyCode::Tab => false,
            _ => super::message_box_event_text(event)
                .filter(|text| !text.contains(['\n', '\r', '\t']))
                .map(|text| {
                    if uppercase_inserted_text {
                        crate::agents::uppercase_agent_label_text(&text)
                    } else {
                        text
                    }
                })
                .is_some_and(|text| field.insert_text(&text)),
        }
    } else {
        false
    }
}

fn handle_text_field_event(
    field: &mut crate::app::TextFieldState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
) -> bool {
    handle_text_field_event_impl(field, event, modifiers, false)
}

fn handle_agent_name_field_event(
    field: &mut crate::app::TextFieldState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
) -> bool {
    handle_text_field_event_impl(field, event, modifiers, true)
}

#[cfg(test)]
mod tests {
    use super::{
        handle_message_dialog_clipboard_text, handle_message_dialog_key,
        message_dialog_requests_clipboard_paste, ClipboardIngressDecision, KeyModifiers,
        MessageDialogClipboardIngressState, MESSAGE_DIALOG_CLIPBOARD_ESCAPE_GRACE,
    };
    use crate::{
        agents::AgentId,
        app::{AppCommand, AppSessionState, ComposerCommand},
        composer::MessageDialogFocus,
    };
    use bevy::{
        ecs::entity::Entity,
        input::{
            keyboard::{Key, KeyboardInput},
            ButtonState,
        },
        prelude::KeyCode,
    };
    use std::time::{Duration, Instant};

    fn pressed(key_code: KeyCode, logical_key: Key, text: Option<&str>) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    fn pressed_ctrl_v() -> KeyboardInput {
        KeyboardInput {
            key_code: KeyCode::KeyV,
            logical_key: Key::Character("v".into()),
            state: ButtonState::Pressed,
            text: Some("v".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    #[test]
    fn clipboard_ingress_state_uses_changed_clipboard_on_modifier_prelude_then_escape() {
        let mut state = MessageDialogClipboardIngressState::default();
        let start = Instant::now();
        state.sync_visibility(true, Some("before"));

        assert_eq!(
            state.handle_event(
                &pressed(KeyCode::ControlLeft, Key::Control, None),
                Some("dictated text"),
                start,
            ),
            ClipboardIngressDecision::None
        );
        assert_eq!(
            state.handle_event(
                &pressed(KeyCode::Escape, Key::Escape, Some("\u{1b}")),
                Some("dictated text"),
                start + Duration::from_millis(10),
            ),
            ClipboardIngressDecision::InsertClipboard
        );
        assert_eq!(
            state.handle_event(
                &pressed(KeyCode::Escape, Key::Escape, Some("\u{1b}")),
                Some("dictated text"),
                start + Duration::from_millis(20),
            ),
            ClipboardIngressDecision::Swallow
        );
    }

    #[test]
    fn clipboard_ingress_state_does_not_trigger_without_clipboard_change() {
        let mut state = MessageDialogClipboardIngressState::default();
        state.sync_visibility(true, Some("before"));

        assert_eq!(
            state.handle_event(
                &pressed(KeyCode::ControlLeft, Key::Control, None),
                Some("before"),
                Instant::now(),
            ),
            ClipboardIngressDecision::None
        );
        assert_eq!(
            state.handle_event(
                &pressed(KeyCode::Escape, Key::Escape, Some("\u{1b}")),
                Some("before"),
                Instant::now(),
            ),
            ClipboardIngressDecision::None
        );
    }

    #[test]
    fn clipboard_ingress_state_lets_manual_escape_close_after_grace_window() {
        let mut state = MessageDialogClipboardIngressState::default();
        let start = Instant::now();
        state.sync_visibility(true, Some("before"));

        assert_eq!(
            state.handle_event(
                &pressed(KeyCode::ControlLeft, Key::Control, None),
                Some("dictated text"),
                start,
            ),
            ClipboardIngressDecision::None
        );
        assert_eq!(
            state.handle_event(
                &pressed(KeyCode::Escape, Key::Escape, Some("\u{1b}")),
                Some("dictated text"),
                start + Duration::from_millis(10),
            ),
            ClipboardIngressDecision::InsertClipboard
        );
        assert_eq!(
            state.handle_event(
                &pressed(KeyCode::Escape, Key::Escape, Some("\u{1b}")),
                Some("dictated text"),
                start + MESSAGE_DIALOG_CLIPBOARD_ESCAPE_GRACE + Duration::from_millis(20),
            ),
            ClipboardIngressDecision::None
        );
    }

    #[test]
    fn ctrl_v_requests_clipboard_paste() {
        assert!(message_dialog_requests_clipboard_paste(
            &pressed_ctrl_v(),
            KeyModifiers {
                ctrl: true,
                alt: false,
                super_key: false,
                shift: false,
            },
        ));
    }

    #[test]
    fn clipboard_text_ingress_updates_message_editor_only_when_editor_focused() {
        let mut app_session = AppSessionState::default();
        app_session.composer.open_message(AgentId(1));

        let outcome = handle_message_dialog_clipboard_text(&mut app_session, "hello\r\nworld");
        assert!(outcome.needs_redraw);
        assert!(outcome.stop);
        assert_eq!(app_session.composer.message_editor.text, "hello\nworld");

        app_session.composer.message_dialog_focus = MessageDialogFocus::AppendButton;
        let ignored = handle_message_dialog_clipboard_text(&mut app_session, "ignored");
        assert!(!ignored.needs_redraw);
        assert!(!ignored.stop);
        assert_eq!(app_session.composer.message_editor.text, "hello\nworld");
    }

    #[test]
    fn message_dialog_arrow_up_moves_within_wrapped_visual_rows() {
        let mut app_session = AppSessionState::default();
        app_session.composer.open_message(AgentId(1));
        app_session.composer.message_editor.load_text("hello world");
        app_session.composer.message_editor.cursor = 8;
        let mut commands = Vec::new();

        let outcome = handle_message_dialog_key(
            &mut app_session,
            &pressed(KeyCode::ArrowUp, Key::ArrowUp, None),
            KeyModifiers {
                ctrl: false,
                alt: false,
                super_key: false,
                shift: false,
            },
            7,
            &mut commands,
        );

        assert!(outcome.needs_redraw);
        assert!(!outcome.stop);
        assert!(commands.is_empty());
        assert_eq!(app_session.composer.message_editor.cursor, 2);
    }

    #[test]
    fn real_escape_still_cancels_message_box() {
        let mut app_session = AppSessionState::default();
        app_session.composer.open_message(AgentId(1));
        let mut commands = Vec::new();

        let outcome = handle_message_dialog_key(
            &mut app_session,
            &KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state: ButtonState::Pressed,
                text: Some("\u{1b}".into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            },
            KeyModifiers {
                ctrl: false,
                alt: false,
                super_key: false,
                shift: false,
            },
            80,
            &mut commands,
        );

        assert!(!outcome.needs_redraw);
        assert!(outcome.stop);
        assert_eq!(
            commands,
            vec![AppCommand::Composer(ComposerCommand::Cancel)]
        );
    }
}
