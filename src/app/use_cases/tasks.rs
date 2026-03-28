use crate::{agents::AgentId, conversations::AgentTaskStore, terminals::TerminalManager};

use super::send_terminal_command;

/// Sets task text.
pub(crate) fn set_task_text(agent_id: AgentId, text: &str, tasks: &mut AgentTaskStore) -> bool {
    tasks.set_text(agent_id, text)
}

/// Appends task.
pub(crate) fn append_task(agent_id: AgentId, text: &str, tasks: &mut AgentTaskStore) -> bool {
    tasks.append_task(agent_id, text)
}

/// Prepends task.
pub(crate) fn prepend_task(agent_id: AgentId, text: &str, tasks: &mut AgentTaskStore) -> bool {
    tasks.prepend_task(agent_id, text)
}

/// Clears done tasks.
pub(crate) fn clear_done_tasks(agent_id: AgentId, tasks: &mut AgentTaskStore) -> bool {
    tasks.clear_done(agent_id)
}

/// Consumes next task.
pub(crate) fn consume_next_task(
    agent_id: AgentId,
    tasks: &mut AgentTaskStore,
    terminal_id: Option<crate::terminals::TerminalId>,
    terminal_manager: &TerminalManager,
) -> bool {
    let Some(message) = tasks.consume_next(agent_id) else {
        return false;
    };
    if let Some(terminal_id) = terminal_id {
        send_terminal_command(terminal_id, &message, terminal_manager);
    }
    true
}
