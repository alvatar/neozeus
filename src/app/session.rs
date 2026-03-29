use crate::{
    agents::AgentId, composer::TextEditorState, hud::HudInputCaptureState, terminals::TerminalId,
};

use super::commands::{AgentCommand, AppCommand};
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

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CreateAgentDialogState {
    pub(crate) visible: bool,
    pub(crate) name_editor: TextEditorState,
    pub(crate) starting_folder_editor: TextEditorState,
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
        self.name_editor.visible = true;
        self.name_editor.load_text("");
        self.starting_folder_editor.visible = true;
        self.starting_folder_editor.load_text("~/code");
    }

    /// Closes the dialog and discards all current field state.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.focus = CreateAgentDialogField::Name;
        self.error = None;
        self.name_editor.close_and_discard();
        self.starting_folder_editor.close_and_discard();
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
    }

    /// Sets the selected creation kind and clears any stale dialog error.
    pub(crate) fn set_kind(&mut self, kind: CreateAgentKind) {
        self.kind = kind;
        self.error = None;
    }

    /// Returns the label entered by the user, trimmed and normalized to optional form.
    pub(crate) fn label(&self) -> Option<String> {
        let trimmed = self.name_editor.text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    /// Returns the raw starting-folder field text after outer trimming.
    pub(crate) fn starting_folder(&self) -> String {
        self.starting_folder_editor.text.trim().to_owned()
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

#[cfg(test)]
mod tests;
