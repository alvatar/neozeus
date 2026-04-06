use super::super::debug::append_debug_log;
use super::super::types::{TerminalCommand, TerminalSnapshot, TerminalUpdate};
use super::owned_tmux::OwnedTmuxSessionInfo;
use super::protocol::{
    read_server_message, write_client_message, ClientMessage, DaemonEvent, DaemonRequest,
    DaemonResponse, DaemonSessionInfo, ServerMessage,
};
use crate::shared::{
    daemon_client_core::{spawn_daemon_subprocess, wait_for_connect, SocketRequestClientCore},
    daemon_socket::resolve_daemon_socket_path,
};
use bevy::prelude::Resource;
use std::{
    collections::HashMap,
    path::Path,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

const DAEMON_CONNECT_RETRY_TIMEOUT: Duration = Duration::from_secs(2);
const DAEMON_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub(crate) struct AttachedDaemonSession {
    pub(crate) snapshot: TerminalSnapshot,
    pub(crate) updates: mpsc::Receiver<TerminalUpdate>,
}

pub(crate) trait TerminalDaemonClient: Send + Sync {
    /// Returns the daemon's current session list with runtime/revision metadata.
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String>;
    /// Updates mutable session metadata stored by the daemon.
    fn update_session_metadata_label(
        &self,
        session_id: &str,
        agent_label: Option<&str>,
    ) -> Result<(), String>;
    /// Asks the daemon to create a new session id using the provided prefix, optional working directory,
    /// and per-session environment overrides.
    fn create_session_with_env(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        env_overrides: &[(String, String)],
    ) -> Result<String, String>;
    #[allow(
        dead_code,
        reason = "trait convenience helper remains available for tests and direct clients"
    )]
    /// Asks the daemon to create a new session id using the provided prefix and optional working directory.
    fn create_session(&self, prefix: &str, cwd: Option<&str>) -> Result<String, String> {
        self.create_session_with_env(prefix, cwd, &[])
    }
    /// Lists all persistent agent-owned tmux child sessions currently discoverable by the daemon.
    fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String>;
    #[allow(
        dead_code,
        reason = "owned tmux creation is exercised through the concrete socket client and helper CLI"
    )]
    /// Creates one persistent agent-owned tmux session.
    fn create_owned_tmux_session(
        &self,
        owner_agent_uid: &str,
        display_name: &str,
        cwd: Option<&str>,
        command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String>;
    /// Captures one owned tmux session's pane text for read-only inspection.
    fn capture_owned_tmux_session(&self, session_uid: &str, lines: usize)
        -> Result<String, String>;
    /// Kills one owned tmux session by stable uid.
    fn kill_owned_tmux_session(&self, session_uid: &str) -> Result<(), String>;
    /// Kills all owned tmux sessions belonging to one agent uid.
    fn kill_owned_tmux_sessions_for_agent(&self, owner_agent_uid: &str) -> Result<(), String>;
    /// Attaches to one daemon session and returns its current snapshot plus a live update stream.
    fn attach_session(&self, session_id: &str) -> Result<AttachedDaemonSession, String>;
    /// Sends one terminal command into the named daemon session.
    fn send_command(&self, session_id: &str, command: TerminalCommand) -> Result<(), String>;
    #[allow(
        dead_code,
        reason = "protocol includes resize even before the UI drives it"
    )]
    /// Requests a PTY resize for the named daemon session.
    fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String>;
    /// Terminates the named daemon session and removes it from the daemon registry.
    fn kill_session(&self, session_id: &str) -> Result<(), String>;
}

#[derive(Resource, Clone)]
pub(crate) struct TerminalDaemonClientResource {
    inner: Arc<dyn TerminalDaemonClient>,
}

impl TerminalDaemonClientResource {
    /// Builds the Bevy resource wrapper around the default socket-backed daemon client.
    ///
    /// Startup uses this to connect to an existing daemon or auto-start one if needed.
    pub(crate) fn system() -> Result<Self, String> {
        Ok(Self {
            inner: Arc::new(SocketTerminalDaemonClient::connect_or_start_default()?),
        })
    }

    /// Builds this value from client.
    #[cfg(test)]
    pub(crate) fn from_client<T>(client: Arc<T>) -> Self
    where
        T: TerminalDaemonClient + 'static,
    {
        Self { inner: client }
    }

    /// Returns the erased daemon-client trait object stored inside the resource.
    pub(crate) fn client(&self) -> &dyn TerminalDaemonClient {
        self.inner.as_ref()
    }
}

