mod ansi_surface;
mod backend;
pub(crate) mod box_drawing;
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
    resolve_daemon_socket_path, run_daemon_server, AttachedDaemonSession, DaemonSessionInfo,
    TerminalDaemonClientResource, PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
};
pub(crate) use debug::{
    append_debug_log, note_key_event, note_terminal_error, with_debug_stats, TerminalDebugStats,
};
pub(crate) use fonts::{
    configure_terminal_fonts, is_emoji_like, is_private_use_like, TerminalCellMetrics,
    TerminalFontState, TerminalTextRenderer,
};
pub(crate) use lifecycle::{attach_terminal_session, kill_active_terminal_session_and_remove};
pub(crate) use mailbox::TerminalUpdateMailbox;
pub(crate) use notes::{
    clear_done_tasks, extract_next_task, load_terminal_notes_from, mark_terminal_notes_dirty,
    resolve_terminal_notes_path, save_terminal_notes_if_dirty, task_entry_from_text,
    TerminalNotesState,
};
#[cfg(test)]
pub(crate) use presentation::hud_terminal_target_position;
pub(crate) use presentation::{
    active_terminal_layout_for_dimensions, sync_active_terminal_dimensions,
    sync_terminal_hud_surface, sync_terminal_panel_frames, sync_terminal_presentations,
    sync_terminal_projection_entities, target_active_terminal_dimensions,
    terminal_texture_screen_size,
};
pub(crate) use presentation_state::{
    PresentedTerminal, TerminalCameraMarker, TerminalDisplayMode, TerminalHudSurfaceMarker,
    TerminalPanel, TerminalPanelFrame, TerminalPanelSprite, TerminalPointerState,
    TerminalPresentation, TerminalPresentationStore, TerminalTextureState, TerminalViewState,
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
    DrainedTerminalUpdates, LatestTerminalStatus, PtySession, TerminalCell, TerminalCellContent,
    TerminalCommand, TerminalCursor, TerminalCursorShape, TerminalDamage, TerminalDimensions,
    TerminalFontFace, TerminalFontReport, TerminalFrameUpdate, TerminalRuntimeState,
    TerminalSnapshot, TerminalSurface, TerminalUpdate, PTY_OUTPUT_BATCH_BYTES,
    PTY_OUTPUT_BATCH_WINDOW, PTY_OUTPUT_WAIT_TIMEOUT,
};

#[cfg(test)]
pub(crate) use ansi_surface::build_surface;
#[cfg(test)]
pub(crate) use backend::{resolve_alacritty_color, send_command_payload_bytes, xterm_indexed_rgb};
#[cfg(test)]
pub(crate) use daemon::{
    read_client_message, read_server_message, resolve_daemon_socket_path_with,
    write_client_message, write_server_message, ClientMessage, DaemonEvent, DaemonRequest,
    DaemonServerHandle, ServerMessage, SocketTerminalDaemonClient, TerminalDaemonClient,
};
#[cfg(test)]
pub(crate) use fonts::TerminalFontRasterConfig;
#[cfg(test)]
pub(crate) use fonts::{
    find_kitty_config_path_with, initialize_terminal_text_renderer_with_locale,
    measure_monospace_cell, parse_kitty_config_file, resolve_terminal_font_report_for_family,
    resolve_terminal_font_report_for_path, KittyFontConfig,
};
#[cfg(test)]
pub(crate) use presentation::{
    active_terminal_cell_size, active_terminal_dimensions, active_terminal_layout,
    active_terminal_viewport, pixel_perfect_cell_size, pixel_perfect_terminal_logical_size,
    snap_to_pixel_grid,
};
#[cfg(test)]
pub(crate) use raster::{
    blend_rgba_in_place, create_terminal_image, rasterize_terminal_glyph, CachedTerminalGlyph,
    TerminalFontRole, TerminalGlyphCacheKey,
};
#[cfg(test)]
pub(crate) use session_persistence::{
    serialize_persisted_terminal_sessions, PersistedTerminalSessions, TerminalSessionRecord,
};
