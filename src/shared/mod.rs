//! Shared surface consumed by the `src/bin/neozeus-*` support binaries plus the main binary.
//!
//! Items reachable through a `pub` module here are part of the cross-binary contract — the daemon
//! wire protocol, the on-disk app-state file format, the worktree operations the support bins
//! drive, and the helper layers those bins compose on top (daemon socket resolution, client
//! transport, shell quoting, command framing). Modules needed only inside the library crate stay
//! `pub(crate)`.

pub mod app_state_file;
pub mod daemon_client_core;
pub mod daemon_socket;
pub mod daemon_wire;
pub mod send_command;
pub mod shell;
pub mod worktree;

pub(crate) mod agent_durability;
pub(crate) mod capture;
pub(crate) mod codex_state;
pub(crate) mod command_runner;
pub(crate) mod linux_display;
pub(crate) mod persistence;
pub(crate) mod pi_session_files;
pub(crate) mod readback;
pub(crate) mod text_cursor;
pub(crate) mod text_escape;
pub(crate) mod visual_contracts;