pub(crate) struct SocketTerminalDaemonClient {
    core: SocketRequestClientCore<ClientMessage, DaemonResponse>,
    session_routes: Arc<Mutex<HashMap<String, mpsc::Sender<TerminalUpdate>>>>,
}

impl SocketTerminalDaemonClient {
    /// Connects to the default daemon socket, spawning a background daemon process if the first
    /// connect attempt fails.
    ///
    /// This is the "just make the daemon exist" entry point used by normal app startup.
    fn connect_or_start_default() -> Result<Self, String> {
        let socket_path = resolve_daemon_socket_path()
            .ok_or_else(|| "failed to resolve daemon socket path".to_owned())?;
        match Self::connect(&socket_path) {
            Ok(client) => Ok(client),
            Err(connect_error) => {
                append_debug_log(format!(
                    "daemon connect failed {}: {connect_error}; attempting start",
                    socket_path.display()
                ));
                spawn_daemon_subprocess(&socket_path)?;
                wait_for_connect(&socket_path, DAEMON_CONNECT_RETRY_TIMEOUT, Self::connect)
            }
        }
    }

    /// Connects to an already-running daemon socket and starts the reader/writer background threads.
    ///
    /// Requests are sent through a writer channel, responses are matched back to waiting callers by
    /// request id, and session update events are fanned out by session id.
    pub(crate) fn connect(socket_path: &Path) -> Result<Self, String> {
        let session_routes = Arc::new(Mutex::new(
            HashMap::<String, mpsc::Sender<TerminalUpdate>>::new(),
        ));
        let routes_reader = session_routes.clone();
        let core = SocketRequestClientCore::connect(
            socket_path,
            write_client_message,
            read_server_message,
            Arc::new(move |message| match message {
                ServerMessage::Response {
                    request_id,
                    response,
                } => Some((request_id, response)),
                ServerMessage::Event(event) => {
                    dispatch_event(&routes_reader, event);
                    None
                }
            }),
        )?;
        Ok(Self {
            core,
            session_routes,
        })
    }

    /// Sends one request to the daemon and waits synchronously for the matching response.
    fn request(&self, request: DaemonRequest) -> Result<DaemonResponse, String> {
        self.core.request_with(
            DAEMON_REQUEST_TIMEOUT,
            Arc::new(move |request_id| ClientMessage::Request {
                request_id,
                request: request.clone(),
            }),
        )
    }
}

