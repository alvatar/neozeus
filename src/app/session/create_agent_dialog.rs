use crate::{
    composer::TextEditorState,
    dialogs::{cycle_dialog_focus, DialogTabOrder},
    shared::text_cursor::{
        next_char_boundary, previous_char_boundary, word_backward_boundary, word_forward_boundary,
    },
};

use super::super::{
    commands::{AgentCommand, AppCommand},
    path_completion::{complete_directory_segment, DirectoryCompletionCandidate},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CreateAgentKind {
    #[default]
    Pi,
    Claude,
    Codex,
    Terminal,
}

impl CreateAgentKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Terminal => "terminal",
        }
    }

    pub(crate) const fn agent_kind(self) -> crate::agents::AgentKind {
        match self {
            Self::Pi => crate::agents::AgentKind::Pi,
            Self::Claude => crate::agents::AgentKind::Claude,
            Self::Codex => crate::agents::AgentKind::Codex,
            Self::Terminal => crate::agents::AgentKind::Terminal,
        }
    }

    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Pi => Self::Claude,
            Self::Claude => Self::Codex,
            Self::Codex => Self::Terminal,
            Self::Terminal => Self::Pi,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CreateAgentDialogField {
    #[default]
    Name,
    Kind,
    StartingFolder,
    CreateButton,
}

impl DialogTabOrder for CreateAgentDialogField {
    const TAB_ORDER: &'static [Self] = &[
        Self::Name,
        Self::Kind,
        Self::StartingFolder,
        Self::CreateButton,
    ];
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
        let target = word_backward_boundary(&self.text, self.cursor, |ch| !ch.is_whitespace());
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        true
    }

    /// Moves the cursor to the start of the next word boundary.
    pub(crate) fn move_word_forward(&mut self) -> bool {
        let target = word_forward_boundary(&self.text, self.cursor, |ch| !ch.is_whitespace());
        if target == self.cursor {
            return false;
        }
        self.cursor = target;
        true
    }

    /// Deletes the word before the cursor.
    pub(crate) fn kill_word_backward(&mut self) -> bool {
        let start = word_backward_boundary(&self.text, self.cursor, |ch| !ch.is_whitespace());
        if start == self.cursor {
            return false;
        }
        self.text.drain(start..self.cursor);
        self.cursor = start;
        true
    }

    /// Deletes the word after the cursor.
    pub(crate) fn kill_word_forward(&mut self) -> bool {
        let end = word_forward_boundary(&self.text, self.cursor, |ch| !ch.is_whitespace());
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

    /// Deletes the entire field contents and resets the cursor.
    pub(crate) fn kill_all(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        self.clear();
        true
    }
}

/// Active cwd-completion session state for the create-agent dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CwdCompletionState {
    pub(crate) items: Vec<DirectoryCompletionCandidate>,
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
    /// Runs one field-text mutation and clears any stale completion session afterward.
    pub(crate) fn mutate_text<T>(&mut self, mutate: impl FnOnce(&mut TextFieldState) -> T) -> T {
        let result = mutate(&mut self.field);
        self.clear_completion();
        result
    }

    /// Replaces the field contents and clears any stale completion session.
    pub(crate) fn load_text(&mut self, text: &str) {
        self.mutate_text(|field| field.load_text(text));
    }

    /// Clears both field text and completion state.
    pub(crate) fn clear(&mut self) {
        self.mutate_text(TextFieldState::clear);
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
            items,
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
                        items,
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
            items,
            selected: 0,
            preview_active: false,
        });
        true
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CloneAgentDialogField {
    #[default]
    Name,
    Workdir,
    CloneButton,
}

impl DialogTabOrder for CloneAgentDialogField {
    const TAB_ORDER: &'static [Self] = &[Self::Name, Self::Workdir, Self::CloneButton];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RenameAgentDialogField {
    #[default]
    Name,
    RenameButton,
}

