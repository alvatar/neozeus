use crate::shared::daemon_wire::DaemonSessionMetrics;
use bevy::prelude::*;
use std::collections::BTreeMap;

use super::{DaemonSessionInfo, TerminalRuntimeSpawner};

const SESSION_METRICS_SYNC_INTERVAL_SECS: f32 = 1.0;

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct LiveSessionMetricsStore {
    metrics_by_session: BTreeMap<String, DaemonSessionMetrics>,
    pub(crate) last_error: Option<String>,
    last_sync_secs: Option<f32>,
}

impl LiveSessionMetricsStore {
    pub(crate) fn metrics(&self, session_id: &str) -> Option<&DaemonSessionMetrics> {
        self.metrics_by_session.get(session_id)
    }

    #[cfg(test)]
    pub(crate) fn set_metrics_for_tests(
        &mut self,
        session_id: &str,
        metrics: DaemonSessionMetrics,
    ) {
        self.metrics_by_session
            .insert(session_id.to_owned(), metrics);
        self.last_error = None;
    }

    fn replace_from_sessions(&mut self, sessions: Vec<DaemonSessionInfo>) -> bool {
        let metrics_by_session = sessions
            .into_iter()
            .map(|session| (session.session_id, session.metrics))
            .collect::<BTreeMap<_, _>>();
        let changed = self.metrics_by_session != metrics_by_session || self.last_error.is_some();
        self.metrics_by_session = metrics_by_session;
        self.last_error = None;
        changed
    }

    fn record_refresh_error(&mut self, error: String) -> bool {
        let changed = self.last_error.as_deref() != Some(error.as_str());
        self.last_error = Some(error);
        changed
    }
}

pub(crate) fn refresh_live_session_metrics_now(
    runtime_spawner: &TerminalRuntimeSpawner,
    store: &mut LiveSessionMetricsStore,
) -> Result<bool, String> {
    match runtime_spawner.list_session_infos() {
        Ok(sessions) => Ok(store.replace_from_sessions(sessions)),
        Err(error) if error == "terminal runtime still connecting" => Err(error),
        Err(error) => {
            let _ = store.record_refresh_error(error.clone());
            Err(error)
        }
    }
}

pub(crate) fn sync_live_session_metrics(
    time: Res<Time>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    mut store: ResMut<LiveSessionMetricsStore>,
    mut redraws: MessageWriter<bevy::window::RequestRedraw>,
) {
    let now_secs = time.elapsed_secs();
    if store.last_sync_secs.is_some_and(|last_sync_secs| {
        now_secs - last_sync_secs < SESSION_METRICS_SYNC_INTERVAL_SECS
    }) {
        return;
    }
    store.last_sync_secs = Some(now_secs);
    if matches!(
        refresh_live_session_metrics_now(&runtime_spawner, &mut store),
        Ok(true)
    ) {
        redraws.write(bevy::window::RequestRedraw);
    }
}

#[cfg(test)]
mod tests {
    use super::{refresh_live_session_metrics_now, LiveSessionMetricsStore};
    use crate::{
        shared::daemon_wire::DaemonSessionMetrics,
        terminals::{
            AttachedDaemonSession, DaemonSessionInfo, OwnedTmuxSessionInfo, TerminalCommand,
            TerminalDaemonClient, TerminalDaemonClientResource, TerminalRuntimeSpawner,
            TerminalRuntimeState,
        },
    };
    use std::sync::Arc;

    #[derive(Default)]
    struct FakeDaemonClient {
        sessions: Vec<DaemonSessionInfo>,
    }

    impl TerminalDaemonClient for FakeDaemonClient {
        fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
            Ok(self.sessions.clone())
        }

        fn update_session_metadata(
            &self,
            _session_id: &str,
            _metadata: &crate::shared::daemon_wire::DaemonSessionMetadata,
        ) -> Result<(), String> {
            Ok(())
        }

        fn create_session_with_env(
            &self,
            _prefix: &str,
            _cwd: Option<&str>,
            _env_overrides: &[(String, String)],
        ) -> Result<String, String> {
            Err("not needed".into())
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
            Err("not needed".into())
        }

        fn capture_owned_tmux_session(
            &self,
            _session_uid: &str,
            _lines: usize,
        ) -> Result<String, String> {
            Err("not needed".into())
        }

        fn kill_owned_tmux_session(&self, _session_uid: &str) -> Result<(), String> {
            Err("not needed".into())
        }

        fn kill_owned_tmux_sessions_for_agent(&self, _owner_agent_uid: &str) -> Result<(), String> {
            Err("not needed".into())
        }

        fn attach_session(&self, _session_id: &str) -> Result<AttachedDaemonSession, String> {
            Err("not needed".into())
        }

        fn send_command(&self, _session_id: &str, _command: TerminalCommand) -> Result<(), String> {
            Err("not needed".into())
        }

        fn resize_session(
            &self,
            _session_id: &str,
            _cols: usize,
            _rows: usize,
        ) -> Result<(), String> {
            Err("not needed".into())
        }

        fn kill_session(&self, _session_id: &str) -> Result<(), String> {
            Err("not needed".into())
        }
    }

    #[test]
    fn refresh_live_session_metrics_now_replaces_metrics_by_session() {
        let runtime_spawner = TerminalRuntimeSpawner::for_tests(
            TerminalDaemonClientResource::from_client(Arc::new(FakeDaemonClient {
                sessions: vec![DaemonSessionInfo {
                    session_id: "neozeus-session-1".into(),
                    runtime: TerminalRuntimeState::running("running"),
                    revision: 1,
                    created_order: 1,
                    metadata: crate::shared::daemon_wire::DaemonSessionMetadata::default(),
                    metrics: DaemonSessionMetrics {
                        cpu_pct_milli: Some(12_300),
                        ram_bytes: Some(64 * 1024 * 1024),
                        net_rx_bytes_per_sec: Some(1024),
                        net_tx_bytes_per_sec: Some(2048),
                    },
                }],
            })),
        );
        let mut store = LiveSessionMetricsStore::default();

        assert!(refresh_live_session_metrics_now(&runtime_spawner, &mut store).unwrap());
        assert_eq!(
            store.metrics("neozeus-session-1"),
            Some(&DaemonSessionMetrics {
                cpu_pct_milli: Some(12_300),
                ram_bytes: Some(64 * 1024 * 1024),
                net_rx_bytes_per_sec: Some(1024),
                net_tx_bytes_per_sec: Some(2048),
            })
        );
        assert_eq!(store.last_error, None);
    }
}
