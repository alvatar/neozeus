use super::{AppSessionState, VisibilityMode};
use crate::agents::AgentId;

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
