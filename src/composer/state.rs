use super::layout::{MessageBoxAction, TaskDialogAction};
use crate::{
    agents::AgentId,
    app::{AppCommand, TaskCommand},
    dialogs::{cycle_dialog_focus, DialogTabOrder},
};
use std::collections::BTreeMap;

pub(super) const TEXT_EDITOR_KILL_RING_LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextEditorYankState {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) ring_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct TextEditorDraft {
    pub(super) text: String,
    pub(super) cursor: usize,
    pub(super) mark: Option<usize>,
    pub(super) preferred_column: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TextEditorState {
    pub(crate) visible: bool,
    #[cfg(test)]
    pub(crate) target_terminal: Option<crate::terminals::TerminalId>,
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) mark: Option<usize>,
    pub(crate) preferred_column: Option<usize>,
    pub(crate) kill_ring: Vec<String>,
    pub(crate) yank_state: Option<TextEditorYankState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ComposerMode {
    Message { agent_id: AgentId },
    TaskEdit { agent_id: AgentId },
}

impl ComposerMode {
    /// Returns the agent id associated with this composer mode.
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MessageDialogFocus {
    #[default]
    Editor,
    AppendButton,
    PrependButton,
}

impl DialogTabOrder for MessageDialogFocus {
    const TAB_ORDER: &'static [Self] = &[Self::Editor, Self::AppendButton, Self::PrependButton];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TaskDialogFocus {
    #[default]
    Editor,
    ClearDoneButton,
}

impl DialogTabOrder for TaskDialogFocus {
    const TAB_ORDER: &'static [Self] = &[Self::Editor, Self::ClearDoneButton];
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ComposerState {
    pub(crate) session: Option<ComposerSession>,
    pub(crate) message_editor: TextEditorState,
    pub(crate) task_editor: TextEditorState,
    pub(crate) message_dialog_focus: MessageDialogFocus,
    pub(crate) task_dialog_focus: TaskDialogFocus,
    message_drafts: BTreeMap<AgentId, TextEditorDraft>,
}

impl ComposerState {
    /// Handles unbind agent.
    pub(crate) fn unbind_agent(&mut self, agent_id: AgentId) {
        self.message_drafts.remove(&agent_id);
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.mode.agent_id() == agent_id)
        {
            self.cancel_preserving_draft();
        }
    }

    /// Opens message.
    pub(crate) fn open_message(&mut self, agent_id: AgentId) {
        self.save_open_message_draft();
        self.task_editor.close();
        self.task_dialog_focus = TaskDialogFocus::Editor;
        self.message_editor.visible = true;
        self.message_dialog_focus = MessageDialogFocus::Editor;
        if let Some(draft) = self.message_drafts.get(&agent_id).cloned() {
            self.message_editor.restore_draft(draft);
        } else {
            self.message_editor.load_text("");
        }
        self.session = Some(ComposerSession {
            mode: ComposerMode::Message { agent_id },
        });
    }

    /// Opens task editor.
    pub(crate) fn open_task_editor(&mut self, agent_id: AgentId, text: &str) {
        self.save_open_message_draft();
        self.message_editor.close();
        self.message_dialog_focus = MessageDialogFocus::Editor;
        self.task_editor.visible = true;
        self.task_dialog_focus = TaskDialogFocus::Editor;
        self.task_editor.load_text(text);
        self.session = Some(ComposerSession {
            mode: ComposerMode::TaskEdit { agent_id },
        });
    }

    /// Handles cancel preserving draft.
    pub(crate) fn cancel_preserving_draft(&mut self) {
        if let Some(session) = self.session.take() {
            match session.mode {
                ComposerMode::Message { agent_id } => {
                    self.save_message_draft(agent_id);
                    self.message_editor.close();
                    self.message_dialog_focus = MessageDialogFocus::Editor;
                }
                ComposerMode::TaskEdit { .. } => self.task_editor.close(),
            }
        }
    }

    /// Handles discard current message.
    pub(crate) fn discard_current_message(&mut self) {
        if let Some(agent_id) = self.current_message_agent() {
            self.message_drafts.remove(&agent_id);
        }
        self.message_editor.close_and_discard();
        self.message_dialog_focus = MessageDialogFocus::Editor;
        if matches!(
            self.session.as_ref().map(|session| &session.mode),
            Some(ComposerMode::Message { .. })
        ) {
            self.session = None;
        }
    }

    /// Closes task editor.
    pub(crate) fn close_task_editor(&mut self) {
        self.task_editor.close();
        self.task_dialog_focus = TaskDialogFocus::Editor;
        if matches!(
            self.session.as_ref().map(|session| &session.mode),
            Some(ComposerMode::TaskEdit { .. })
        ) {
            self.session = None;
        }
    }

    /// Returns the agent currently bound to the active composer session.
    pub(crate) fn current_agent(&self) -> Option<AgentId> {
        self.session.as_ref().map(|session| session.mode.agent_id())
    }

    /// Returns the agent currently bound to the active message composer session.
    fn current_message_agent(&self) -> Option<AgentId> {
        match self.session.as_ref().map(|session| &session.mode) {
            Some(ComposerMode::Message { agent_id }) => Some(*agent_id),
            _ => None,
        }
    }

    /// Saves open message draft.
    fn save_open_message_draft(&mut self) {
        if let Some(agent_id) = self.current_message_agent() {
            self.save_message_draft(agent_id);
        }
    }

    /// Saves message draft.
    fn save_message_draft(&mut self, agent_id: AgentId) {
        self.message_drafts
            .insert(agent_id, self.message_editor.snapshot_draft());
    }

    /// Advances focus through the message dialog's shared tab order.
    pub(crate) fn cycle_message_dialog_focus(&mut self, reverse: bool) {
        cycle_dialog_focus(&mut self.message_dialog_focus, reverse);
    }

    /// Advances focus through the task dialog's shared tab order.
    pub(crate) fn cycle_task_dialog_focus(&mut self, reverse: bool) {
        cycle_dialog_focus(&mut self.task_dialog_focus, reverse);
    }

    /// Converts a message-box action button selection into the corresponding task command.
    pub(crate) fn message_box_action_command(
        &mut self,
        action: MessageBoxAction,
    ) -> Option<AppCommand> {
        let agent_id = match self.session.as_ref().map(|session| &session.mode) {
            Some(ComposerMode::Message { agent_id }) => *agent_id,
            _ => return None,
        };
        let payload = self.message_editor.text.trim().to_owned();
        if payload.is_empty() {
            return None;
        }
        self.discard_current_message();
        Some(AppCommand::Task(match action {
            MessageBoxAction::AppendTask => TaskCommand::Append {
                agent_id,
                text: payload,
            },
            MessageBoxAction::PrependTask => TaskCommand::Prepend {
                agent_id,
                text: payload,
            },
        }))
    }

    /// Converts a task-dialog action button selection into the corresponding task command.
    pub(crate) fn task_dialog_action_command(
        &self,
        action: TaskDialogAction,
    ) -> Option<AppCommand> {
        let agent_id = match self.session.as_ref().map(|session| &session.mode) {
            Some(ComposerMode::TaskEdit { agent_id }) => *agent_id,
            _ => return None,
        };
        match action {
            TaskDialogAction::ClearDone => {
                Some(AppCommand::Task(TaskCommand::ClearDone { agent_id }))
            }
        }
    }
}