impl TerminalDaemonClient for SocketTerminalDaemonClient {
    /// Issues a `ListSessions` request and asserts that the daemon answered with a session list.
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        match self.request(DaemonRequest::ListSessionsDetailed)? {
            DaemonResponse::SessionListDetailed { sessions } => Ok(sessions),
            response => Err(format!("unexpected daemon list response: {response:?}")),
        }
    }

    fn update_session_metadata_label(
        &self,
        session_id: &str,
        agent_label: Option<&str>,
    ) -> Result<(), String> {
        match self.request(DaemonRequest::UpdateSessionMetadata {
            session_id: session_id.to_owned(),
            agent_label: agent_label.map(str::to_owned),
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!(
                "unexpected daemon update-session-metadata response: {response:?}"
            )),
        }
    }

    /// Issues a `CreateSession` request and extracts the returned session id.
    fn create_session_with_env(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        env_overrides: &[(String, String)],
    ) -> Result<String, String> {
        match self.request(DaemonRequest::CreateSession {
            prefix: prefix.to_owned(),
            cwd: cwd.map(str::to_owned),
            env_overrides: env_overrides.to_vec(),
        })? {
            DaemonResponse::SessionCreated { session_id } => Ok(session_id),
            response => Err(format!("unexpected daemon create response: {response:?}")),
        }
    }

    fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String> {
        match self.request(DaemonRequest::ListOwnedTmuxSessions)? {
            DaemonResponse::OwnedTmuxSessionList { sessions } => Ok(sessions),
            response => Err(format!("unexpected owned tmux list response: {response:?}")),
        }
    }

    fn create_owned_tmux_session(
        &self,
        owner_agent_uid: &str,
        display_name: &str,
        cwd: Option<&str>,
        command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String> {
        match self.request(DaemonRequest::CreateOwnedTmuxSession {
            owner_agent_uid: owner_agent_uid.to_owned(),
            display_name: display_name.to_owned(),
            cwd: cwd.map(str::to_owned),
            command: command.to_owned(),
        })? {
            DaemonResponse::OwnedTmuxSessionCreated { session } => Ok(session),
            response => Err(format!(
                "unexpected owned tmux create response: {response:?}"
            )),
        }
    }

    fn capture_owned_tmux_session(
        &self,
        session_uid: &str,
        lines: usize,
    ) -> Result<String, String> {
        match self.request(DaemonRequest::CaptureOwnedTmuxSession {
            session_uid: session_uid.to_owned(),
            lines,
        })? {
            DaemonResponse::OwnedTmuxSessionCapture {
                session_uid: _,
                text,
            } => Ok(text),
            response => Err(format!(
                "unexpected owned tmux capture response: {response:?}"
            )),
        }
    }

    fn kill_owned_tmux_session(&self, session_uid: &str) -> Result<(), String> {
        match self.request(DaemonRequest::KillOwnedTmuxSession {
            session_uid: session_uid.to_owned(),
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!("unexpected owned tmux kill response: {response:?}")),
        }
    }

    fn kill_owned_tmux_sessions_for_agent(&self, owner_agent_uid: &str) -> Result<(), String> {
        match self.request(DaemonRequest::KillOwnedTmuxSessionsForAgent {
            owner_agent_uid: owner_agent_uid.to_owned(),
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!(
                "unexpected owned tmux owner-kill response: {response:?}"
            )),
        }
    }

    /// Attaches to a daemon session, installs a local update route for it, and returns its current
    /// snapshot plus live update receiver.
    ///
    /// The client rejects duplicate attaches for the same session within one UI process because the
    /// routing table only supports one live receiver per session id.
    fn attach_session(&self, session_id: &str) -> Result<AttachedDaemonSession, String> {
        let (updates_tx, updates_rx) = mpsc::channel();
        {
            let mut routes = lock(&self.session_routes);
            // A single UI process owns at most one live route per session id. Re-attachment within
            // the same process must happen after the previous route is torn down.
            if routes.contains_key(session_id) {
                return Err(format!(
                    "daemon session `{session_id}` is already attached in this UI process"
                ));
            }
            routes.insert(session_id.to_owned(), updates_tx);
        }

        match self.request(DaemonRequest::AttachSession {
            session_id: session_id.to_owned(),
        }) {
            Ok(DaemonResponse::SessionAttached {
                session_id: attached_session_id,
                snapshot,
                ..
            }) => {
                if attached_session_id != session_id {
                    lock(&self.session_routes).remove(session_id);
                    return Err(format!(
                        "daemon attach returned mismatched session id `{attached_session_id}`"
                    ));
                }
                Ok(AttachedDaemonSession {
                    snapshot,
                    updates: updates_rx,
                })
            }
            Ok(response) => {
                lock(&self.session_routes).remove(session_id);
                Err(format!("unexpected daemon attach response: {response:?}"))
            }
            Err(error) => {
                lock(&self.session_routes).remove(session_id);
                Err(error)
            }
        }
    }

    /// Forwards one terminal command to the daemon and expects a plain acknowledgement.
    fn send_command(&self, session_id: &str, command: TerminalCommand) -> Result<(), String> {
        match self.request(DaemonRequest::SendCommand {
            session_id: session_id.to_owned(),
            command,
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!(
                "unexpected daemon send-command response: {response:?}"
            )),
        }
    }

    /// Forwards a resize request to the daemon and expects a plain acknowledgement.
    fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String> {
        match self.request(DaemonRequest::ResizeSession {
            session_id: session_id.to_owned(),
            cols,
            rows,
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!("unexpected daemon resize response: {response:?}")),
        }
    }

    /// Requests daemon-side session termination and drops any local update route for that session.
    fn kill_session(&self, session_id: &str) -> Result<(), String> {
        match self.request(DaemonRequest::KillSession {
            session_id: session_id.to_owned(),
        })? {
            DaemonResponse::Ack => {
                lock(&self.session_routes).remove(session_id);
                Ok(())
            }
            response => Err(format!("unexpected daemon kill response: {response:?}")),
        }
    }
}

/// Routes one daemon event to the locally attached receiver for its session, if any.
///
/// If the receiver has gone away, the stale route is removed from the routing table.
fn dispatch_event(
    routes: &Arc<Mutex<HashMap<String, mpsc::Sender<TerminalUpdate>>>>,
    event: DaemonEvent,
) {
    match event {
        DaemonEvent::SessionUpdated {
            session_id, update, ..
        } => {
            let route = lock(routes).get(&session_id).cloned();
            if let Some(route) = route {
                if route.send(update).is_err() {
                    lock(routes).remove(&session_id);
                }
            }
        }
    }
}

/// Locks this value.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
