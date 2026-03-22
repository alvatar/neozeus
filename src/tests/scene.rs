use crate::{
    hud::TerminalVisibilityPolicy,
    scene::{
        choose_startup_focus_session_name, format_startup_panic, should_request_visual_redraw,
        startup_visibility_policy_for_focus,
    },
    terminals::TerminalId,
};

#[test]
fn redraw_scheduler_stays_idle_without_visual_work() {
    assert!(!should_request_visual_redraw(false, false, false));
}

#[test]
fn redraw_scheduler_runs_when_visual_work_exists() {
    assert!(should_request_visual_redraw(true, false, false));
    assert!(should_request_visual_redraw(false, true, false));
    assert!(should_request_visual_redraw(false, false, true));
}

#[test]
fn startup_focus_prefers_persisted_focus_then_restored_then_imported() {
    assert_eq!(
        choose_startup_focus_session_name(Some("session-b"), &["session-a", "session-b"], &[]),
        Some("session-b")
    );
    assert_eq!(
        choose_startup_focus_session_name(None, &["session-a", "session-b"], &["session-c"]),
        Some("session-a")
    );
    assert_eq!(
        choose_startup_focus_session_name(None, &[], &["session-c", "session-d"]),
        Some("session-c")
    );
    assert_eq!(choose_startup_focus_session_name(None, &[], &[]), None);
}

#[test]
fn startup_visibility_isolate_focused_terminal() {
    assert_eq!(
        startup_visibility_policy_for_focus(Some(TerminalId(7))),
        TerminalVisibilityPolicy::Isolate(TerminalId(7))
    );
    assert_eq!(
        startup_visibility_policy_for_focus(None),
        TerminalVisibilityPolicy::ShowAll
    );
}

#[test]
fn formats_missing_gpu_startup_panics_as_user_facing_errors() {
    let error = format_startup_panic(&"Unable to find a GPU! renderer init failed")
        .expect("missing gpu panic should be formatted");
    assert!(error.contains("could not find a usable graphics adapter"));
    assert!(format_startup_panic(&"some other panic").is_none());
}
