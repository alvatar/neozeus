mod create_agent_dialog;

use crate::{agents::AgentId, hud::HudInputCaptureState, terminals::TerminalId};
use bevy::prelude::Resource;

pub(crate) use create_agent_dialog::{
    AegisDialogField, AegisDialogState, CloneAgentDialogField, CloneAgentDialogState,
    CreateAgentDialogField, CreateAgentDialogState, CreateAgentKind, RenameAgentDialogField,
    RenameAgentDialogState, ResetDialogFocus, ResetDialogState, TextFieldState,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VisibilityMode {
    #[default]
    ShowAll,
    FocusedOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DialogInputOwner {
    ComposerMessage,
    ComposerTask,
    CreateAgent,
    CloneAgent,
    RenameAgent,
    Aegis,
    Reset,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum InputOwner {
    #[default]
    None,
    Dialog(DialogInputOwner),
    DirectTerminal(TerminalId),
}

impl InputOwner {
    pub(crate) fn keyboard_capture_active(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub(crate) fn dialog_visible(&self) -> bool {
        matches!(self, Self::Dialog(_))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum FocusIntentTarget {
    #[default]
    None,
    Agent(AgentId),
    OwnedTmux(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FocusIntentState {
    pub(crate) target: FocusIntentTarget,
    pub(crate) visibility_mode: VisibilityMode,
}

impl FocusIntentState {
    pub(crate) fn focus_agent(&mut self, agent_id: AgentId, visibility_mode: VisibilityMode) {
        self.target = FocusIntentTarget::Agent(agent_id);
        self.visibility_mode = visibility_mode;
    }

    pub(crate) fn focus_owned_tmux(&mut self, session_uid: String) {
        self.target = FocusIntentTarget::OwnedTmux(session_uid);
    }

    pub(crate) fn clear(&mut self, visibility_mode: VisibilityMode) {
        self.target = FocusIntentTarget::None;
        self.visibility_mode = visibility_mode;
    }

    pub(crate) fn selected_agent(&self) -> Option<AgentId> {
        match self.target {
            FocusIntentTarget::Agent(agent_id) => Some(agent_id),
            FocusIntentTarget::None | FocusIntentTarget::OwnedTmux(_) => None,
        }
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AppSessionState {
    pub(crate) focus_intent: FocusIntentState,
    pub(crate) composer: crate::composer::ComposerState,
    pub(crate) create_agent_dialog: CreateAgentDialogState,
    pub(crate) clone_agent_dialog: CloneAgentDialogState,
    pub(crate) rename_agent_dialog: RenameAgentDialogState,
    pub(crate) aegis_dialog: AegisDialogState,
    pub(crate) reset_dialog: ResetDialogState,
}

impl AppSessionState {
    pub(crate) fn visibility_mode(&self) -> VisibilityMode {
        self.focus_intent.visibility_mode
    }

    pub(crate) fn input_owner(&self, input_capture: &HudInputCaptureState) -> InputOwner {
        if self.composer.message_editor.visible {
            InputOwner::Dialog(DialogInputOwner::ComposerMessage)
        } else if self.composer.task_editor.visible {
            InputOwner::Dialog(DialogInputOwner::ComposerTask)
        } else if self.create_agent_dialog.visible {
            InputOwner::Dialog(DialogInputOwner::CreateAgent)
        } else if self.clone_agent_dialog.visible {
            InputOwner::Dialog(DialogInputOwner::CloneAgent)
        } else if self.rename_agent_dialog.visible {
            InputOwner::Dialog(DialogInputOwner::RenameAgent)
        } else if self.aegis_dialog.visible {
            InputOwner::Dialog(DialogInputOwner::Aegis)
        } else if self.reset_dialog.visible {
            InputOwner::Dialog(DialogInputOwner::Reset)
        } else if let Some(terminal_id) = input_capture.direct_input_terminal {
            InputOwner::DirectTerminal(terminal_id)
        } else {
            InputOwner::None
        }
    }

    /// Returns whether any modal/editor state currently owns keyboard capture.
    pub(crate) fn keyboard_capture_active(&self, input_capture: &HudInputCaptureState) -> bool {
        self.input_owner(input_capture).keyboard_capture_active()
    }

    /// Returns whether any HUD modal is visible.
    pub(crate) fn modal_visible(&self) -> bool {
        self.composer.message_editor.visible
            || self.composer.task_editor.visible
            || self.create_agent_dialog.visible
            || self.clone_agent_dialog.visible
            || self.rename_agent_dialog.visible
            || self.aegis_dialog.visible
            || self.reset_dialog.visible
    }

    pub(crate) fn modal_input_owner(&self, input_capture: &HudInputCaptureState) -> bool {
        self.input_owner(input_capture).dialog_visible()
    }
}

#[cfg(test)]
mod tests;
