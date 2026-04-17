mod active_content;
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
mod owned_tmux_state;
mod presentation;
mod presentation_state;
mod pty_spawn;
mod raster;
mod readiness;
mod registry;
mod runtime;
mod session_metrics;
mod session_persistence;
mod types;

pub(crate) use active_content::{
    sync_active_terminal_content, ActiveTerminalContentState, ActiveTerminalContentSyncState,
};
pub(crate) use ansi_surface::surface_from_ansi_text_auto_size;
pub(crate) use bridge::TerminalBridge;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use daemon::TerminalDaemonClient;
pub(crate) use daemon::{
    resolve_daemon_socket_path, run_daemon_server, DaemonSessionInfo, OwnedTmuxSessionInfo,
    TerminalDaemonClientResource, PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
};
pub(crate) use debug::append_debug_log;
pub(crate) use fonts::{configure_terminal_fonts, TerminalFontState, TerminalTextRenderer};
#[cfg(test)]
pub(crate) use lifecycle::kill_active_terminal_session_and_remove;
pub(crate) use lifecycle::{attach_terminal_session, kill_terminal_session_and_remove};
pub(crate) use notes::{
    clear_done_tasks, extract_next_task, load_terminal_notes_from, mark_terminal_notes_dirty,
    resolve_terminal_notes_path, save_terminal_notes_if_dirty, task_entry_from_text,
    TerminalNotesState,
};
pub(crate) use owned_tmux_state::{
    refresh_owned_tmux_sessions_now, sync_owned_tmux_sessions, OwnedTmuxSessionStore,
};
pub(crate) use presentation::{
    sync_active_terminal_dimensions, sync_terminal_hud_surface, sync_terminal_panel_frames,
    sync_terminal_presentations, sync_terminal_projection_entities, terminal_texture_screen_size,
};
pub(crate) use presentation_state::{
    TerminalCameraMarker, TerminalDisplayMode, TerminalHudSurfaceMarker, TerminalPanel,
    TerminalPointerState, TerminalPresentation, TerminalPresentationStore, TerminalViewState,
};
#[cfg(test)]
pub(crate) use presentation_state::PresentedTerminal;
pub(crate) use raster::{sync_terminal_texture, TerminalGlyphCache};
pub(crate) use readiness::{terminal_readiness_for_id, TerminalReadiness};
pub(crate) use registry::{
    poll_terminal_snapshots, TerminalFocusState, TerminalId, TerminalManager,
};
pub(crate) use runtime::{RuntimeNotifier, TerminalRuntimeSpawner};
pub(crate) use session_metrics::{sync_live_session_metrics, LiveSessionMetricsStore};
pub(crate) use session_persistence::{
    load_persisted_terminal_sessions_from, resolve_terminal_sessions_path,
    PersistedTerminalSessions,
};
#[cfg(test)]
pub(crate) use session_persistence::{
    serialize_persisted_terminal_sessions, TerminalSessionRecord,
};
pub(crate) use types::TerminalLifecycle;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use types::TerminalSnapshot;
pub(crate) use types::{
    TerminalCell, TerminalCellContent, TerminalCommand, TerminalRuntimeState, TerminalSurface,
    TerminalViewportPoint,
};

#[cfg(test)]
pub(crate) use ansi_surface::build_surface;
#[cfg(test)]
pub(crate) use daemon::AttachedDaemonSession;
#[cfg(test)]
pub(crate) use debug::TerminalDebugStats;
#[cfg(test)]
pub(crate) use mailbox::TerminalUpdateMailbox;
#[cfg(test)]
pub(crate) use presentation::{
    active_terminal_cell_size, active_terminal_dimensions, active_terminal_layout,
};
#[cfg(test)]
pub(crate) use presentation_state::{TerminalPanelFrame, TerminalTextureState};
#[cfg(test)]
pub(crate) use types::{TerminalDamage, TerminalFrameUpdate, TerminalUpdate};
