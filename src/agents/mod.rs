mod catalog;
mod runtime_index;
mod status;

pub(crate) use catalog::{AgentCapabilities, AgentCatalog, AgentId, AgentKind};
pub(crate) use runtime_index::AgentRuntimeIndex;
pub(crate) use status::{sync_agent_status, AgentStatus, AgentStatusStore};

#[cfg(test)]
pub(crate) use runtime_index::AgentRuntimeLifecycle;

#[cfg(test)]
mod tests;
