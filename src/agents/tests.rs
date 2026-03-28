use super::{
    AgentCapabilities, AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex, AgentRuntimeLifecycle,
};
use crate::terminals::{TerminalId, TerminalRuntimeState};

/// Verifies that catalog assigns stable default labels in creation order.
#[test]
fn catalog_assigns_stable_default_labels_in_creation_order() {
    let mut catalog = AgentCatalog::default();
    let alpha = catalog.create_agent(
        None,
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );
    let beta = catalog.create_agent(
        None,
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );

    assert_eq!(alpha, AgentId(1));
    assert_eq!(beta, AgentId(2));
    assert_eq!(catalog.label(alpha), Some("agent-1"));
    assert_eq!(catalog.label(beta), Some("agent-2"));
}

/// Verifies that runtime index links terminal session and runtime state.
#[test]
fn runtime_index_links_terminal_session_and_runtime_state() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut runtime_index = AgentRuntimeIndex::default();
    let runtime = TerminalRuntimeState::running("running");

    runtime_index.link_terminal(
        AgentId(7),
        TerminalId(9),
        "session-9".into(),
        Some(&runtime),
    );

    assert_eq!(
        runtime_index.agent_for_terminal(TerminalId(9)),
        Some(AgentId(7))
    );
    assert_eq!(
        runtime_index.agent_for_session("session-9"),
        Some(AgentId(7))
    );
    assert_eq!(
        runtime_index.primary_terminal(AgentId(7)),
        Some(TerminalId(9))
    );
    assert_eq!(runtime_index.session_name(AgentId(7)), Some("session-9"));
    assert_eq!(
        runtime_index.lifecycle(AgentId(7)),
        Some(&AgentRuntimeLifecycle::Running)
    );
}

/// Verifies that runtime index remove terminal clears reverse indexes.
#[test]
fn runtime_index_remove_terminal_clears_reverse_indexes() {
    let mut runtime_index = AgentRuntimeIndex::default();
    runtime_index.link_terminal(
        AgentId(4),
        TerminalId(3),
        "session-3".into(),
        Some(&TerminalRuntimeState::running("ok")),
    );

    assert_eq!(
        runtime_index.remove_terminal(TerminalId(3)),
        Some(AgentId(4))
    );
    assert_eq!(runtime_index.agent_for_terminal(TerminalId(3)), None);
    assert_eq!(runtime_index.agent_for_session("session-3"), None);
    assert_eq!(runtime_index.primary_terminal(AgentId(4)), None);
}
