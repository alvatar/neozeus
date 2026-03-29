use super::{
    AppSessionState, CreateAgentDialogField, CreateAgentDialogState, CreateAgentKind,
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

    dialog.open(CreateAgentKind::Shell);

    assert!(dialog.visible);
    assert_eq!(dialog.kind, CreateAgentKind::Shell);
    assert_eq!(dialog.focus, CreateAgentDialogField::Name);
    assert_eq!(dialog.name_field.text, "");
    assert_eq!(dialog.cwd_field.field.text, "~/code");
    assert_eq!(dialog.error, None);
}

/// Verifies that tab traversal cycles through the fixed dialog field order in both directions.
#[test]
fn create_agent_dialog_cycles_focus_forward_and_backward() {
    let mut dialog = CreateAgentDialogState::default();
    dialog.open(CreateAgentKind::Agent);

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
    session.create_agent_dialog.open(CreateAgentKind::Agent);
    assert!(session.keyboard_capture_active(&HudInputCaptureState::default()));
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
    dialog.open(CreateAgentKind::Agent);
    assert!(dialog.cwd_field.start_or_cycle_completion(false));
    assert!(dialog.cwd_field.completion.is_some());

    let changed = dialog.cwd_field.mutate_text(|field| field.insert_text("x"));

    assert!(changed);
    assert!(dialog.cwd_field.completion.is_none());
}
