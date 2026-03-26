mod client;
mod protocol;
mod server;
mod session;

pub(crate) use client::{
    resolve_daemon_socket_path, AttachedDaemonSession, TerminalDaemonClientResource,
};
#[cfg(test)]
pub(crate) use client::{
    resolve_daemon_socket_path_with, SocketTerminalDaemonClient, TerminalDaemonClient,
};
pub(crate) use protocol::DaemonSessionInfo;
#[cfg(test)]
pub(crate) use protocol::{
    read_client_message, read_server_message, write_client_message, write_server_message,
    ClientMessage, DaemonEvent, DaemonRequest, ServerMessage,
};
pub(crate) use server::run_daemon_server;
#[cfg(test)]
pub(crate) use server::DaemonServerHandle;
pub(crate) use session::{
    is_persistent_session_name, PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
};
