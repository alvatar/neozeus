#[path = "terminals/backend.rs"]
mod backend;
#[path = "terminals/bridge.rs"]
mod bridge;
#[path = "terminals/debug.rs"]
mod debug;
#[path = "terminals/fonts.rs"]
mod fonts;
#[path = "terminals/mailbox.rs"]
mod mailbox;
#[path = "terminals/presentation.rs"]
mod presentation;
#[path = "terminals/presentation_state.rs"]
mod presentation_state;
#[path = "terminals/raster.rs"]
mod raster;
#[path = "terminals/registry.rs"]
mod registry;
#[path = "terminals/runtime.rs"]
mod runtime;
#[path = "terminals/types.rs"]
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
    pixel_perfect_cell_size, spawn_terminal_instance, sync_terminal_hud_surface,
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
pub(crate) use types::{
    DrainedTerminalUpdates, LatestTerminalStatus, PtySession, TerminalCell, TerminalCellContent,
    TerminalCommand, TerminalCursor, TerminalCursorShape, TerminalDamage, TerminalDimensions,
    TerminalFontFace, TerminalFontReport, TerminalFrameUpdate, TerminalRuntimeState,
    TerminalSnapshot, TerminalSurface, TerminalUpdate, PTY_OUTPUT_BATCH_BYTES,
    PTY_OUTPUT_BATCH_WINDOW, PTY_OUTPUT_WAIT_TIMEOUT,
};
