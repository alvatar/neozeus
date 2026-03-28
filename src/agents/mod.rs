mod catalog;
mod runtime_index;

pub(crate) use catalog::{AgentCapabilities, AgentCatalog, AgentId, AgentKind};
pub(crate) use runtime_index::AgentRuntimeIndex;

#[cfg(test)]
pub(crate) use runtime_index::AgentRuntimeLifecycle;

#[cfg(test)]
mod tests;
