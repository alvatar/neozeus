use bevy::prelude::Resource;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct AgentId(pub(crate) u64);

pub(crate) type AgentUid = String;

static NEXT_AGENT_UID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AgentKind {
    Pi,
    Claude,
    Codex,
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

impl AgentKind {
    pub(crate) const fn capabilities(self) -> AgentCapabilities {
        match self {
            Self::Pi | Self::Claude | Self::Codex | Self::Terminal => {
                AgentCapabilities::terminal_defaults()
            }
            Self::Verifier => AgentCapabilities::verifier_defaults(),
        }
    }

    pub(crate) const fn bootstrap_command(self) -> Option<&'static str> {
        match self {
            Self::Pi => Some("pi"),
            Self::Claude => Some("claude"),
            Self::Codex => Some("codex"),
            Self::Terminal | Self::Verifier => None,
        }
    }

    pub(crate) const fn env_name(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Terminal => "terminal",
            Self::Verifier => "verifier",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingAgentIdentity {
    pub(crate) uid: AgentUid,
    pub(crate) label: String,
    pub(crate) kind: AgentKind,
    pub(crate) capabilities: AgentCapabilities,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentRecord {
    pub(crate) uid: AgentUid,
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

pub(crate) fn uppercase_agent_label_text(text: &str) -> String {
    text.to_uppercase()
}

fn generate_agent_uid() -> AgentUid {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = NEXT_AGENT_UID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("agent-{now_nanos:032x}-{counter:016x}")
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

    /// Allocates a stable pending identity without inserting the agent yet.
    pub(crate) fn allocate_identity(
        &self,
        label: Option<&str>,
        kind: AgentKind,
        capabilities: AgentCapabilities,
    ) -> Result<PendingAgentIdentity, String> {
        let label = self
            .validate_new_label(label)?
            .unwrap_or_else(|| self.next_default_label());
        Ok(PendingAgentIdentity {
            uid: generate_agent_uid(),
            label,
            kind,
            capabilities,
        })
    }

    /// Creates one agent from a prevalidated pending identity.
    pub(crate) fn create_agent_from_identity(&mut self, identity: PendingAgentIdentity) -> AgentId {
        debug_assert!(
            !self.label_exists(&identity.label, None),
            "create_agent_from_identity requires a unique label"
        );
        debug_assert!(
            self.find_by_uid(&identity.uid).is_none(),
            "create_agent_from_identity requires a unique uid"
        );
        let id = AgentId(self.next_id.max(1));
        self.next_id = id.0 + 1;
        self.agents.insert(
            id,
            AgentRecord {
                uid: identity.uid,
                label: identity.label,
                kind: identity.kind,
                capabilities: identity.capabilities,
            },
        );
        self.order.push(id);
        id
    }

    /// Creates one agent with either a prevalidated explicit label or the next unique default label.
    pub(crate) fn create_agent(
        &mut self,
        label: Option<String>,
        kind: AgentKind,
        capabilities: AgentCapabilities,
    ) -> AgentId {
        let identity = self
            .allocate_identity(label.as_deref(), kind, capabilities)
            .expect("create_agent label allocation should only fail on duplicate labels");
        self.create_agent_from_identity(identity)
    }

    /// Creates one agent with an explicit stable uid.
    pub(crate) fn create_agent_with_uid(
        &mut self,
        uid: AgentUid,
        label: Option<String>,
        kind: AgentKind,
        capabilities: AgentCapabilities,
    ) -> AgentId {
        let label = normalize_requested_label(label.as_deref())
            .unwrap_or_else(|| self.next_default_label());
        debug_assert!(
            !self.label_exists(&label, None),
            "create_agent_with_uid requires a unique label"
        );
        debug_assert!(
            self.find_by_uid(&uid).is_none(),
            "create_agent_with_uid requires a unique uid"
        );
        let id = AgentId(self.next_id.max(1));
        self.next_id = id.0 + 1;
        self.agents.insert(
            id,
            AgentRecord {
                uid,
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

    /// Returns the stable uid.
    pub(crate) fn uid(&self, agent_id: AgentId) -> Option<&str> {
        self.agents.get(&agent_id).map(|record| record.uid.as_str())
    }

    /// Resolves one runtime agent id from a stable uid.
    pub(crate) fn find_by_uid(&self, uid: &str) -> Option<AgentId> {
        self.agents
            .iter()
            .find_map(|(agent_id, record)| (record.uid == uid).then_some(*agent_id))
    }

    /// Returns the label.
    pub(crate) fn label(&self, agent_id: AgentId) -> Option<&str> {
        self.agents
            .get(&agent_id)
            .map(|record| record.label.as_str())
    }

    /// Returns the retained kind metadata for one agent.
    pub(crate) fn kind(&self, agent_id: AgentId) -> Option<AgentKind> {
        self.agents.get(&agent_id).map(|record| record.kind)
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
            let candidate = format!("AGENT-{display_index}");
            if !self.label_exists(&candidate, None) {
                return candidate;
            }
            display_index += 1;
        }
    }
}

fn normalize_requested_label(label: Option<&str>) -> Option<String> {
    let label = label.map(str::trim)?;
    (!label.is_empty()).then(|| uppercase_agent_label_text(label))
}
