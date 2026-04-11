mod aegis;
mod clone_agent;
mod composer;
mod conversation;
mod daemon_metadata;
mod focus_agent;
mod kill_selected_agent;
mod owned_tmux;
mod recovery;
mod restore_app;
mod spawn_agent_terminal;
mod tasks;
mod terminals;
mod widgets;

pub(crate) use aegis::{disable_aegis, enable_aegis};
pub(crate) use clone_agent::clone_agent;
pub(crate) use composer::{
    cancel_composer, clear_composer_and_direct_input, open_composer, submit_composer,
};
pub(crate) use conversation::{send_message, send_outbound_message, OutboundMessageSource};
pub(crate) use daemon_metadata::{sync_agent_metadata_to_daemon, sync_session_agent_metadata};
pub(crate) use focus_agent::{
    clear_focus_without_persist, focus_agent, focus_agent_without_persist,
    focus_owned_tmux_without_persist, focus_terminal_without_persist, project_focus_intent,
    FocusMutationContext, FocusProjectionContext,
};
pub(crate) use kill_selected_agent::{kill_selected_agent, KillSelectedAgentContext};
pub(crate) use owned_tmux::{kill_selected_owned_tmux, select_owned_tmux};
pub(crate) use recovery::{reset_runtime_from_snapshot, ResetRuntimeContext};
pub(crate) use restore_app::{render_recovery_status_summary, restore_app, RestoreAppContext};
pub(crate) use spawn_agent_terminal::{
    attach_restored_terminal, claude_fork_launch_spec, codex_fork_launch_spec,
    generate_provider_session_id, launch_spec_for_recovery_spec, pi_launch_spec_for_session_path,
    respawn_recovered_agent_with_launch_spec, spawn_agent_terminal,
    spawn_agent_terminal_with_launch_spec, spawn_runtime_terminal_session, SpawnAgentContext,
};
pub(crate) use tasks::{
    append_task, clear_done_tasks, consume_next_task, prepend_task, set_task_text,
};
pub(crate) use terminals::send_terminal_command;
pub(crate) use widgets::{reset_widget, toggle_widget};
