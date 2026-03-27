mod composer;
mod conversation;
mod focus_agent;
mod kill_active_agent;
mod restore_app;
mod spawn_agent_terminal;
mod widget;

pub(crate) use composer::{cancel_composer, open_composer, submit_composer};
pub(crate) use conversation::{
    append_task, clear_done_tasks, consume_next_task, prepend_task, reset_active_view,
    send_message, send_terminal_command, set_task_text, toggle_active_display_mode, toggle_widget,
};
pub(crate) use focus_agent::focus_agent;
pub(crate) use kill_active_agent::kill_active_agent;
pub(crate) use restore_app::restore_app;
pub(crate) use spawn_agent_terminal::{attach_restored_terminal, spawn_agent_terminal};
pub(crate) use widget::reset_widget;
