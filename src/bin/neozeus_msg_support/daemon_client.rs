use neozeus::shared::{
    daemon_client_core::{spawn_daemon_subprocess, wait_for_connect, SocketRequestClientCore},
    daemon_socket::resolve_daemon_socket_path,
    daemon_wire::{
        read_server_message, write_client_message, ClientMessage, DaemonRequest, DaemonResponse,
        ServerMessage,
    },
};
use std::{path::Path, sync::Arc, time::Duration};

pub(crate) use neozeus::shared::daemon_wire::{DaemonSessionInfo, TerminalCommand};
#[cfg(test)]
pub(crate) use neozeus::shared::daemon_wire::{TerminalLifecycle, TerminalRuntimeState};

const DAEMON_CONNECT_RETRY_TIMEOUT: Duration = Duration::from_secs(2);
const DAEMON_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) trait DaemonMessenger {
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String>;
    fn send_command(&self, session_name: &str, command: TerminalCommand) -> Result<(), String>;
}

pub(crate) struct SocketDaemonMessenger {
    core: SocketRequestClientCore<ClientMessage, DaemonResponse>,
}

impl SocketDaemonMessenger {
    pub(crate) fn connect_or_start_default() -> Result<Self, String> {
        let socket_path = resolve_daemon_socket_path()
            .ok_or_else(|| "failed to resolve daemon socket path".to_owned())?;
        match Self::connect(&socket_path) {
            Ok(client) => Ok(client),
            Err(_) => {
                spawn_daemon_subprocess(&socket_path)?;
                wait_for_connect(&socket_path, DAEMON_CONNECT_RETRY_TIMEOUT, Self::connect)
            }
        }
    }

    fn connect(socket_path: &Path) -> Result<Self, String> {
        let core = SocketRequestClientCore::connect(
            socket_path,
            write_client_message,
            read_server_message,
            Arc::new(|message| match message {
                ServerMessage::Response {
                    request_id,
                    response,
                } => Some((request_id, response)),
            }),
        )?;
        Ok(Self { core })
    }

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

impl DaemonMessenger for SocketDaemonMessenger {
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        match self.request(DaemonRequest::ListSessionsDetailed)? {
            DaemonResponse::SessionListDetailed { sessions } => Ok(sessions),
            response => Err(format!("unexpected daemon list response: {response:?}")),
        }
    }

    fn send_command(&self, session_name: &str, command: TerminalCommand) -> Result<(), String> {
        match self.request(DaemonRequest::SendCommand {
            session_id: session_name.to_owned(),
            command,
        })? {
            DaemonResponse::Ack => Ok(()),
            response => Err(format!("unexpected daemon send response: {response:?}")),
        }
    }
}
