mod client;
mod owned_tmux;
mod protocol;
mod server;
mod session;
mod session_metrics;

#[cfg(test)]
mod tests;

pub(crate) use crate::shared::daemon_socket::resolve_daemon_socket_path;
#[cfg(test)]
pub(crate) use crate::shared::daemon_socket::resolve_daemon_socket_path_with;
pub(crate) use crate::shared::daemon_wire::DaemonSessionInfo;
pub(crate) use client::{AttachedDaemonSession, TerminalDaemonClientResource};
#[cfg(test)]
pub(crate) use client::{SocketTerminalDaemonClient, TerminalDaemonClient};
pub(crate) use owned_tmux::OwnedTmuxSessionInfo;
#[cfg(test)]
pub(crate) use protocol::{
    read_client_message, read_server_message, write_client_message, write_server_message,
    ClientMessage, DaemonEvent, DaemonRequest, ServerMessage,
};
pub(crate) use server::run_daemon_server;
#[cfg(test)]
pub(crate) use server::DaemonServerHandle;
#[cfg(test)]
pub(crate) use session::is_persistent_session_name;
pub(crate) use session::{PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX};
