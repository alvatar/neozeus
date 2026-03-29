use crate::{
    app::{AppCommand, AppSessionState, ComposerCommand, CreateAgentDialogField, CreateAgentKind},
    composer::{MessageDialogFocus, TaskDialogFocus},
};
use bevy::{input::keyboard::KeyboardInput, prelude::KeyCode};

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

/// Handles one key press while the create-agent dialog owns keyboard capture.
pub(super) fn handle_create_agent_dialog_key(
    app_session: &mut AppSessionState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    emitted_commands: &mut Vec<AppCommand>,
) -> ModalKeyResult {
    if event.key_code == KeyCode::Escape {
        app_session.create_agent_dialog.close();
        return ModalKeyResult::redraw_and_stop();
    }

    if modifiers.plain() && event.key_code == KeyCode::Tab {
        app_session.create_agent_dialog.cycle_focus(modifiers.shift);
        return ModalKeyResult::redraw();
    }

    let (changed, clear_error) = match app_session.create_agent_dialog.focus {
        CreateAgentDialogField::Name => (
            handle_text_field_event(
                &mut app_session.create_agent_dialog.name_field,
                event,
                modifiers,
            ),
            true,
        ),
        CreateAgentDialogField::Kind => {
            if modifiers.plain() && event.key_code == KeyCode::Space {
                let next_kind = match app_session.create_agent_dialog.kind {
                    CreateAgentKind::Agent => CreateAgentKind::Shell,
                    CreateAgentKind::Shell => CreateAgentKind::Agent,
                };
                app_session.create_agent_dialog.set_kind(next_kind);
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

    if changed {
        if clear_error {
            app_session.create_agent_dialog.error = None;
        }
        ModalKeyResult::redraw()
    } else {
        ModalKeyResult::default()
    }
}

/// Handles one key press while the message dialog owns keyboard capture.
pub(super) fn handle_message_dialog_key(
    app_session: &mut AppSessionState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
    emitted_commands: &mut Vec<AppCommand>,
) -> ModalKeyResult {
    if modifiers.ctrl_only() && event.key_code == KeyCode::KeyS {
        if !app_session.composer.message_editor.text.trim().is_empty() {
            emitted_commands.push(AppCommand::Composer(ComposerCommand::Submit));
        }
        return ModalKeyResult::stop();
    }

    if event.key_code == KeyCode::Escape {
        emitted_commands.push(AppCommand::Composer(ComposerCommand::Cancel));
        return ModalKeyResult::stop();
    }

    if modifiers.plain() && event.key_code == KeyCode::Tab {
        app_session
            .composer
            .cycle_message_dialog_focus(modifiers.shift);
        return ModalKeyResult::redraw();
    }

    match app_session.composer.message_dialog_focus {
        MessageDialogFocus::Editor => handle_text_editor_result(super::handle_text_editor_event(
            &mut app_session.composer.message_editor,
            event,
            modifiers.ctrl,
            modifiers.alt,
            modifiers.super_key,
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

    if event.key_code == KeyCode::Escape {
        emitted_commands.push(AppCommand::Composer(ComposerCommand::Submit));
        return ModalKeyResult::stop();
    }

    if modifiers.plain() && event.key_code == KeyCode::Tab {
        app_session
            .composer
            .cycle_task_dialog_focus(modifiers.shift);
        return ModalKeyResult::redraw();
    }

    match app_session.composer.task_dialog_focus {
        TaskDialogFocus::Editor => handle_text_editor_result(super::handle_text_editor_event(
            &mut app_session.composer.task_editor,
            event,
            modifiers.ctrl,
            modifiers.alt,
            modifiers.super_key,
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
fn handle_text_field_event(
    field: &mut crate::app::TextFieldState,
    event: &KeyboardInput,
    modifiers: KeyModifiers,
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
                .is_some_and(|text| field.insert_text(&text)),
        }
    } else {
        false
    }
}
