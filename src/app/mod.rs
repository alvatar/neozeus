pub(crate) mod bootstrap;
pub(crate) mod output;
mod schedule;

pub(crate) use schedule::NeoZeusSet;

pub(crate) use bootstrap::{build_app, resolve_window_scale_factor};
pub(crate) use output::{
    request_final_frame_capture, AppOutputConfig, FinalFrameCaptureConfig, FinalFrameOutputState,
    OutputMode,
};

#[cfg(test)]
pub(crate) use bootstrap::{
    format_startup_panic, primary_window_config_for, primary_window_config_for_with_config,
    primary_window_plugin_config_for, resolve_force_fallback_adapter,
    resolve_force_fallback_adapter_for, resolve_window_mode, uses_headless_runner,
};
#[cfg(test)]
pub(crate) use output::{resolve_output_dimension, resolve_output_mode};
