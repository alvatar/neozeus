use crate::{agents::AgentId, hud::HudInputCaptureState, terminals::TerminalId};

use super::{
    commands::{AgentCommand, AppCommand},
    path_completion::{complete_directory_segment, DirectoryCompletionCandidate},
};
use bevy::prelude::Resource;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VisibilityMode {
    #[default]
    ShowAll,
    FocusedOnly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CreateAgentKind {
    #[default]
    Agent,
    Shell,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CreateAgentDialogField {
    #[default]
    Name,
    Kind,
    StartingFolder,
    CreateButton,
}

/// Lightweight single-line text field state for form controls.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextFieldState {
    pub(crate) text: String,
    pub(crate) cursor: usize,
}

impl TextFieldState {
    /// Replaces the entire field contents and moves the cursor to the end.
    pub(crate) fn load_text(&mut self, text: &str) {
        self.text = normalize_single_line_text(text);
        self.cursor = self.text.len();
    }

    /// Clears the field contents and resets the cursor.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Inserts plain single-line text at the cursor.
    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        let text = normalize_single_line_text(text);
        if text.is_empty() {
            return false;
        }
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
        true
    }

    /// Moves the cursor one character left.
    pub(crate) fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        if previous == self.cursor {
            return false;
        }
        self.cursor = previous;
        true
    }

    /// Moves the cursor one character right.
    pub(crate) fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        if next == self.cursor {
            return false;
        }
        self.cursor = next;
        true
    }

    /// Moves the cursor to the field start.
    pub(crate) fn move_start(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor = 0;
        true
    }

    /// Moves the cursor to the field end.
    pub(crate) fn move_end(&mut self) -> bool {
        if self.cursor == self.text.len() {
            return false;
        }
        self.cursor = self.text.len();
        true
    }

    /// Deletes the character before the cursor.
    pub(crate) fn delete_backward_char(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        if previous == self.cursor {
            return false;
        }
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
        true
    }

    /// Deletes the character at the cursor.
    pub(crate) fn delete_forward_char(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        if next == self.cursor {
            return false;
        }
        self.text.drain(self.cursor..next);
        true
    }

    /// Moves the cursor to the start of the previous word.
    pub(crate) fn move_word_backward(&mut self) -> bool {
        let target = word_backward_boundary(&self.text, self.cursor);
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        true
    }

    /// Moves the cursor to the start of the next word boundary.
    pub(crate) fn move_word_forward(&mut self) -> bool {
        let target = word_forward_boundary(&self.text, self.cursor);
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        true
    }

    /// Deletes the word before the cursor.
    pub(crate) fn kill_word_backward(&mut self) -> bool {
        let start = word_backward_boundary(&self.text, self.cursor);
        if start == self.cursor {
            return false;
        }
        self.text.drain(start..self.cursor);
        self.cursor = start;
        true
    }

    /// Deletes the word after the cursor.
    pub(crate) fn kill_word_forward(&mut self) -> bool {
        let end = word_forward_boundary(&self.text, self.cursor);
        if end == self.cursor {
            return false;
        }
        self.text.drain(self.cursor..end);
        true
    }

    /// Deletes from the cursor to the end of the field.
    pub(crate) fn kill_to_end(&mut self) -> bool {
        if self.cursor >= self.text.len() {
            return false;
        }
        self.text.truncate(self.cursor);
        true
    }
}

/// One currently available cwd completion candidate shown in the create-agent dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CwdCompletionItem {
    pub(crate) display: String,
    pub(crate) completion_text: String,
}

/// Active cwd-completion session state for the create-agent dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CwdCompletionState {
    pub(crate) source_text: String,
    pub(crate) items: Vec<CwdCompletionItem>,
    pub(crate) selected: usize,
    pub(crate) preview_active: bool,
}

/// Specialized cwd field state with directory completion support.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CwdFieldState {
    pub(crate) field: TextFieldState,
    pub(crate) completion: Option<CwdCompletionState>,
}

impl CwdFieldState {
    /// Replaces the field contents and clears any stale completion session.
    pub(crate) fn load_text(&mut self, text: &str) {
        self.field.load_text(text);
        self.clear_completion();
    }

