mod ansi_surface;
mod backend;
mod bridge;
mod damage;
mod debug;
mod fonts;
mod lifecycle;
mod mailbox;
mod presentation;
mod presentation_state;
mod pty_backend;
mod pty_spawn;
mod raster;
mod registry;
mod runtime;
mod session_persistence;
mod tmux;
mod tmux_viewer_backend;
mod types;

pub(crate) use backend::{build_surface, send_command_payload_bytes};
#[cfg(test)]
pub(crate) use backend::{compute_terminal_damage, resolve_alacritty_color, xterm_indexed_rgb};
pub(crate) use bridge::TerminalBridge;
pub(crate) use debug::{
    append_debug_log, note_key_event, note_terminal_error, with_debug_stats, TerminalDebugStats,
};
pub(crate) use fonts::{
    configure_terminal_fonts, is_emoji_like, is_private_use_like, TerminalFontState,
    TerminalTextRenderer,
};
#[cfg(test)]
pub(crate) use fonts::{
    find_kitty_config_path_with, initialize_terminal_text_renderer, parse_kitty_config_file,
    resolve_terminal_font_report, KittyFontConfig,
};
pub(crate) use lifecycle::{
    kill_active_terminal_session_and_remove, spawn_attached_terminal_with_presentation,
};
pub(crate) use mailbox::TerminalUpdateMailbox;
pub(crate) use presentation::{
    pixel_perfect_cell_size, spawn_terminal_presentation, sync_terminal_hud_surface,
    sync_terminal_panel_frames, sync_terminal_presentations, terminal_texture_screen_size,
};
#[cfg(test)]
pub(crate) use presentation::{pixel_perfect_terminal_logical_size, snap_to_pixel_grid};
pub(crate) use presentation_state::{
    PresentedTerminal, TerminalCameraMarker, TerminalDisplayMode, TerminalHudSurfaceMarker,
    TerminalPanel, TerminalPanelFrame, TerminalPanelSprite, TerminalPointerState,
    TerminalPresentation, TerminalPresentationStore, TerminalTextureState, TerminalViewState,
};
#[cfg(test)]
pub(crate) use raster::{
    blend_rgba_in_place, rasterize_terminal_glyph, CachedTerminalGlyph, TerminalFontRole,
    TerminalGlyphCacheKey,
};
pub(crate) use raster::{create_terminal_image, sync_terminal_texture, TerminalGlyphCache};
pub(crate) use registry::{poll_terminal_snapshots, TerminalId, TerminalManager};
pub(crate) use runtime::{RuntimeNotifier, TerminalRuntimeSpawner};
pub(crate) use session_persistence::{
    load_persisted_terminal_sessions_from, mark_terminal_sessions_dirty,
    reconcile_terminal_sessions, resolve_terminal_sessions_path, save_terminal_sessions_if_dirty,
    TerminalSessionPersistenceState,
};
#[cfg(test)]
pub(crate) use session_persistence::{
    parse_persisted_terminal_sessions, resolve_terminal_sessions_path_with,
    serialize_persisted_terminal_sessions, PersistedTerminalSessions, TerminalSessionRecord,
};
#[cfg(test)]
pub(crate) use tmux::create_detached_session_tmux_commands;
pub(crate) use tmux::{
    build_attach_command_argv, generate_unique_session_name, is_persistent_session_name,
    provision_terminal_target, resolve_tmux_active_pane_target, TerminalSessionClient,
    TmuxClientResource, TmuxPaneClient, PERSISTENT_TMUX_SESSION_PREFIX,
    VERIFIER_TMUX_SESSION_PREFIX,
};
#[cfg(test)]
pub(crate) use tmux::{send_bytes_tmux_commands, TmuxPaneDescriptor, TmuxPaneState};
#[cfg(test)]
pub(crate) use types::TerminalLifecycle;
pub(crate) use types::{
    DrainedTerminalUpdates, LatestTerminalStatus, PtySession, TerminalAttachTarget, TerminalCell,
    TerminalCellContent, TerminalCommand, TerminalCursor, TerminalCursorShape, TerminalDamage,
    TerminalDimensions, TerminalFontFace, TerminalFontReport, TerminalFrameUpdate,
    TerminalProvisionTarget, TerminalRuntimeState, TerminalSnapshot, TerminalSurface,
    TerminalUpdate, PTY_OUTPUT_BATCH_BYTES, PTY_OUTPUT_BATCH_WINDOW, PTY_OUTPUT_WAIT_TIMEOUT,
};
