mod ansi_surface;
mod backend;
mod box_drawing;
mod bridge;
mod daemon;
mod damage;
mod debug;
mod fonts;
mod lifecycle;
mod mailbox;
mod notes;
mod presentation;
mod presentation_state;
mod pty_spawn;
mod raster;
mod registry;
mod runtime;
mod session_persistence;
mod types;

pub(crate) use bridge::TerminalBridge;
pub(crate) use daemon::{
    resolve_daemon_socket_path, run_daemon_server, DaemonSessionInfo, TerminalDaemonClientResource,
    PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
};
pub(crate) use debug::append_debug_log;
pub(crate) use fonts::{configure_terminal_fonts, TerminalFontState, TerminalTextRenderer};
pub(crate) use lifecycle::{attach_terminal_session, kill_active_terminal_session_and_remove};
pub(crate) use notes::{
    clear_done_tasks, extract_next_task, load_terminal_notes_from, mark_terminal_notes_dirty,
    resolve_terminal_notes_path, save_terminal_notes_if_dirty, task_entry_from_text,
    TerminalNotesState,
};
pub(crate) use presentation::{
    sync_active_terminal_dimensions, sync_terminal_hud_surface, sync_terminal_panel_frames,
    sync_terminal_presentations, sync_terminal_projection_entities, terminal_texture_screen_size,
};
pub(crate) use presentation_state::{
    TerminalCameraMarker, TerminalDisplayMode, TerminalHudSurfaceMarker, TerminalPanel,
    TerminalPointerState, TerminalPresentation, TerminalPresentationStore, TerminalViewState,
};
pub(crate) use raster::{sync_terminal_texture, TerminalGlyphCache};
pub(crate) use registry::{
    poll_terminal_snapshots, TerminalFocusState, TerminalId, TerminalManager,
};
pub(crate) use runtime::{RuntimeNotifier, TerminalRuntimeSpawner};
pub(crate) use session_persistence::{
    load_persisted_terminal_sessions_from, mark_terminal_sessions_dirty,
    reconcile_terminal_sessions, resolve_terminal_sessions_path, save_terminal_sessions_if_dirty,
    TerminalSessionPersistenceState,
};
pub(crate) use types::TerminalLifecycle;
pub(crate) use types::{
    TerminalCell, TerminalCellContent, TerminalCommand, TerminalRuntimeState, TerminalSurface,
};

#[cfg(test)]
pub(crate) use tests::*;

#[cfg(test)]
mod tests;
