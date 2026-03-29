mod composer;
mod conversation;
mod focus_agent;
mod kill_active_agent;
mod restore_app;
mod spawn_agent_terminal;
mod tasks;
mod terminals;
mod widgets;

pub(crate) use composer::{cancel_composer, open_composer, submit_composer};
pub(crate) use conversation::send_message;
pub(crate) use focus_agent::focus_agent;
pub(crate) use kill_active_agent::kill_active_agent;
pub(crate) use restore_app::restore_app;
pub(crate) use spawn_agent_terminal::{attach_restored_terminal, spawn_agent_terminal};
pub(crate) use tasks::{
    append_task, clear_done_tasks, consume_next_task, prepend_task, set_task_text,
};
pub(crate) use terminals::send_terminal_command;
pub(crate) use widgets::{reset_widget, toggle_widget};
