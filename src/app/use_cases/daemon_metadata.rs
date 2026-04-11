use crate::{
    agents::{AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex},
    shared::daemon_wire::{DaemonAgentKind, DaemonSessionMetadata},
    terminals::TerminalRuntimeSpawner,
};

fn daemon_agent_kind(kind: AgentKind) -> DaemonAgentKind {
    match kind {
        AgentKind::Pi => DaemonAgentKind::Pi,
        AgentKind::Claude => DaemonAgentKind::Claude,
        AgentKind::Codex => DaemonAgentKind::Codex,
        AgentKind::Terminal => DaemonAgentKind::Terminal,
        AgentKind::Verifier => DaemonAgentKind::Verifier,
    }
}

/// Pushes app-owned agent identity into one live daemon session's metadata.
///
/// Ownership contract:
/// - session existence/runtime lifecycle belongs to the daemon + runtime index,
/// - agent uid/label/kind belong to the app catalog,
/// - recovery metadata plus conversations/tasks/Aegis remain app-only and must never be copied into
///   daemon session metadata.
///
/// `Ok(false)` means there is currently no bound live session to mirror into.
pub(crate) fn sync_session_agent_metadata(
    runtime_spawner: &TerminalRuntimeSpawner,
    session_name: Option<&str>,
    agent_uid: &str,
    agent_label: &str,
    agent_kind: AgentKind,
) -> Result<bool, String> {
    let Some(session_name) = session_name else {
        return Ok(false);
    };
    let metadata = DaemonSessionMetadata {
        agent_uid: Some(agent_uid.to_owned()),
        agent_label: Some(agent_label.to_owned()),
        agent_kind: Some(daemon_agent_kind(agent_kind)),
    };
    runtime_spawner.update_session_metadata(session_name, &metadata)?;
    Ok(true)
}

/// Mirrors the current app-catalog identity for one agent into its bound daemon session, if any.
pub(crate) fn sync_agent_metadata_to_daemon(
    runtime_spawner: &TerminalRuntimeSpawner,
    runtime_index: &AgentRuntimeIndex,
    agent_catalog: &AgentCatalog,
    agent_id: AgentId,
) -> Result<bool, String> {
    let agent_uid = agent_catalog
        .uid(agent_id)
        .ok_or_else(|| format!("missing stable uid for agent {}", agent_id.0))?;
    let agent_label = agent_catalog
        .label(agent_id)
        .ok_or_else(|| format!("missing label for agent {}", agent_id.0))?;
    let agent_kind = agent_catalog
        .kind(agent_id)
        .ok_or_else(|| format!("missing kind for agent {}", agent_id.0))?;
    sync_session_agent_metadata(
        runtime_spawner,
        runtime_index.session_name(agent_id),
        agent_uid,
        agent_label,
        agent_kind,
    )
}

#[cfg(test)]
mod tests {
    use super::{sync_agent_metadata_to_daemon, sync_session_agent_metadata};
    use crate::{
        agents::{AgentCatalog, AgentKind, AgentRuntimeIndex},
        terminals::{
            AttachedDaemonSession, DaemonSessionInfo, OwnedTmuxSessionInfo, TerminalCommand,
            TerminalDaemonClient, TerminalDaemonClientResource, TerminalRuntimeSpawner,
            TerminalRuntimeState,
        },
    };
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MetadataDaemonClient {
        updated: Mutex<Vec<(String, crate::shared::daemon_wire::DaemonSessionMetadata)>>,
    }

    impl TerminalDaemonClient for MetadataDaemonClient {
        fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
            Ok(Vec::new())
        }

        fn update_session_metadata(
            &self,
            session_id: &str,
            metadata: &crate::shared::daemon_wire::DaemonSessionMetadata,
        ) -> Result<(), String> {
            self.updated
                .lock()
                .unwrap()
                .push((session_id.to_owned(), metadata.clone()));
            Ok(())
        }

        fn create_session_with_env(
            &self,
            _prefix: &str,
            _cwd: Option<&str>,
            _env_overrides: &[(String, String)],
        ) -> Result<String, String> {
            Err("unused in test".into())
        }

        fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String> {
            Ok(Vec::new())
        }

        fn create_owned_tmux_session(
            &self,
            _owner_agent_uid: &str,
            _display_name: &str,
            _cwd: Option<&str>,
            _command: &str,
        ) -> Result<OwnedTmuxSessionInfo, String> {
            Err("unused in test".into())
        }

        fn capture_owned_tmux_session(
            &self,
            _session_uid: &str,
            _lines: usize,
        ) -> Result<String, String> {
            Err("unused in test".into())
        }

        fn kill_owned_tmux_session(&self, _session_uid: &str) -> Result<(), String> {
            Err("unused in test".into())
        }

        fn kill_owned_tmux_sessions_for_agent(&self, _owner_agent_uid: &str) -> Result<(), String> {
            Err("unused in test".into())
        }

        fn attach_session(&self, _session_id: &str) -> Result<AttachedDaemonSession, String> {
            Err("unused in test".into())
        }

        fn send_command(&self, _session_id: &str, _command: TerminalCommand) -> Result<(), String> {
            Err("unused in test".into())
        }

        fn resize_session(
            &self,
            _session_id: &str,
            _cols: usize,
            _rows: usize,
        ) -> Result<(), String> {
            Err("unused in test".into())
        }

        fn kill_session(&self, _session_id: &str) -> Result<(), String> {
            Err("unused in test".into())
        }
    }

    #[test]
    fn sync_session_agent_metadata_is_safe_when_no_session_is_bound() {
        let daemon = Arc::new(MetadataDaemonClient::default());
        let runtime_spawner =
            TerminalRuntimeSpawner::for_tests(TerminalDaemonClientResource::from_client(daemon));

        assert!(!sync_session_agent_metadata(
            &runtime_spawner,
            None,
            "agent-uid-1",
            "ALPHA",
            AgentKind::Pi
        )
        .unwrap());
    }

    #[test]
    fn sync_agent_metadata_to_daemon_pushes_uid_label_and_kind() {
        let daemon = Arc::new(MetadataDaemonClient::default());
        let runtime_spawner = TerminalRuntimeSpawner::for_tests(
            TerminalDaemonClientResource::from_client(daemon.clone()),
        );
        let mut agent_catalog = AgentCatalog::default();
        let agent_id = agent_catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Claude,
            AgentKind::Claude.capabilities(),
        );
        let agent_uid = agent_catalog.uid(agent_id).unwrap().to_owned();
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(
            agent_id,
            crate::terminals::TerminalId(1),
            "neozeus-session-a".into(),
            Some(&TerminalRuntimeState::running("ready")),
        );

        assert!(sync_agent_metadata_to_daemon(
            &runtime_spawner,
            &runtime_index,
            &agent_catalog,
            agent_id
        )
        .unwrap());

        let updated = daemon.updated.lock().unwrap();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].0, "neozeus-session-a");
        assert_eq!(updated[0].1.agent_uid.as_deref(), Some(agent_uid.as_str()));
        assert_eq!(updated[0].1.agent_label.as_deref(), Some("ALPHA"));
        assert_eq!(
            updated[0].1.agent_kind,
            Some(crate::shared::daemon_wire::DaemonAgentKind::Claude)
        );
    }
}
