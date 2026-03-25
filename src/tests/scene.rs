use crate::{
    hud::TerminalVisibilityPolicy,
    scene::{
        choose_startup_focus_session_name, format_startup_panic, resolve_force_fallback_adapter,
        resolve_window_mode, resolve_window_scale_factor, should_request_visual_redraw,
        startup_visibility_policy_for_focus,
    },
    terminals::TerminalId,
};
use bevy::window::{MonitorSelection, WindowMode};

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
fn defaults_window_mode_to_borderless_fullscreen() {
    assert_eq!(
        resolve_window_mode(None),
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    );
    assert_eq!(
        resolve_window_mode(Some("fullscreen")),
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    );
}

#[test]
fn allows_explicit_windowed_override() {
    assert_eq!(resolve_window_mode(Some("windowed")), WindowMode::Windowed);
    assert_eq!(
        resolve_window_mode(Some(" WINDOWED ")),
        WindowMode::Windowed
    );
}

#[test]
fn parses_optional_window_scale_factor_override() {
    assert_eq!(resolve_window_scale_factor(None), None);
    assert_eq!(resolve_window_scale_factor(Some("")), None);
    assert_eq!(resolve_window_scale_factor(Some("  ")), None);
    assert_eq!(resolve_window_scale_factor(Some("1.0")), Some(1.0));
    assert_eq!(resolve_window_scale_factor(Some(" 2.5 ")), Some(2.5));
    assert_eq!(resolve_window_scale_factor(Some("0")), None);
    assert_eq!(resolve_window_scale_factor(Some("-1")), None);
    assert_eq!(resolve_window_scale_factor(Some("abc")), None);
}

#[test]
fn parses_force_fallback_adapter_override() {
    assert!(resolve_force_fallback_adapter(None));
    assert!(resolve_force_fallback_adapter(Some("")));
    assert!(resolve_force_fallback_adapter(Some("true")));
    assert!(resolve_force_fallback_adapter(Some("1")));
    assert!(!resolve_force_fallback_adapter(Some("false")));
    assert!(!resolve_force_fallback_adapter(Some("0")));
}

#[test]
fn formats_missing_gpu_startup_panics_as_user_facing_errors() {
    let error = format_startup_panic(&"Unable to find a GPU! renderer init failed")
        .expect("missing gpu panic should be formatted");
    assert!(error.contains("could not find a usable graphics adapter"));
    assert!(format_startup_panic(&"some other panic").is_none());
}
