use super::{
    AppSessionState, CreateAgentDialogField, CreateAgentDialogState, CreateAgentKind,
    VisibilityMode,
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
