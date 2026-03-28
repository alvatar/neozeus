use crate::{
    agents::{AgentId, AgentRuntimeIndex},
    terminals::{mark_terminal_notes_dirty, TerminalNotesState},
};
use bevy::prelude::{Res, ResMut, Resource, Time};
use std::collections::BTreeMap;

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentTaskStore {
    tasks_by_agent: BTreeMap<AgentId, String>,
}

impl AgentTaskStore {
    /// Handles text.
    pub(crate) fn text(&self, agent_id: AgentId) -> Option<&str> {
        self.tasks_by_agent.get(&agent_id).map(String::as_str)
    }

    /// Removes agent.
    pub(crate) fn remove_agent(&mut self, agent_id: AgentId) -> bool {
        self.tasks_by_agent.remove(&agent_id).is_some()
    }

    /// Sets text.
    pub(crate) fn set_text(&mut self, agent_id: AgentId, text: &str) -> bool {
        let trimmed = text.trim_end();
        if trimmed.is_empty() {
            return self.tasks_by_agent.remove(&agent_id).is_some();
        }
        match self.tasks_by_agent.get_mut(&agent_id) {
            Some(existing) if existing == trimmed => false,
            Some(existing) => {
                existing.clear();
                existing.push_str(trimmed);
                true
            }
            None => {
                self.tasks_by_agent.insert(agent_id, trimmed.to_owned());
                true
            }
        }
    }

    /// Appends task.
    pub(crate) fn append_task(&mut self, agent_id: AgentId, text: &str) -> bool {
        let Some(task_entry) = crate::terminals::task_entry_from_text(text) else {
            return false;
        };
        let existing = self
            .text(agent_id)
            .unwrap_or_default()
            .trim_end()
            .to_owned();
        let updated = if existing.is_empty() {
            task_entry
        } else {
            format!("{existing}\n{task_entry}")
        };
        self.set_text(agent_id, &updated)
    }

    /// Prepends task.
    pub(crate) fn prepend_task(&mut self, agent_id: AgentId, text: &str) -> bool {
        let Some(task_entry) = crate::terminals::task_entry_from_text(text) else {
            return false;
        };
        let existing = self
            .text(agent_id)
            .unwrap_or_default()
            .trim_end()
            .to_owned();
        let updated = if existing.is_empty() {
            task_entry
        } else {
            format!("{task_entry}\n{existing}")
        };
        self.set_text(agent_id, &updated)
    }

    /// Clears done.
    pub(crate) fn clear_done(&mut self, agent_id: AgentId) -> bool {
        let Some(text) = self.text(agent_id) else {
            return false;
        };
        let (updated, removed) = crate::terminals::clear_done_tasks(text);
        removed != 0 && self.set_text(agent_id, &updated)
    }

    /// Consumes next.
    pub(crate) fn consume_next(&mut self, agent_id: AgentId) -> Option<String> {
        let text = self.text(agent_id)?;
        let (message, updated) = crate::terminals::extract_next_task(text)?;
        if !message.trim().is_empty() {
            let _ = self.set_text(agent_id, &updated);
            return Some(message);
        }
        None
    }
}

#[derive(Resource, Default, Clone, Debug)]
pub(crate) struct MessageTransportAdapter;

/// Handles sync task notes projection.
pub(crate) fn sync_task_notes_projection(
    time: Res<Time>,
    runtime_index: Res<AgentRuntimeIndex>,
    task_store: Res<AgentTaskStore>,
    mut notes_state: ResMut<TerminalNotesState>,
) {
    let mut changed = false;
    for (agent_id, session_name) in runtime_index.session_bindings() {
        let next_text = task_store.text(agent_id).unwrap_or_default();
        changed |= notes_state.set_note_text(session_name, next_text);
    }
    if changed {
        mark_terminal_notes_dirty(&mut notes_state, Some(&time));
    }
}