impl DialogTabOrder for RenameAgentDialogField {
    const TAB_ORDER: &'static [Self] = &[Self::Name, Self::RenameButton];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AegisDialogField {
    #[default]
    Prompt,
    EnableButton,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ResetDialogFocus {
    #[default]
    CancelButton,
    ResetButton,
}

impl DialogTabOrder for AegisDialogField {
    const TAB_ORDER: &'static [Self] = &[Self::Prompt, Self::EnableButton];
}

impl DialogTabOrder for ResetDialogFocus {
    const TAB_ORDER: &'static [Self] = &[Self::CancelButton, Self::ResetButton];
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CloneAgentDialogState {
    pub(crate) visible: bool,
    pub(crate) source_agent: Option<crate::agents::AgentId>,
    pub(crate) source_kind: Option<crate::agents::AgentKind>,
    pub(crate) name_field: TextFieldState,
    pub(crate) workdir: bool,
    pub(crate) focus: CloneAgentDialogField,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RenameAgentDialogState {
    pub(crate) visible: bool,
    pub(crate) target_agent: Option<crate::agents::AgentId>,
    pub(crate) name_field: TextFieldState,
    pub(crate) focus: RenameAgentDialogField,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AegisDialogState {
    pub(crate) visible: bool,
    pub(crate) target_agent: Option<crate::agents::AgentId>,
    pub(crate) prompt_editor: TextEditorState,
    pub(crate) focus: AegisDialogField,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResetDialogState {
    pub(crate) visible: bool,
    pub(crate) focus: ResetDialogFocus,
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

    #[allow(
        dead_code,
        reason = "capture ownership is derived centrally through AppSessionState::input_owner"
    )]
    /// Returns whether this dialog currently owns keyboard capture.
    pub(crate) fn keyboard_capture_active(&self) -> bool {
        self.visible
    }

    /// Advances focus to the next or previous field in the dialog's shared tab order.
    pub(crate) fn cycle_focus(&mut self, reverse: bool) {
        cycle_dialog_focus(&mut self.focus, reverse);
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
        (!trimmed.is_empty()).then(|| crate::agents::uppercase_agent_label_text(trimmed))
    }

    /// Returns the raw cwd field text after outer trimming.
    pub(crate) fn starting_folder(&self) -> String {
        self.cwd_field.field.text.trim().to_owned()
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
            kind: self.kind.agent_kind(),
            working_directory,
        }))
    }
}

impl CloneAgentDialogState {
    /// Opens the clone-agent dialog for one source agent and suggested label.
    pub(crate) fn open(
        &mut self,
        agent_id: crate::agents::AgentId,
        source_kind: crate::agents::AgentKind,
        current_label: &str,
    ) {
        self.visible = true;
        self.source_agent = Some(agent_id);
        self.source_kind = Some(source_kind);
        self.focus = CloneAgentDialogField::Name;
        self.error = None;
        self.workdir = false;
        self.name_field.load_text(&format!(
            "{}-CLONE",
            crate::agents::uppercase_agent_label_text(current_label)
        ));
    }

    /// Closes the dialog and discards all current field state.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.source_agent = None;
        self.source_kind = None;
        self.focus = CloneAgentDialogField::Name;
        self.error = None;
        self.workdir = false;
        self.name_field.clear();
    }

    #[allow(
        dead_code,
        reason = "capture ownership is derived centrally through AppSessionState::input_owner"
    )]
    /// Returns whether this dialog currently owns keyboard capture.
    pub(crate) fn keyboard_capture_active(&self) -> bool {
        self.visible
    }

    /// Advances focus to the next or previous field in the dialog's shared tab order.
    pub(crate) fn cycle_focus(&mut self, reverse: bool) {
        if self.supports_workdir() {
            cycle_dialog_focus(&mut self.focus, reverse);
            return;
        }
        self.focus = match (self.focus, reverse) {
            (CloneAgentDialogField::Name, false) => CloneAgentDialogField::CloneButton,
            (CloneAgentDialogField::CloneButton, false) => CloneAgentDialogField::Name,
            (CloneAgentDialogField::Name, true) => CloneAgentDialogField::CloneButton,
            (CloneAgentDialogField::CloneButton, true) => CloneAgentDialogField::Name,
            (CloneAgentDialogField::Workdir, false | true) => CloneAgentDialogField::Name,
        };
    }

    pub(crate) fn supports_workdir(&self) -> bool {
        self.source_kind == Some(crate::agents::AgentKind::Pi)
    }

    /// Toggles the workdir checkbox and clears any stale error.
    pub(crate) fn toggle_workdir(&mut self) {
        if !self.supports_workdir() {
            return;
        }
        self.workdir = !self.workdir;
        self.error = None;
    }

    /// Builds the app command that should clone the configured agent, validating required fields.
    pub(crate) fn build_clone_command(&mut self) -> Option<AppCommand> {
        let Some(source_agent_id) = self.source_agent else {
            self.error = Some("missing clone source".to_owned());
            return None;
        };
        let label = self.name_field.text.trim();
        if label.is_empty() {
            self.error = Some("agent name is required".to_owned());
            return None;
        }
        self.error = None;
        Some(AppCommand::Agent(AgentCommand::Clone {
            source_agent_id,
            label: crate::agents::uppercase_agent_label_text(label),
            workdir: self.supports_workdir() && self.workdir,
        }))
    }
}

