use super::{
    AppSessionState, CloneAgentDialogField, CloneAgentDialogState, CreateAgentDialogField,
    CreateAgentDialogState, CreateAgentKind, RenameAgentDialogField, RenameAgentDialogState,
    TextFieldState, VisibilityMode,
};
use crate::{agents::AgentId, hud::HudInputCaptureState};

/// Verifies that session focus and visibility update independently.
#[test]
fn session_focus_and_visibility_update_independently() {
    let mut session = AppSessionState {
        active_agent: Some(AgentId(4)),
        visibility_mode: VisibilityMode::FocusedOnly,
        ..Default::default()
    };
    assert_eq!(session.active_agent, Some(AgentId(4)));
    assert_eq!(session.visibility_mode, VisibilityMode::FocusedOnly);
    session.visibility_mode = VisibilityMode::ShowAll;
    assert_eq!(session.visibility_mode, VisibilityMode::ShowAll);
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
fn clone_agent_dialog_open_prefills_name_and_focus() {
    let mut dialog = CloneAgentDialogState {
        error: Some("stale".into()),
        ..Default::default()
    };

    dialog.open(crate::agents::AgentId(9), "alpha");

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
    dialog.open(crate::agents::AgentId(1), "alpha");
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
    dialog.open(crate::agents::AgentId(3), "alpha");
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
