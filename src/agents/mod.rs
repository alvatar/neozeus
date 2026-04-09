mod catalog;
mod runtime_index;
mod status;

pub(crate) use catalog::{
    uppercase_agent_label_text, AgentCatalog, AgentId, AgentKind, AgentMetadata, AgentRecoverySpec,
    PendingAgentIdentity,
};

#[cfg(test)]
pub(crate) use catalog::AgentCapabilities;
pub(crate) use runtime_index::AgentRuntimeIndex;
#[cfg(test)]
pub(crate) use status::parse_agent_context_pct_milli;
pub(crate) use status::{sync_agent_status, AgentStatus, AgentStatusStore};

#[cfg(test)]
pub(crate) use runtime_index::AgentRuntimeLifecycle;

#[cfg(test)]
mod tests;
