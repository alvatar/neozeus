use neozeus::shared::{
    daemon_client_core::SocketRequestClientCore,
    daemon_wire::{
        read_server_message, write_client_message, ClientMessage, DaemonRequest, DaemonResponse,
        OwnedTmuxSessionInfo, ServerMessage,
    },
};
use std::{path::Path, sync::Arc, time::Duration};

const DAEMON_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) trait OwnedTmuxCreator {
    fn create_owned_tmux_session(
        &self,
        owner_agent_uid: &str,
        display_name: &str,
        cwd: Option<&str>,
        command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String>;
}

pub(crate) struct SocketOwnedTmuxClient {
    core: SocketRequestClientCore<ClientMessage, DaemonResponse>,
}

impl SocketOwnedTmuxClient {
    pub(crate) fn connect(socket_path: &Path) -> Result<Self, String> {
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

impl OwnedTmuxCreator for SocketOwnedTmuxClient {
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
}
