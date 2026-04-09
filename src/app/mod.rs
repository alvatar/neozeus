mod bootstrap;
mod commands;
mod dispatch;
mod output;
mod path_completion;
mod persistence;
mod schedule;
mod session;
mod use_cases;

pub(crate) use commands::{
    AegisCommand, AgentCommand, AppCommand, ComposerCommand, ComposerRequest, OwnedTmuxCommand,
    RecoveryCommand, TaskCommand, WidgetCommand,
};
pub(crate) use persistence::{
    load_persisted_app_state_from, mark_app_state_dirty, ordered_reconciled_persisted_agents,
    reconcile_persisted_agents, resolve_app_state_path, save_app_state_if_dirty,
    AppStatePersistenceState,
};
pub(crate) use session::{
    AegisDialogField, AppSessionState, CloneAgentDialogField, CreateAgentDialogField,
    CreateAgentKind, RenameAgentDialogField, ResetDialogFocus, TextFieldState, VisibilityMode,
};
pub(crate) use use_cases::{
    clear_composer_and_direct_input, focus_agent_without_persist, open_composer, restore_app,
    send_outbound_message, spawn_runtime_terminal_session, OutboundMessageSource,
};

pub(crate) use bootstrap::build_app;
#[cfg(test)]
pub(crate) use bootstrap::resolve_window_scale_factor;
#[cfg(test)]
pub(crate) use output::{AppOutputConfig, OutputMode};

#[cfg(test)]
pub(crate) use dispatch::sync_agents_from_terminals;

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

/// Test harness that runs the `apply_app_commands` system exactly once against the given world.
#[cfg(test)]
pub(crate) fn run_apply_app_commands(world: &mut bevy::prelude::World) {
    use bevy::ecs::system::RunSystemOnce;

    if !world.contains_resource::<crate::hud::AgentListSelection>() {
        world.insert_resource(crate::hud::AgentListSelection::default());
    }
    if !world.contains_resource::<crate::terminals::OwnedTmuxSessionStore>() {
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    }
    if !world.contains_resource::<crate::terminals::ActiveTerminalContentState>() {
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    }
    if !world.contains_resource::<crate::terminals::ActiveTerminalContentSyncState>() {
        world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    }
    if !world.contains_resource::<crate::aegis::AegisPolicyStore>() {
        world.insert_resource(crate::aegis::AegisPolicyStore::default());
    }
    if !world.contains_resource::<crate::aegis::AegisRuntimeStore>() {
        world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    }

    world.run_system_once(dispatch::apply_app_commands).unwrap();
}