    /// Clears both field text and completion state.
    pub(crate) fn clear(&mut self) {
        self.field.clear();
        self.clear_completion();
    }

    /// Removes the current completion session without modifying field text.
    pub(crate) fn clear_completion(&mut self) {
        self.completion = None;
    }

    /// Starts completion or cycles the active candidate preview.
    pub(crate) fn start_or_cycle_completion(&mut self, reverse: bool) -> bool {
        if let Some(session) = self.completion.as_mut() {
            if session.items.is_empty() {
                self.completion = None;
                return false;
            }
            if session.preview_active {
                session.selected = if reverse {
                    (session.selected + session.items.len() - 1) % session.items.len()
                } else {
                    (session.selected + 1) % session.items.len()
                };
            } else {
                session.preview_active = true;
            }
            let preview = session.items[session.selected].completion_text.clone();
            self.field.load_text(&preview);
            return true;
        }

        let Ok(items) = complete_directory_segment(&self.field.text, self.field.cursor) else {
            return false;
        };
        if items.is_empty() {
            return false;
        }

        let session = CwdCompletionState {
            source_text: self.field.text.clone(),
            items: items.into_iter().map(CwdCompletionItem::from).collect(),
            selected: 0,
            preview_active: true,
        };
        let preview = session.items[0].completion_text.clone();
        self.field.load_text(&preview);
        self.completion = Some(session);
        true
    }

    /// Accepts the currently selected completion and prepares completion for the next path level.
    pub(crate) fn accept_completion(&mut self) -> bool {
        if let Some(session) = self.completion.take() {
            let accepted_text = if session.preview_active {
                session.items[session.selected].completion_text.clone()
            } else {
                self.field.text.clone()
            };
            self.field.load_text(&accepted_text);
            if let Ok(items) = complete_directory_segment(&self.field.text, self.field.cursor) {
                if !items.is_empty() {
                    self.completion = Some(CwdCompletionState {
                        source_text: accepted_text,
                        items: items.into_iter().map(CwdCompletionItem::from).collect(),
                        selected: 0,
                        preview_active: false,
                    });
                }
            }
            return true;
        }

        let Ok(items) = complete_directory_segment(&self.field.text, self.field.cursor) else {
            return false;
        };
        if items.is_empty() {
            return false;
        }
        self.completion = Some(CwdCompletionState {
            source_text: self.field.text.clone(),
            items: items.into_iter().map(CwdCompletionItem::from).collect(),
            selected: 0,
            preview_active: false,
        });
        true
    }
}

