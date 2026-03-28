use crate::terminals::{TerminalId, TerminalLifecycle, TerminalRuntimeState};
use bevy::prelude::Resource;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct AgentId(pub(crate) u64);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum AgentKind {
    #[default]
    Terminal,
    Verifier,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentCapabilities {
    pub(crate) can_message: bool,
    pub(crate) has_tasks: bool,
    pub(crate) shell_spawnable: bool,
}

impl AgentCapabilities {
    pub(crate) const fn terminal_defaults() -> Self {
        Self {
            can_message: true,
            has_tasks: true,
            shell_spawnable: true,
        }
    }

    pub(crate) const fn verifier_defaults() -> Self {
        Self {
            can_message: false,
            has_tasks: false,
            shell_spawnable: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentRecord {
    pub(crate) label: String,
    pub(crate) kind: AgentKind,
    pub(crate) capabilities: AgentCapabilities,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentCatalog {
    next_id: u64,
    pub(crate) agents: BTreeMap<AgentId, AgentRecord>,
    pub(crate) order: Vec<AgentId>,
}

impl AgentCatalog {
    /// Creates agent.
    pub(crate) fn create_agent(
        &mut self,
        label: Option<String>,
        kind: AgentKind,
        capabilities: AgentCapabilities,
    ) -> AgentId {
        // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
        let id = AgentId(self.next_id.max(1));
        self.next_id = id.0 + 1;
        let display_index = self.order.len() + 1;
        self.agents.insert(
            id,
            AgentRecord {
                label: label.unwrap_or_else(|| format!("agent-{display_index}")),
                kind,
                capabilities,
            },
        );
        self.order.push(id);
        id
    }

    /// Handles remove.
    pub(crate) fn remove(&mut self, agent_id: AgentId) -> Option<AgentRecord> {
        let removed = self.agents.remove(&agent_id)?;
        self.order.retain(|existing| *existing != agent_id);
        Some(removed)
    }

    /// Returns the label.
    pub(crate) fn label(&self, agent_id: AgentId) -> Option<&str> {
        self.agents
            .get(&agent_id)
            .map(|record| record.label.as_str())
    }

    /// Returns the label for terminal.
    pub(crate) fn label_for_terminal(
        &self,
        runtime_index: &AgentRuntimeIndex,
        terminal_id: TerminalId,
    ) -> Option<&str> {
        runtime_index
            .agent_for_terminal(terminal_id)
            .and_then(|agent_id| self.label(agent_id))
    }

    /// Iterates iter.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (AgentId, &AgentRecord)> {
        self.order
            .iter()
            .copied()
            .filter_map(|agent_id| self.agents.get(&agent_id).map(|record| (agent_id, record)))
    }
}

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
    pub(crate) fn from_runtime(runtime: &TerminalRuntimeState) -> Self {
        match runtime.lifecycle {
            TerminalLifecycle::Running => Self::Running,
            TerminalLifecycle::Exited { .. } => Self::Exited,
            TerminalLifecycle::Disconnected => Self::Disconnected,
            TerminalLifecycle::Failed => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentRuntimeLink {
    pub(crate) primary_terminal: Option<TerminalId>,
    pub(crate) session_name: Option<String>,
    pub(crate) lifecycle: AgentRuntimeLifecycle,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentRuntimeIndex {
    pub(crate) agent_to_runtime: BTreeMap<AgentId, AgentRuntimeLink>,
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
        // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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

    /// Returns the runtime lifecycle state.
    #[cfg(test)]
    pub(crate) fn lifecycle(&self, agent_id: AgentId) -> Option<&AgentRuntimeLifecycle> {
        self.agent_to_runtime
            .get(&agent_id)
            .map(|link| &link.lifecycle)
    }
}

#[cfg(test)]
mod tests;