impl ResetDialogState {
    pub(crate) fn open(&mut self) {
        self.visible = true;
        self.focus = ResetDialogFocus::CancelButton;
    }

    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.focus = ResetDialogFocus::CancelButton;
    }

    pub(crate) fn cycle_focus(&mut self, reverse: bool) {
        cycle_dialog_focus(&mut self.focus, reverse);
    }
}

impl RenameAgentDialogState {
    /// Opens the rename-agent dialog for the provided target and current label.
    pub(crate) fn open(&mut self, agent_id: crate::agents::AgentId, current_label: &str) {
        self.visible = true;
        self.target_agent = Some(agent_id);
        self.focus = RenameAgentDialogField::Name;
        self.error = None;
        self.name_field
            .load_text(&crate::agents::uppercase_agent_label_text(current_label));
    }

    /// Closes the dialog and discards all current field state.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.target_agent = None;
        self.focus = RenameAgentDialogField::Name;
        self.error = None;
        self.name_field.clear();
    }

    #[allow(
        dead_code,
        reason = "capture ownership is derived centrally through AppSessionState::input_owner"
    )]
    /// Returns whether this dialog currently owns keyboard capture.
    pub(crate) fn keyboard_capture_active(&self) -> bool {
        self.visible
    }

    /// Advances focus to the next or previous field in the dialog's shared tab order.
    pub(crate) fn cycle_focus(&mut self, reverse: bool) {
        cycle_dialog_focus(&mut self.focus, reverse);
    }

    /// Builds the app command that should rename the configured agent, validating required fields.
    pub(crate) fn build_rename_command(&mut self) -> Option<AppCommand> {
        let Some(agent_id) = self.target_agent else {
            self.error = Some("missing rename target".to_owned());
            return None;
        };
        let label = self.name_field.text.trim();
        if label.is_empty() {
            self.error = Some("agent name is required".to_owned());
            return None;
        }
        self.error = None;
        Some(AppCommand::Agent(AgentCommand::Rename {
            agent_id,
            label: crate::agents::uppercase_agent_label_text(label),
        }))
    }
}

impl AegisDialogState {
    /// Opens the Aegis dialog for the provided target and prompt text.
    pub(crate) fn open(&mut self, agent_id: crate::agents::AgentId, prompt_text: &str) {
        self.visible = true;
        self.target_agent = Some(agent_id);
        self.focus = AegisDialogField::Prompt;
        self.error = None;
        self.prompt_editor.load_text(prompt_text);
        self.prompt_editor.visible = true;
    }

    /// Closes the dialog and discards the current field state.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.target_agent = None;
        self.focus = AegisDialogField::Prompt;
        self.error = None;
        self.prompt_editor.close_and_discard();
    }

    /// Advances focus to the next or previous field in the dialog's shared tab order.
    pub(crate) fn cycle_focus(&mut self, reverse: bool) {
        cycle_dialog_focus(&mut self.focus, reverse);
    }

    /// Builds the app command that should enable Aegis for the configured agent.
    pub(crate) fn build_enable_command(&mut self) -> Option<AppCommand> {
        let Some(agent_id) = self.target_agent else {
            self.error = Some("missing Aegis target".to_owned());
            return None;
        };
        let prompt_text = self.prompt_editor.text.trim();
        if prompt_text.is_empty() {
            self.error = Some("Aegis prompt is required".to_owned());
            return None;
        }
        self.error = None;
        Some(AppCommand::Aegis(
            super::super::commands::AegisCommand::Enable {
                agent_id,
                prompt_text: prompt_text.to_owned(),
            },
        ))
    }
}

fn normalize_single_line_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, '\n' | '\r' | '\t'))
        .collect()
}
