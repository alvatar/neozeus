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
    assert_eq!(catalog.label(alpha), Some("AGENT-1"));
    assert_eq!(catalog.label(beta), Some("AGENT-2"));
}

/// Verifies that explicit agent labels must be unique while default labels skip occupied names.
#[test]
fn catalog_rejects_duplicate_labels_and_skips_taken_default_names() {
    let mut catalog = AgentCatalog::default();
    let initial_label = catalog.validate_new_label(Some("agent-1")).unwrap();
    assert_eq!(initial_label, Some("AGENT-1".into()));
    let _ = catalog.create_agent(
        initial_label,
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );

    let generated = catalog.create_agent(
        None,
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );

    assert_eq!(catalog.label(generated), Some("AGENT-2"));
    assert_eq!(
        catalog.validate_new_label(Some("agent-1")),
        Err("agent `AGENT-1` already exists".into())
    );
    assert_eq!(
        catalog.validate_new_label(Some("AGENT-1")),
        Err("agent `AGENT-1` already exists".into())
    );
}

/// Verifies that renaming also enforces uniqueness and trims outer whitespace.
#[test]
fn catalog_rename_rejects_duplicates() {
    let mut catalog = AgentCatalog::default();
    let alpha = catalog.create_agent(
        Some("alpha".into()),
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );
    let beta = catalog.create_agent(
        Some("beta".into()),
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );

    assert_eq!(
        catalog.validate_rename_label(beta, " alpha "),
        Err("agent `ALPHA` already exists".into())
    );
    let label = catalog.validate_rename_label(beta, " gamma ").unwrap();
    catalog.rename_agent(beta, label).unwrap();
    assert_eq!(catalog.label(beta), Some("GAMMA"));
    assert_eq!(catalog.label(alpha), Some("ALPHA"));
}

/// Verifies that moving one agent updates the retained display order deterministically.
#[test]
fn catalog_move_to_index_reorders_agents() {
    let mut catalog = AgentCatalog::default();
    let alpha = catalog.create_agent(
        Some("alpha".into()),
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );
    let beta = catalog.create_agent(
        Some("beta".into()),
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );
    let gamma = catalog.create_agent(
        Some("gamma".into()),
        AgentKind::Terminal,
        AgentCapabilities::terminal_defaults(),
    );

    assert!(catalog.move_to_index(gamma, 0));
    assert_eq!(catalog.order, vec![gamma, alpha, beta]);
    assert!(!catalog.move_to_index(gamma, 0));
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