impl From<DirectoryCompletionCandidate> for CwdCompletionItem {
    fn from(value: DirectoryCompletionCandidate) -> Self {
        Self {
            display: value.display,
            completion_text: value.completion_text,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CreateAgentDialogState {
    pub(crate) visible: bool,
    pub(crate) name_field: TextFieldState,
    pub(crate) cwd_field: CwdFieldState,
    pub(crate) kind: CreateAgentKind,
    pub(crate) focus: CreateAgentDialogField,
    pub(crate) error: Option<String>,
}

impl CreateAgentDialogState {
    /// Opens the create-agent dialog with the provided initial kind and default folder.
    pub(crate) fn open(&mut self, kind: CreateAgentKind) {
        self.visible = true;
        self.kind = kind;
        self.focus = CreateAgentDialogField::Name;
        self.error = None;
        self.name_field.load_text("");
        self.cwd_field.load_text("~/code");
    }

    /// Closes the dialog and discards all current field state.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.focus = CreateAgentDialogField::Name;
        self.error = None;
        self.name_field.clear();
        self.cwd_field.clear();
    }

    /// Returns whether this dialog currently owns keyboard capture.
    pub(crate) fn keyboard_capture_active(&self) -> bool {
        self.visible
    }

    /// Advances focus to the next or previous field in the dialog's fixed tab order.
    pub(crate) fn cycle_focus(&mut self, reverse: bool) {
        self.focus = match (self.focus, reverse) {
            (CreateAgentDialogField::Name, false) => CreateAgentDialogField::Kind,
            (CreateAgentDialogField::Kind, false) => CreateAgentDialogField::StartingFolder,
            (CreateAgentDialogField::StartingFolder, false) => CreateAgentDialogField::Name,
            (CreateAgentDialogField::CreateButton, false) => CreateAgentDialogField::Name,
            (CreateAgentDialogField::Name, true) => CreateAgentDialogField::StartingFolder,
            (CreateAgentDialogField::Kind, true) => CreateAgentDialogField::Name,
            (CreateAgentDialogField::StartingFolder, true) => CreateAgentDialogField::Kind,
            (CreateAgentDialogField::CreateButton, true) => CreateAgentDialogField::StartingFolder,
        };
        if self.focus != CreateAgentDialogField::StartingFolder {
            self.cwd_field.clear_completion();
        }
    }

    /// Sets the selected creation kind and clears any stale dialog error.
    pub(crate) fn set_kind(&mut self, kind: CreateAgentKind) {
        self.kind = kind;
        self.error = None;
    }

    /// Returns the label entered by the user, trimmed and normalized to optional form.
    pub(crate) fn label(&self) -> Option<String> {
        let trimmed = self.name_field.text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    /// Returns the raw cwd field text after outer trimming.
    pub(crate) fn starting_folder(&self) -> String {
        self.cwd_field.field.text.trim().to_owned()
    }

    /// Returns whether the selected creation kind should spawn a raw shell.
    pub(crate) fn spawn_shell_only(&self) -> bool {
        matches!(self.kind, CreateAgentKind::Shell)
    }

    /// Builds the app command that should create the configured agent, validating required fields.
    pub(crate) fn build_create_command(&mut self) -> Option<AppCommand> {
        let working_directory = self.starting_folder();
        if working_directory.is_empty() {
            self.error = Some("cwd is required".to_owned());
            return None;
        }
        self.error = None;
        Some(AppCommand::Agent(AgentCommand::Create {
            label: self.label(),
            spawn_shell_only: self.spawn_shell_only(),
            working_directory,
        }))
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AppSessionState {
    pub(crate) active_agent: Option<AgentId>,
    pub(crate) visibility_mode: VisibilityMode,
    pub(crate) composer: crate::composer::ComposerState,
    pub(crate) create_agent_dialog: CreateAgentDialogState,
    pub(crate) direct_input_terminal: Option<TerminalId>,
}

impl AppSessionState {
    /// Returns whether any modal/editor state currently owns keyboard capture.
    pub(crate) fn keyboard_capture_active(&self, input_capture: &HudInputCaptureState) -> bool {
        self.composer.keyboard_capture_active(input_capture)
            || self.create_agent_dialog.keyboard_capture_active()
    }

    /// Returns whether any HUD modal is visible.
    pub(crate) fn modal_visible(&self) -> bool {
        self.composer.message_editor.visible
            || self.composer.task_editor.visible
            || self.create_agent_dialog.visible
    }
}

fn normalize_single_line_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, '\n' | '\r' | '\t'))
        .collect()
}

fn previous_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return None;
    }
    text[..index]
        .char_indices()
        .last()
        .map(|(offset, _)| offset)
}

fn next_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index >= text.len() {
        return None;
    }
    text[index..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| index + offset)
        .or(Some(text.len()))
}

fn word_backward_boundary(text: &str, mut index: usize) -> usize {
    while let Some(previous) = previous_char_boundary(text, index) {
        let ch = text[previous..index].chars().next().unwrap();
        if !ch.is_whitespace() {
            index = previous;
            break;
        }
        index = previous;
    }
    while let Some(previous) = previous_char_boundary(text, index) {
        let ch = text[previous..index].chars().next().unwrap();
        if ch.is_whitespace() {
            break;
        }
        index = previous;
    }
    index
}

fn word_forward_boundary(text: &str, mut index: usize) -> usize {
    while let Some(next) = next_char_boundary(text, index) {
        let ch = text[index..next].chars().next().unwrap();
        if !ch.is_whitespace() {
            break;
        }
        index = next;
        if index >= text.len() {
            return index;
        }
    }
    while let Some(next) = next_char_boundary(text, index) {
        let ch = text[index..next].chars().next().unwrap();
        if ch.is_whitespace() {
            break;
        }
        index = next;
        if index >= text.len() {
            break;
        }
    }
    index
}

#[cfg(test)]
mod tests;
