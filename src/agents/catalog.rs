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
struct AgentRecord {
    pub(crate) label: String,
    pub(crate) kind: AgentKind,
    pub(crate) capabilities: AgentCapabilities,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentCatalog {
    next_id: u64,
    agents: BTreeMap<AgentId, AgentRecord>,
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
    pub(crate) fn remove(&mut self, agent_id: AgentId) -> bool {
        let removed = self.agents.remove(&agent_id).is_some();
        if removed {
            self.order.retain(|existing| *existing != agent_id);
        }
        removed
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
        runtime_index: &super::AgentRuntimeIndex,
        terminal_id: crate::terminals::TerminalId,
    ) -> Option<&str> {
        runtime_index
            .agent_for_terminal(terminal_id)
            .and_then(|agent_id| self.label(agent_id))
    }

    /// Iterates iter.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (AgentId, &str)> {
        self.order.iter().copied().filter_map(|agent_id| {
            self.agents
                .get(&agent_id)
                .map(|record| (agent_id, record.label.as_str()))
        })
    }
}
