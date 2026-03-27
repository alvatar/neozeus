use crate::{
    agents::AgentId,
    hud::{HudMessageBoxState, HudTaskDialogState},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ComposerMode {
    Message { agent_id: AgentId },
    TaskEdit { agent_id: AgentId },
}

impl ComposerMode {
    pub(crate) fn agent_id(&self) -> AgentId {
        match self {
            Self::Message { agent_id } | Self::TaskEdit { agent_id } => *agent_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerSession {
    pub(crate) mode: ComposerMode,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ComposerState {
    pub(crate) session: Option<ComposerSession>,
    pub(crate) message_editor: HudMessageBoxState,
    pub(crate) task_editor: HudTaskDialogState,
    agent_to_terminal: BTreeMap<AgentId, crate::terminals::TerminalId>,
}

impl ComposerState {
    pub(crate) fn keyboard_capture_active(
        &self,
        input_capture: &crate::hud::HudInputCaptureState,
    ) -> bool {
        self.message_editor.visible
            || self.task_editor.visible
            || input_capture.direct_input_terminal.is_some()
    }

    pub(crate) fn bind_agent_terminal(
        &mut self,
        agent_id: AgentId,
        terminal_id: crate::terminals::TerminalId,
    ) {
        self.agent_to_terminal.insert(agent_id, terminal_id);
    }

    pub(crate) fn unbind_agent(&mut self, agent_id: AgentId) {
        self.agent_to_terminal.remove(&agent_id);
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.mode.agent_id() == agent_id)
        {
            self.cancel_preserving_draft();
        }
    }

    pub(crate) fn open_message(
        &mut self,
        agent_id: AgentId,
        terminal_id: crate::terminals::TerminalId,
    ) {
        self.bind_agent_terminal(agent_id, terminal_id);
        self.task_editor.close();
        self.message_editor.reset_for_target(terminal_id);
        self.session = Some(ComposerSession {
            mode: ComposerMode::Message { agent_id },
        });
    }

    pub(crate) fn open_task_editor(
        &mut self,
        agent_id: AgentId,
        terminal_id: crate::terminals::TerminalId,
        text: &str,
    ) {
        self.bind_agent_terminal(agent_id, terminal_id);
        self.message_editor.close();
        self.task_editor.open_with_text(terminal_id, text);
        self.session = Some(ComposerSession {
            mode: ComposerMode::TaskEdit { agent_id },
        });
    }

    pub(crate) fn cancel_preserving_draft(&mut self) {
        if let Some(session) = self.session.take() {
            match session.mode {
                ComposerMode::Message { .. } => self.message_editor.close(),
                ComposerMode::TaskEdit { .. } => self.task_editor.close(),
            }
        }
    }

    pub(crate) fn discard_current_message(&mut self) {
        self.message_editor.close_and_discard_current();
        if matches!(
            self.session.as_ref().map(|session| &session.mode),
            Some(ComposerMode::Message { .. })
        ) {
            self.session = None;
        }
    }

    pub(crate) fn close_task_editor(&mut self) {
        self.task_editor.close();
        if matches!(
            self.session.as_ref().map(|session| &session.mode),
            Some(ComposerMode::TaskEdit { .. })
        ) {
            self.session = None;
        }
    }
}

#[cfg(test)]
mod tests;
