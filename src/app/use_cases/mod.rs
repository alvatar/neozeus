mod clone_pi_agent;
mod composer;
mod conversation;
mod focus_agent;
mod kill_selected_agent;
mod owned_tmux;
mod restore_app;
mod spawn_agent_terminal;
mod tasks;
mod terminals;
mod widgets;

pub(crate) use clone_pi_agent::clone_pi_agent;
pub(crate) use composer::{
    cancel_composer, clear_composer_and_direct_input, open_composer, submit_composer,
};
pub(crate) use conversation::send_message;
pub(crate) use focus_agent::{apply_focus_intent, focus_agent, focus_agent_without_persist};
pub(crate) use kill_selected_agent::kill_selected_agent;
pub(crate) use owned_tmux::{kill_selected_owned_tmux, select_owned_tmux};
pub(crate) use restore_app::restore_app;
pub(crate) use spawn_agent_terminal::{
    attach_restored_terminal, pi_launch_spec_for_session_path, spawn_agent_terminal,
    spawn_agent_terminal_with_launch_spec,
};
pub(crate) use tasks::{
    append_task, clear_done_tasks, consume_next_task, prepend_task, set_task_text,
};
pub(crate) use terminals::send_terminal_command;
pub(crate) use widgets::{reset_widget, toggle_widget};
