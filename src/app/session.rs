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
    #[cfg(test)]
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
    #[cfg(test)]
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
mod tests {
    use super::{
        AegisDialogField, AegisDialogState, AppSessionState, CloneAgentDialogField,
        CloneAgentDialogState, CreateAgentDialogField, CreateAgentDialogState, CreateAgentKind,
        DialogInputOwner, FocusIntentState, InputOwner, RecoveryStatusState, RecoveryStatusTone,
        RenameAgentDialogField, RenameAgentDialogState, TextFieldState, VisibilityMode,
    };
    use crate::hud::HudInputCaptureState;

    /// Verifies that session visibility remains independently mutable.
    #[test]
    fn session_visibility_mode_updates() {
        let mut session = AppSessionState {
            focus_intent: FocusIntentState {
                visibility_mode: VisibilityMode::FocusedOnly,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(session.visibility_mode(), VisibilityMode::FocusedOnly);
        session.focus_intent.visibility_mode = VisibilityMode::ShowAll;
        assert_eq!(session.visibility_mode(), VisibilityMode::ShowAll);
    }

    /// Verifies that opening the create-agent dialog resets its fields to the requested defaults.
    #[test]
    fn create_agent_dialog_open_resets_defaults() {
        let mut dialog = CreateAgentDialogState::default();
        dialog.name_field.load_text("stale");
        dialog.cwd_field.load_text("/tmp");
        dialog.error = Some("old error".into());

        dialog.open(CreateAgentKind::Terminal);

        assert!(dialog.visible);
        assert_eq!(dialog.kind, CreateAgentKind::Terminal);
        assert_eq!(dialog.focus, CreateAgentDialogField::Name);
        assert_eq!(dialog.name_field.text, "");
        assert_eq!(dialog.cwd_field.field.text, "~/code");
        assert_eq!(dialog.error, None);
    }

    /// Verifies that tab traversal cycles through the fixed dialog field order in both directions.
    #[test]
    fn create_agent_dialog_cycles_focus_forward_and_backward() {
        let mut dialog = CreateAgentDialogState::default();
        dialog.open(CreateAgentKind::Pi);

        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, CreateAgentDialogField::Kind);
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, CreateAgentDialogField::StartingFolder);
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, CreateAgentDialogField::CreateButton);
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, CreateAgentDialogField::Name);

        dialog.cycle_focus(true);
        assert_eq!(dialog.focus, CreateAgentDialogField::CreateButton);
    }

    /// Verifies that the session-level keyboard-capture predicate includes the create-agent dialog.
    #[test]
    fn create_agent_dialog_counts_as_keyboard_capture() {
        let mut session = AppSessionState::default();
        assert!(!session.keyboard_capture_active(&HudInputCaptureState::default()));
        session.create_agent_dialog.open(CreateAgentKind::Pi);
        assert!(session.keyboard_capture_active(&HudInputCaptureState::default()));
    }

    #[test]
    fn input_owner_prioritizes_visible_modal_over_direct_terminal_capture() {
        let mut session = AppSessionState::default();
        let input_capture = HudInputCaptureState {
            direct_input_terminal: Some(crate::terminals::TerminalId(7)),
        };

        session.create_agent_dialog.open(CreateAgentKind::Pi);

        assert_eq!(
            session.input_owner(&input_capture),
            InputOwner::Dialog(DialogInputOwner::CreateAgent)
        );
    }

    #[test]
    fn input_owner_reports_direct_terminal_when_no_modal_is_visible() {
        let session = AppSessionState::default();
        let input_capture = HudInputCaptureState {
            direct_input_terminal: Some(crate::terminals::TerminalId(7)),
        };

        assert_eq!(
            session.input_owner(&input_capture),
            InputOwner::DirectTerminal(crate::terminals::TerminalId(7))
        );
    }

    #[test]
    fn selected_agent_context_does_not_count_as_modal_visibility() {
        let session = AppSessionState::default();
        let agent_list_state = crate::hud::AgentListUiState {
            show_selected_context: true,
            ..Default::default()
        };

        assert!(agent_list_state.show_selected_context);
        assert!(!session.modal_visible());
    }

    #[test]
    fn clone_agent_dialog_open_prefills_name_and_focus() {
        let mut dialog = CloneAgentDialogState {
            error: Some("stale".into()),
            ..Default::default()
        };

        dialog.open(
            crate::agents::AgentId(9),
            crate::agents::AgentKind::Pi,
            "alpha",
        );

        assert!(dialog.visible);
        assert_eq!(dialog.source_agent, Some(crate::agents::AgentId(9)));
        assert_eq!(dialog.name_field.text, "ALPHA-CLONE");
        assert!(!dialog.workdir);
        assert_eq!(dialog.focus, CloneAgentDialogField::Name);
        assert_eq!(dialog.error, None);
    }

    #[test]
    fn clone_agent_dialog_cycles_focus_and_counts_as_keyboard_capture() {
        let mut dialog = CloneAgentDialogState::default();
        dialog.open(
            crate::agents::AgentId(1),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, CloneAgentDialogField::Workdir);
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, CloneAgentDialogField::CloneButton);
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, CloneAgentDialogField::Name);

        let session = AppSessionState {
            clone_agent_dialog: dialog,
            ..Default::default()
        };
        assert!(session.keyboard_capture_active(&HudInputCaptureState::default()));
    }

    #[test]
    fn clone_agent_dialog_builds_clone_command() {
        let mut dialog = CloneAgentDialogState::default();
        dialog.open(
            crate::agents::AgentId(3),
            crate::agents::AgentKind::Pi,
            "alpha",
        );
        dialog.name_field.load_text("child");
        dialog.toggle_workdir();

        assert_eq!(
            dialog.build_clone_command(),
            Some(crate::app::AppCommand::Agent(
                crate::app::AgentCommand::Clone {
                    source_agent_id: crate::agents::AgentId(3),
                    label: "CHILD".into(),
                    workdir: true,
                }
            ))
        );
    }

    #[test]
    fn clone_agent_dialog_for_non_pi_skips_workdir_focus_and_builds_plain_clone_command() {
        let mut dialog = CloneAgentDialogState::default();
        dialog.open(
            crate::agents::AgentId(5),
            crate::agents::AgentKind::Claude,
            "alpha",
        );
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, CloneAgentDialogField::CloneButton);
        dialog.toggle_workdir();
        assert!(!dialog.workdir);
        dialog.name_field.load_text("child");
        assert_eq!(
            dialog.build_clone_command(),
            Some(crate::app::AppCommand::Agent(
                crate::app::AgentCommand::Clone {
                    source_agent_id: crate::agents::AgentId(5),
                    label: "CHILD".into(),
                    workdir: false,
                }
            ))
        );
    }

    #[test]
    fn rename_agent_dialog_open_prefills_name_and_focus() {
        let mut dialog = RenameAgentDialogState {
            error: Some("stale".into()),
            ..Default::default()
        };

        dialog.open(crate::agents::AgentId(7), "alpha");

        assert!(dialog.visible);
        assert_eq!(dialog.target_agent, Some(crate::agents::AgentId(7)));
        assert_eq!(dialog.name_field.text, "ALPHA");
        assert_eq!(dialog.focus, RenameAgentDialogField::Name);
        assert_eq!(dialog.error, None);
    }

    #[test]
    fn rename_agent_dialog_cycles_focus_and_counts_as_keyboard_capture() {
        let mut dialog = RenameAgentDialogState::default();
        dialog.open(crate::agents::AgentId(1), "alpha");
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, RenameAgentDialogField::RenameButton);
        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, RenameAgentDialogField::Name);

        let session = AppSessionState {
            rename_agent_dialog: dialog,
            ..Default::default()
        };
        assert!(session.keyboard_capture_active(&HudInputCaptureState::default()));
    }

    #[test]
    fn aegis_dialog_open_and_build_command() {
        let mut dialog = AegisDialogState::default();
        dialog.open(crate::agents::AgentId(4), "continue cleanly");
        assert!(dialog.visible);
        assert_eq!(dialog.target_agent, Some(crate::agents::AgentId(4)));
        assert_eq!(dialog.prompt_editor.text, "continue cleanly");
        assert_eq!(dialog.focus, AegisDialogField::Prompt);

        dialog.cycle_focus(false);
        assert_eq!(dialog.focus, AegisDialogField::EnableButton);
        assert_eq!(
            dialog.build_enable_command(),
            Some(crate::app::AppCommand::Aegis(
                crate::app::AegisCommand::Enable {
                    agent_id: crate::agents::AgentId(4),
                    prompt_text: "continue cleanly".into(),
                }
            ))
        );
    }

    #[test]
    fn aegis_dialog_counts_as_keyboard_capture() {
        let session = AppSessionState {
            aegis_dialog: AegisDialogState {
                visible: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            session.input_owner(&HudInputCaptureState::default()),
            InputOwner::Dialog(DialogInputOwner::Aegis)
        );
    }

    #[test]
    fn recovery_status_auto_dismisses_after_timeout() {
        let mut status = RecoveryStatusState::default();

        status.show(RecoveryStatusTone::Success, "done", vec!["detail".into()]);
        assert_eq!(status.title.as_deref(), Some("done"));
        assert_eq!(status.remaining_secs, Some(5.0));

        status.tick(4.0);
        assert_eq!(status.title.as_deref(), Some("done"));
        assert_eq!(status.remaining_secs, Some(1.0));

        status.tick(1.1);
        assert_eq!(status.title, None);
        assert!(status.details.is_empty());
        assert_eq!(status.remaining_secs, None);
    }

    #[test]
    fn recovery_status_show_resets_timeout_when_reused() {
        let mut status = RecoveryStatusState::default();

        status.show(RecoveryStatusTone::Info, "first", Vec::new());
        status.tick(4.5);
        status.show(RecoveryStatusTone::Error, "second", Vec::new());

        assert_eq!(status.title.as_deref(), Some("second"));
        assert_eq!(status.remaining_secs, Some(5.0));
    }

    #[test]
    fn agent_dialog_commands_normalize_labels_to_uppercase() {
        let mut create = CreateAgentDialogState::default();
        create.open(CreateAgentKind::Terminal);
        create.name_field.load_text("oracle");
        create.cwd_field.load_text("~/code");
        assert_eq!(
            create.build_create_command(),
            Some(crate::app::AppCommand::Agent(
                crate::app::AgentCommand::Create {
                    label: Some("ORACLE".into()),
                    kind: crate::agents::AgentKind::Terminal,
                    working_directory: "~/code".into(),
                }
            ))
        );

        let mut rename = RenameAgentDialogState::default();
        rename.open(crate::agents::AgentId(1), "alpha");
        rename.name_field.load_text("beta");
        assert_eq!(
            rename.build_rename_command(),
            Some(crate::app::AppCommand::Agent(
                crate::app::AgentCommand::Rename {
                    agent_id: crate::agents::AgentId(1),
                    label: "BETA".into(),
                }
            ))
        );
    }

    /// Verifies that single-line field character motion and deletion respect UTF-8 codepoint
    /// boundaries.
    #[test]
    fn text_field_char_motion_and_delete_follow_utf8_boundaries() {
        let mut field = TextFieldState::default();
        field.load_text("aéb");

        assert!(field.move_left());
        assert_eq!(field.cursor, 3);
        assert!(field.move_left());
        assert_eq!(field.cursor, 1);
        assert!(field.delete_forward_char());
        assert_eq!(field.text, "ab");
        assert_eq!(field.cursor, 1);
    }

    /// Verifies that single-line field word movement uses whitespace-delimited segments rather than the
    /// composer editor's identifier-style word policy.
    #[test]
    fn text_field_word_motion_uses_whitespace_boundaries() {
        let mut field = TextFieldState::default();
        field.load_text("  foo-bar baz");
        assert!(field.move_word_backward());
        assert_eq!(field.cursor, 10);
        assert!(field.move_word_backward());
        assert_eq!(field.cursor, 2);
        assert!(field.move_word_forward());
        assert_eq!(field.cursor, 9);
        assert!(field.move_word_forward());
        assert_eq!(field.cursor, 13);
    }

    /// Verifies that cwd-field text mutations always invalidate stale completion previews.
    #[test]
    fn cwd_field_mutate_text_clears_completion_state() {
        let mut dialog = CreateAgentDialogState::default();
        dialog.open(CreateAgentKind::Pi);
        dialog.cwd_field.completion = Some(super::create_agent_dialog::CwdCompletionState {
            items: vec![crate::app::path_completion::DirectoryCompletionCandidate {
                display: "~/code/".into(),
                completion_text: "~/code/".into(),
            }],
            selected: 0,
            preview_active: true,
        });
        assert!(dialog.cwd_field.completion.is_some());

        let changed = dialog.cwd_field.mutate_text(|field| field.insert_text("x"));

        assert!(changed);
        assert!(dialog.cwd_field.completion.is_none());
    }
}
