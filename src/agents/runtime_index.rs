use crate::terminals::{TerminalId, TerminalLifecycle, TerminalRuntimeState};
use bevy::prelude::Resource;
use std::collections::BTreeMap;

use super::catalog::AgentId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum AgentRuntimeLifecycle {
    #[default]
    Unknown,
    Running,
    Exited,
    Disconnected,
    Failed,
}

impl AgentRuntimeLifecycle {
    /// Builds runtime from the supplied source data.
    fn from_runtime(runtime: &TerminalRuntimeState) -> Self {
        match runtime.lifecycle {
            TerminalLifecycle::Running => Self::Running,
            TerminalLifecycle::Exited { .. } => Self::Exited,
            TerminalLifecycle::Disconnected => Self::Disconnected,
            TerminalLifecycle::Failed => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AgentRuntimeLink {
    primary_terminal: Option<TerminalId>,
    session_name: Option<String>,
    lifecycle: AgentRuntimeLifecycle,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentRuntimeIndex {
    agent_to_runtime: BTreeMap<AgentId, AgentRuntimeLink>,
    pub(crate) terminal_to_agent: BTreeMap<TerminalId, AgentId>,
    pub(crate) session_to_agent: BTreeMap<String, AgentId>,
}

impl AgentRuntimeIndex {
    /// Links terminal.
    pub(crate) fn link_terminal(
        &mut self,
        agent_id: AgentId,
        terminal_id: TerminalId,
        session_name: String,
        runtime: Option<&TerminalRuntimeState>,
    ) {
        let lifecycle = runtime
            .map(AgentRuntimeLifecycle::from_runtime)
            .unwrap_or_default();
        self.terminal_to_agent.insert(terminal_id, agent_id);
        self.session_to_agent.insert(session_name.clone(), agent_id);
        self.agent_to_runtime.insert(
            agent_id,
            AgentRuntimeLink {
                primary_terminal: Some(terminal_id),
                session_name: Some(session_name),
                lifecycle,
            },
        );
    }

    /// Updates runtime.
    pub(crate) fn update_runtime(
        &mut self,
        terminal_id: TerminalId,
        runtime: &TerminalRuntimeState,
    ) {
        let Some(agent_id) = self.terminal_to_agent.get(&terminal_id).copied() else {
            return;
        };
        if let Some(link) = self.agent_to_runtime.get_mut(&agent_id) {
            link.lifecycle = AgentRuntimeLifecycle::from_runtime(runtime);
        }
    }

    /// Removes terminal.
    pub(crate) fn remove_terminal(&mut self, terminal_id: TerminalId) -> Option<AgentId> {
        let agent_id = self.terminal_to_agent.remove(&terminal_id)?;
        if let Some(link) = self.agent_to_runtime.remove(&agent_id) {
            if let Some(session_name) = link.session_name {
                self.session_to_agent.remove(&session_name);
            }
        }
        Some(agent_id)
    }

    /// Returns the agent for terminal.
    pub(crate) fn agent_for_terminal(&self, terminal_id: TerminalId) -> Option<AgentId> {
        self.terminal_to_agent.get(&terminal_id).copied()
    }

    /// Returns the agent for session.
    pub(crate) fn agent_for_session(&self, session_name: &str) -> Option<AgentId> {
        self.session_to_agent.get(session_name).copied()
    }

    /// Handles primary terminal.
    pub(crate) fn primary_terminal(&self, agent_id: AgentId) -> Option<TerminalId> {
        self.agent_to_runtime
            .get(&agent_id)
            .and_then(|link| link.primary_terminal)
    }

    /// Returns the session name.
    pub(crate) fn session_name(&self, agent_id: AgentId) -> Option<&str> {
        self.agent_to_runtime
            .get(&agent_id)
            .and_then(|link| link.session_name.as_deref())
    }

    /// Iterates linked agents.
    #[cfg(test)]
    pub(crate) fn agent_ids(&self) -> impl Iterator<Item = AgentId> + '_ {
        self.agent_to_runtime.keys().copied()
    }

    /// Iterates agent/session bindings.
    pub(crate) fn session_bindings(&self) -> impl Iterator<Item = (AgentId, &str)> + '_ {
        self.agent_to_runtime.iter().filter_map(|(agent_id, link)| {
            link.session_name
                .as_deref()
                .map(|session_name| (*agent_id, session_name))
        })
    }

    /// Returns the runtime lifecycle state.
    #[cfg(test)]
    pub(crate) fn lifecycle(&self, agent_id: AgentId) -> Option<&AgentRuntimeLifecycle> {
        self.agent_to_runtime
            .get(&agent_id)
            .map(|link| &link.lifecycle)
    }
}
