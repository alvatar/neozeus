mod bootstrap;
mod commands;
mod dispatch;
mod output;
mod schedule;
mod session;
mod use_cases;

pub(crate) use commands::{
    AgentCommand, AppCommand, ComposerCommand, ComposerRequest, TaskCommand, TerminalCommand,
    WidgetCommand,
};
#[cfg(test)]
pub(crate) use dispatch::apply_app_commands;
pub(crate) use session::AppSessionState;
pub(crate) use use_cases::restore_app;

pub(crate) use bootstrap::build_app;
#[cfg(test)]
pub(crate) use bootstrap::resolve_window_scale_factor;
#[cfg(test)]
pub(crate) use output::{AppOutputConfig, OutputMode};

#[cfg(test)]
pub(crate) use {
    bootstrap::{
        format_startup_panic, normalize_output_for_x11_fallback, primary_window_config_for,
        primary_window_config_for_with_config, primary_window_plugin_config_for,
        resolve_disable_pipelined_rendering_for, resolve_force_fallback_adapter,
        resolve_force_fallback_adapter_for, resolve_linux_window_backend, resolve_window_mode,
        should_force_x11_backend, uses_headless_runner, LinuxWindowBackend,
    },
    output::{resolve_output_dimension, resolve_output_mode},
};
