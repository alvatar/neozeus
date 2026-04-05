mod create_agent_dialog;

use crate::{agents::AgentId, hud::HudInputCaptureState, terminals::TerminalId};
use bevy::prelude::Resource;

pub(crate) use create_agent_dialog::{
    CloneAgentDialogField, CloneAgentDialogState, CreateAgentDialogField, CreateAgentDialogState,
    CreateAgentKind, RenameAgentDialogField, RenameAgentDialogState, TextFieldState,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VisibilityMode {
    #[default]
    ShowAll,
    FocusedOnly,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AppSessionState {
    pub(crate) active_agent: Option<AgentId>,
    pub(crate) visibility_mode: VisibilityMode,
    pub(crate) composer: crate::composer::ComposerState,
    pub(crate) create_agent_dialog: CreateAgentDialogState,
    pub(crate) clone_agent_dialog: CloneAgentDialogState,
    pub(crate) rename_agent_dialog: RenameAgentDialogState,
    pub(crate) direct_input_terminal: Option<TerminalId>,
}

impl AppSessionState {
    /// Returns whether any modal/editor state currently owns keyboard capture.
    pub(crate) fn keyboard_capture_active(&self, input_capture: &HudInputCaptureState) -> bool {
        self.composer.keyboard_capture_active(input_capture)
            || self.create_agent_dialog.keyboard_capture_active()
            || self.clone_agent_dialog.keyboard_capture_active()
            || self.rename_agent_dialog.keyboard_capture_active()
    }

    /// Returns whether any HUD modal is visible.
    pub(crate) fn modal_visible(&self) -> bool {
        self.composer.message_editor.visible
            || self.composer.task_editor.visible
            || self.create_agent_dialog.visible
            || self.clone_agent_dialog.visible
            || self.rename_agent_dialog.visible
    }
}

#[cfg(test)]
mod tests;
