#[allow(
    unused_imports,
    reason = "scene.rs is a compatibility facade for tests and main"
)]
pub(crate) use crate::app::{
    build_app, format_startup_panic, primary_window_config_for, resolve_force_fallback_adapter,
    resolve_output_dimension, resolve_output_mode, resolve_window_mode,
    resolve_window_scale_factor, AppOutputConfig, OutputMode,
};
#[allow(
    unused_imports,
    reason = "scene.rs is a compatibility facade for tests and main"
)]
pub(crate) use crate::startup::{
    choose_startup_focus_session_name, should_request_visual_redraw,
    startup_visibility_policy_for_focus,
};
