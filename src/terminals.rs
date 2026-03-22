mod backend;
mod bridge;
mod debug;
mod fonts;
mod mailbox;
mod presentation;
mod presentation_state;
mod raster;
mod registry;
mod runtime;
mod session_persistence;
mod tmux;
mod types;

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
    find_kitty_config_path, initialize_terminal_text_renderer, parse_kitty_config_file,
    resolve_terminal_font_report, KittyFontConfig,
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
    build_attach_command_argv, capture_pane_tmux_command, generate_unique_session_name,
    is_persistent_session_name, list_panes_tmux_command, pane_state_tmux_command,
    provision_terminal_target, send_bytes_tmux_commands, TmuxClient, TmuxClientResource,
    PERSISTENT_TMUX_SESSION_PREFIX, VERIFIER_TMUX_SESSION_PREFIX,
};
#[cfg(test)]
pub(crate) use types::TerminalLifecycle;
pub(crate) use types::{
    DrainedTerminalUpdates, LatestTerminalStatus, PtySession, TerminalAttachTarget, TerminalCell,
    TerminalCellContent, TerminalCommand, TerminalCursor, TerminalCursorShape, TerminalDamage,
    TerminalDimensions, TerminalFontFace, TerminalFontReport, TerminalFrameUpdate,
    TerminalProvisionTarget, TerminalRuntimeState, TerminalSnapshot, TerminalSurface,
    TerminalUpdate, PTY_OUTPUT_BATCH_BYTES, PTY_OUTPUT_BATCH_WINDOW, PTY_OUTPUT_WAIT_TIMEOUT,
};

pub(crate) fn spawn_attached_terminal_with_presentation(
    commands: &mut bevy::prelude::Commands,
    images: &mut bevy::prelude::Assets<bevy::prelude::Image>,
    terminal_manager: &mut TerminalManager,
    presentation_store: &mut TerminalPresentationStore,
    runtime_spawner: &TerminalRuntimeSpawner,
    session_name: String,
    focus: bool,
) -> (TerminalId, TerminalBridge) {
    let bridge = runtime_spawner.spawn_attached(TerminalAttachTarget::TmuxViewer {
        session_name: session_name.clone(),
    });
    let (terminal_id, slot) = if focus {
        terminal_manager.create_terminal_with_slot_and_session(bridge.clone(), session_name)
    } else {
        terminal_manager
            .create_terminal_without_focus_with_slot_and_session(bridge.clone(), session_name)
    };
    spawn_terminal_presentation(commands, images, presentation_store, terminal_id, slot);
    (terminal_id, bridge)
}
