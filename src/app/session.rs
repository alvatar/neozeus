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
    Terminal(TerminalId),
    OwnedTmux(String),
}

/// Authoritative user-level focus selection.
///
/// Other resources such as agent-list selection, terminal focus, visibility, active terminal
/// content, and direct-input capture are projections derived from this intent, not independent
/// sources of truth.
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

    pub(crate) fn focus_terminal(
        &mut self,
        terminal_id: TerminalId,
        visibility_mode: VisibilityMode,
    ) {
        self.target = FocusIntentTarget::Terminal(terminal_id);
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
            FocusIntentTarget::None
            | FocusIntentTarget::Terminal(_)
            | FocusIntentTarget::OwnedTmux(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RecoveryStatusTone {
    #[default]
    Info,
    Success,
    Error,
}

pub(crate) const RECOVERY_STATUS_AUTO_DISMISS_SECS: f32 = 5.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RecoveryStatusState {
    pub(crate) title: Option<String>,
    pub(crate) details: Vec<String>,
    pub(crate) tone: RecoveryStatusTone,
    pub(crate) remaining_secs: Option<f32>,
}

impl RecoveryStatusState {
    pub(crate) fn clear(&mut self) {
        self.title = None;
        self.details.clear();
        self.tone = RecoveryStatusTone::Info;
        self.remaining_secs = None;
    }

    pub(crate) fn show(
        &mut self,
        tone: RecoveryStatusTone,
        title: impl Into<String>,
        details: Vec<String>,
    ) {
        self.title = Some(title.into());
        self.details = details;
        self.tone = tone;
        self.remaining_secs = Some(RECOVERY_STATUS_AUTO_DISMISS_SECS);
    }

    pub(crate) fn tick(&mut self, delta_secs: f32) {
        let Some(remaining_secs) = self.remaining_secs.as_mut() else {
            return;
        };
        *remaining_secs -= delta_secs.max(0.0);
        if *remaining_secs <= 0.0 {
            self.clear();
        }
    }

    pub(crate) fn show_reset_requested(&mut self) {
        self.show(
            RecoveryStatusTone::Info,
            "Reset requested: confirmation required",
            vec![
                "Kill all live agents, terminals, and owned tmux sessions, then rebuild from the saved snapshot.".into(),
            ],
        );
    }

    pub(crate) fn show_reset_canceled(&mut self) {
        self.show(RecoveryStatusTone::Info, "Reset canceled", Vec::new());
    }

    pub(crate) fn show_reset_confirmed(&mut self) {
        self.show(
            RecoveryStatusTone::Info,
            "Reset confirmed: clearing runtime",
            vec!["Runtime clear started".into()],
        );
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
    pub(crate) recovery_status: RecoveryStatusState,
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
