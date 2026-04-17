mod catalog;
mod runtime_index;
mod status;

pub(crate) use crate::shared::agent_durability::AgentDurability;
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
mod tests {
    use super::{
        AgentCapabilities, AgentCatalog, AgentDurability, AgentId, AgentKind, AgentMetadata,
        AgentRuntimeIndex, AgentRuntimeLifecycle,
    };
    use crate::shared::{app_state_file::PersistedAgentKind, daemon_wire::DaemonAgentKind};
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
        assert_ne!(catalog.uid(alpha), catalog.uid(beta));
        assert_eq!(
            catalog.find_by_uid(catalog.uid(alpha).unwrap()),
            Some(alpha)
        );
        assert_eq!(catalog.find_by_uid(catalog.uid(beta).unwrap()), Some(beta));
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
        let alpha_uid = catalog.uid(alpha).unwrap().to_owned();
        let beta_uid = catalog.uid(beta).unwrap().to_owned();

        assert_eq!(
            catalog.validate_rename_label(beta, " alpha "),
            Err("agent `ALPHA` already exists".into())
        );
        let label = catalog.validate_rename_label(beta, " gamma ").unwrap();
        catalog.rename_agent(beta, label).unwrap();
        assert_eq!(catalog.label(beta), Some("GAMMA"));
        assert_eq!(catalog.label(alpha), Some("ALPHA"));
        assert_eq!(catalog.uid(alpha), Some(alpha_uid.as_str()));
        assert_eq!(catalog.uid(beta), Some(beta_uid.as_str()));
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
        let original_uids = [
            catalog.uid(alpha).unwrap().to_owned(),
            catalog.uid(beta).unwrap().to_owned(),
            catalog.uid(gamma).unwrap().to_owned(),
        ];

        assert!(catalog.move_to_index(gamma, 0));
        assert_eq!(catalog.order, vec![gamma, alpha, beta]);
        assert!(!catalog.move_to_index(gamma, 0));
        assert_eq!(catalog.uid(alpha), Some(original_uids[0].as_str()));
        assert_eq!(catalog.uid(beta), Some(original_uids[1].as_str()));
        assert_eq!(catalog.uid(gamma), Some(original_uids[2].as_str()));
    }

    #[test]
    fn catalog_pause_projects_agent_to_bottom_and_unpause_restores_canonical_position() {
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

        assert_eq!(catalog.display_order(), vec![alpha, beta, gamma]);
        assert_eq!(catalog.toggle_paused(beta), Ok(true));
        assert_eq!(catalog.display_order(), vec![alpha, gamma, beta]);
        assert_eq!(catalog.order, vec![alpha, beta, gamma]);

        assert!(catalog.move_to_index(gamma, 0));
        assert_eq!(catalog.display_order(), vec![gamma, alpha, beta]);
        assert_eq!(catalog.order, vec![gamma, beta, alpha]);

        assert_eq!(catalog.toggle_paused(beta), Ok(false));
        assert_eq!(catalog.display_order(), vec![gamma, beta, alpha]);
        assert_eq!(catalog.order, vec![gamma, beta, alpha]);
    }

    /// Verifies that durable clone/workdir metadata stays attached to the agent record.
    #[test]
    fn catalog_retains_clone_provenance_and_workdir_metadata() {
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent_with_metadata(
            Some("alpha".into()),
            AgentKind::Pi,
            AgentKind::Pi.capabilities(),
            AgentMetadata {
                clone_source_session_path: Some("/tmp/pi-session-alpha.jsonl".into()),
                recovery: Some(super::AgentRecoverySpec::Pi {
                    session_path: "/tmp/pi-session-alpha.jsonl".into(),
                    cwd: "/tmp/demo".into(),
                    is_workdir: true,
                    workdir_slug: None,
                }),
            },
        );

        assert_eq!(
            catalog.clone_source_session_path(agent_id),
            Some("/tmp/pi-session-alpha.jsonl")
        );
        assert!(catalog.is_workdir(agent_id));
        assert_eq!(catalog.kind(agent_id), Some(AgentKind::Pi));
    }

    #[test]
    fn durability_classification_is_explicit_for_live_only_and_recoverable_agents() {
        assert_eq!(
            AgentCatalog::classify_durability(AgentKind::Terminal, None),
            AgentDurability::LiveOnly
        );
        assert_eq!(
            AgentCatalog::classify_durability(
                AgentKind::Verifier,
                Some(&super::AgentRecoverySpec::Claude {
                    session_id: "session-1".into(),
                    cwd: "/tmp/demo".into(),
                    model: None,
                    profile: None,
                })
            ),
            AgentDurability::Recoverable
        );
    }

    #[test]
    fn catalog_reports_agent_durability_without_callers_peeking_at_recovery_presence() {
        let mut catalog = AgentCatalog::default();
        let live_only = catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Terminal,
            AgentCapabilities::terminal_defaults(),
        );
        let recoverable = catalog.create_agent_with_metadata(
            Some("beta".into()),
            AgentKind::Pi,
            AgentKind::Pi.capabilities(),
            AgentMetadata {
                clone_source_session_path: Some("/tmp/pi-session-beta.jsonl".into()),
                recovery: Some(super::AgentRecoverySpec::Pi {
                    session_path: "/tmp/pi-session-beta.jsonl".into(),
                    cwd: "/tmp/demo".into(),
                    is_workdir: false,
                    workdir_slug: None,
                }),
            },
        );

        assert_eq!(
            catalog.durability(live_only),
            Some(AgentDurability::LiveOnly)
        );
        assert_eq!(
            catalog.durability(recoverable),
            Some(AgentDurability::Recoverable)
        );
    }

    /// Verifies that runtime index links terminal session and runtime state.
    #[test]
    fn agent_kind_conversion_helpers_cover_all_persisted_and_daemon_variants() {
        let cases = [
            (AgentKind::Pi, PersistedAgentKind::Pi, DaemonAgentKind::Pi),
            (
                AgentKind::Claude,
                PersistedAgentKind::Claude,
                DaemonAgentKind::Claude,
            ),
            (
                AgentKind::Codex,
                PersistedAgentKind::Codex,
                DaemonAgentKind::Codex,
            ),
            (
                AgentKind::Terminal,
                PersistedAgentKind::Terminal,
                DaemonAgentKind::Terminal,
            ),
            (
                AgentKind::Verifier,
                PersistedAgentKind::Verifier,
                DaemonAgentKind::Verifier,
            ),
        ];

        for (agent_kind, persisted_kind, daemon_kind) in cases {
            assert_eq!(agent_kind.persisted_kind(), persisted_kind);
            assert_eq!(agent_kind.daemon_kind(), daemon_kind);
            assert_eq!(AgentKind::from_persisted_kind(persisted_kind), agent_kind);
            assert_eq!(AgentKind::from_daemon_kind(daemon_kind), agent_kind);
        }
    }

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
}
