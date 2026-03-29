use bevy::prelude::Resource;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct AgentId(pub(crate) u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    /// Validates that one explicit user-facing create label is non-empty after trimming and unique.
    pub(crate) fn validate_new_label(&self, label: Option<&str>) -> Result<Option<String>, String> {
        let Some(label) = normalize_requested_label(label) else {
            return Ok(None);
        };
        if self.label_exists(&label, None) {
            return Err(format!("agent `{label}` already exists"));
        }
        Ok(Some(label))
    }

    /// Validates that one explicit rename target is non-empty after trimming and unique.
    pub(crate) fn validate_rename_label(
        &self,
        agent_id: AgentId,
        label: &str,
    ) -> Result<String, String> {
        let label = normalize_requested_label(Some(label))
            .ok_or_else(|| "agent name is required".to_owned())?;
        if self.label_exists(&label, Some(agent_id)) {
            return Err(format!("agent `{label}` already exists"));
        }
        Ok(label)
    }

    /// Creates one agent with either a prevalidated explicit label or the next unique default label.
    pub(crate) fn create_agent(
        &mut self,
        label: Option<String>,
        kind: AgentKind,
        capabilities: AgentCapabilities,
    ) -> AgentId {
        let label = normalize_requested_label(label.as_deref())
            .unwrap_or_else(|| self.next_default_label());
        debug_assert!(
            !self.label_exists(&label, None),
            "create_agent requires a prevalidated unique label"
        );
        let id = AgentId(self.next_id.max(1));
        self.next_id = id.0 + 1;
        self.agents.insert(
            id,
            AgentRecord {
                label,
                kind,
                capabilities,
            },
        );
        self.order.push(id);
        id
    }

    /// Renames one existing agent using a prevalidated label.
    pub(crate) fn rename_agent(&mut self, agent_id: AgentId, label: String) -> Result<(), String> {
        let Some(record) = self.agents.get_mut(&agent_id) else {
            return Err(format!("unknown agent {}", agent_id.0));
        };
        record.label = label;
        Ok(())
    }

    /// Moves one agent to a specific display-order slot.
    pub(crate) fn move_to_index(&mut self, agent_id: AgentId, target_index: usize) -> bool {
        let Some(current_index) = self.order.iter().position(|existing| *existing == agent_id)
        else {
            return false;
        };
        let clamped_index = target_index.min(self.order.len().saturating_sub(1));
        if current_index == clamped_index {
            return false;
        }
        self.order.remove(current_index);
        self.order.insert(clamped_index, agent_id);
        true
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

    /// Iterates agents in current user-defined display order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (AgentId, &str)> {
        self.order.iter().copied().filter_map(|agent_id| {
            self.agents
                .get(&agent_id)
                .map(|record| (agent_id, record.label.as_str()))
        })
    }

    fn label_exists(&self, label: &str, excluding: Option<AgentId>) -> bool {
        self.agents
            .iter()
            .any(|(agent_id, record)| Some(*agent_id) != excluding && record.label == label)
    }

    fn next_default_label(&self) -> String {
        let mut display_index = self.order.len() + 1;
        loop {
            let candidate = format!("agent-{display_index}");
            if !self.label_exists(&candidate, None) {
                return candidate;
            }
            display_index += 1;
        }
    }
}

fn normalize_requested_label(label: Option<&str>) -> Option<String> {
    let label = label.map(str::trim)?;
    (!label.is_empty()).then(|| label.to_owned())
}
