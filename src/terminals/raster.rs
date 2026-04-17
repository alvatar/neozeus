//! Terminal surface rasterization into GPU-backed textures.
//!
//! Owns the full raster pipeline for a terminal panel: glyph rasterization through the shared
//! font cache, per-cell background/foreground composition, cursor overlay, damage-aware partial
//! repaints, and the upload path into the Bevy `Image` assets that the presentation layer binds.
//! The passes share tight cell-geometry invariants (same origin, same subpixel offset, same cell
//! advance), so extracting them into separate files would force every call site to thread the
//! same per-cell state through a helper boundary for no clarity gain.

use crate::{
    app_config::{resolve_debug_texture_dump_path, DEFAULT_BG},
    hud::HudLayoutState,
    text_selection::TerminalTextSelectionState,
    verification::VerificationTerminalSurfaceOverrides,
};

use super::{
    active_content::ActiveTerminalContentState,
    box_drawing::{is_box_drawing, rasterize_box_drawing},
    debug::append_debug_log,
    fonts::{is_emoji_like, is_private_use_like, TerminalFontState, TerminalTextRenderer},
    presentation::{active_terminal_layout_for_dimensions, target_active_terminal_dimensions},
    presentation_state::{
        PresentedTerminal, TerminalPresentationStore, TerminalTextureState, TerminalViewState,
    },
    registry::{TerminalFocusState, TerminalManager},
    types::{
        TerminalCell, TerminalCellContent, TerminalCursor, TerminalCursorShape, TerminalDamage,
        TerminalDimensions, TerminalSurface, TerminalUnderlineStyle,
    },
};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::{
        Assets, DetectChanges, Handle, Image, Res, ResMut, Resource, Single, UVec2, Window, With,
    },
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};
use bevy_egui::egui;
use cosmic_text::{
    Attrs as CtAttrs, Buffer as CtBuffer, Color as CtColor, Family as CtFamily,
    Shaping as CtShaping, Style as CtStyle, Weight as CtWeight,
};
use std::{env, fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TerminalFontRole {
    Primary,
    PrivateUse,
    Emoji,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalGlyphCacheKey {
    pub(crate) content: super::types::TerminalCellContent,
    pub(crate) font_role: TerminalFontRole,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) width_cells: u8,
    pub(crate) cell_width: u32,
    pub(crate) cell_height: u32,
}

#[derive(Clone)]
pub(crate) struct CachedTerminalGlyph {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
    pub(crate) preserve_color: bool,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalGlyphCache {
    pub(crate) glyphs: std::collections::HashMap<TerminalGlyphCacheKey, CachedTerminalGlyph>,
}

/// Allocates the RGBA image used as a terminal texture backing store.
///
/// New images start filled with the default background color and use nearest sampling so glyphs stay
/// crisp.
pub(crate) fn create_terminal_image(size: UVec2) -> Image {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[
            DEFAULT_BG.r(),
            DEFAULT_BG.g(),
            DEFAULT_BG.b(),
            DEFAULT_BG.a(),
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// Writes a terminal texture image out as a simple binary PPM for debugging.
///
/// Alpha is discarded because PPM is RGB-only.
fn dump_terminal_image_ppm(image: &Image, path: &Path) -> Result<(), String> {
    let width = image.texture_descriptor.size.width;
    let height = image.texture_descriptor.size.height;
    let data = image
        .data
        .as_ref()
        .ok_or_else(|| "image data missing".to_owned())?;
    let mut output = Vec::with_capacity((width as usize * height as usize * 3) + 64);
    output.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
    for pixel in data.chunks_exact(4) {
        output.extend_from_slice(&pixel[..3]);
    }
    fs::write(path, output).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

/// Derives the default texture-state contract for a surface using the font-measured cell dimensions.
fn default_texture_state_for_surface(
    surface: &TerminalSurface,
    font_state: &TerminalFontState,
) -> TerminalTextureState {
    let cw = font_state.cell_metrics.cell_width;
    let ch = font_state.cell_metrics.cell_height;
    TerminalTextureState {
        texture_size: UVec2::new(surface.cols as u32 * cw, surface.rows as u32 * ch),
        cell_size: UVec2::new(cw, ch),
    }
}

/// Returns whether a terminal surface already matches the focused active-layout row/column contract.
fn can_render_active_layout(surface: &TerminalSurface, dimensions: TerminalDimensions) -> bool {
    surface.cols == dimensions.cols && surface.rows == dimensions.rows
}

/// Chooses between the existing uploaded texture-state contract and a conservative default one for a
/// surface.
///
/// Before the first real upload, placeholder texture state is treated as untrustworthy and replaced by
/// the surface-derived default.
fn cached_or_default_texture_state(
    presented_terminal: &PresentedTerminal,
    surface: &TerminalSurface,
    font_state: &TerminalFontState,
) -> TerminalTextureState {
    if presented_terminal.uploaded_revision == 0
        || presented_terminal.texture_state.texture_size == UVec2::ONE
        || presented_terminal.texture_state.cell_size == UVec2::ZERO
    {
        default_texture_state_for_surface(surface, font_state)
    } else {
        presented_terminal.texture_state.clone()
    }
}

#[derive(Clone, Copy)]
struct ActiveRasterTarget {
    active_id: Option<super::registry::TerminalId>,
    active_layout: Option<super::presentation::ActiveTerminalLayout>,
}

#[derive(Clone, Debug)]
struct TerminalRasterWorkPlan {
    upload_state: TerminalTextureState,
    active_override_revision: Option<u64>,
    terminal_selection_revision: Option<u64>,
    full_redraw: bool,
    dirty_rows: Vec<usize>,
}

fn active_raster_target(
    _terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    primary_window: &Window,
    layout_state: &HudLayoutState,
    view_state: &TerminalViewState,
    font_state: &TerminalFontState,
) -> ActiveRasterTarget {
    #[allow(unused_mut)]
    let mut active_id = focus_state.active_id();
    #[cfg(test)]
    {
        active_id = _terminal_manager
            .clone_focus_state()
            .active_id()
            .or(active_id);
    }
    ActiveRasterTarget {
        active_layout: active_id.map(|_| {
            active_terminal_layout_for_dimensions(
                primary_window,
                layout_state,
                view_state,
                target_active_terminal_dimensions(primary_window, layout_state, font_state),
                font_state,
            )
        }),
        active_id,
    }
}

fn raster_source_surface<'a>(
    terminal_id: super::registry::TerminalId,
    terminal: &'a super::registry::ManagedTerminal,
    active_target: ActiveRasterTarget,
    active_terminal_content: &'a ActiveTerminalContentState,
    verification_overrides: Option<&'a VerificationTerminalSurfaceOverrides>,
) -> Option<&'a TerminalSurface> {
    if Some(terminal_id) == active_target.active_id
        && active_terminal_content
            .presentation_override_revision_for(terminal_id)
            .is_some()
    {
        return active_terminal_content.owned_tmux_surface_for(terminal_id);
    }

    verification_overrides
        .and_then(|overrides| overrides.surface_for(terminal_id))
        .or(terminal.snapshot.surface.as_ref())
}

fn upload_state_for_terminal(
    terminal_id: super::registry::TerminalId,
    surface: &TerminalSurface,
    presented_terminal: &mut PresentedTerminal,
    active_target: ActiveRasterTarget,
    font_state: &TerminalFontState,
) -> TerminalTextureState {
    let upload_state = if Some(terminal_id) == active_target.active_id {
        let active_layout = active_target
            .active_layout
            .expect("active layout missing for active terminal");
        let active_target_state = TerminalTextureState {
            texture_size: active_layout.texture_size,
            cell_size: active_layout.cell_size,
        };
        if can_render_active_layout(surface, active_layout.dimensions) {
            active_target_state
        } else {
            cached_or_default_texture_state(presented_terminal, surface, font_state)
        }
    } else if active_target.active_id.is_some() {
        default_texture_state_for_surface(surface, font_state)
    } else {
        cached_or_default_texture_state(presented_terminal, surface, font_state)
    };
    presented_terminal.desired_texture_state = upload_state.clone();
    upload_state
}

fn active_override_revision_for_terminal(
    terminal_id: super::registry::TerminalId,
    active_terminal_content: &ActiveTerminalContentState,
    verification_overrides: Option<&VerificationTerminalSurfaceOverrides>,
) -> Option<u64> {
    active_terminal_content
        .presentation_override_revision_for(terminal_id)
        .or_else(|| {
            verification_overrides
                .and_then(|overrides| overrides.presentation_override_revision_for(terminal_id))
        })
}

fn terminal_raster_work_plan(
    terminal: &super::registry::ManagedTerminal,
    surface: &TerminalSurface,
    presented_terminal: &PresentedTerminal,
    upload_state: TerminalTextureState,
    active_override_revision: Option<u64>,
    terminal_selection_revision: Option<u64>,
    font_state_changed: bool,
) -> TerminalRasterWorkPlan {
    let has_pending_surface = terminal.surface_revision != presented_terminal.uploaded_revision
        || presented_terminal.uploaded_active_override_revision != active_override_revision
        || presented_terminal.uploaded_text_selection_revision != terminal_selection_revision;
    let mut full_redraw = font_state_changed || presented_terminal.texture_state != upload_state;
    let dirty_rows = if full_redraw {
        (0..surface.rows).collect::<Vec<_>>()
    } else if has_pending_surface {
        match terminal
            .pending_damage
            .as_ref()
            .unwrap_or(&TerminalDamage::Full)
        {
            TerminalDamage::Full => {
                full_redraw = true;
                (0..surface.rows).collect::<Vec<_>>()
            }
            TerminalDamage::Rows(rows) => rows.clone(),
        }
    } else {
        Vec::new()
    };
    TerminalRasterWorkPlan {
        upload_state,
        active_override_revision,
        terminal_selection_revision,
        full_redraw,
        dirty_rows,
    }
}

fn all_surface_rows(surface: &TerminalSurface) -> Vec<usize> {
    (0..surface.rows).collect()
}

fn promote_to_full_redraw(
    surface: &TerminalSurface,
    full_redraw: &mut bool,
    dirty_rows: &mut Vec<usize>,
) {
    *full_redraw = true;
    *dirty_rows = all_surface_rows(surface);
}

fn ensure_terminal_target_image(
    presented_terminal: &mut PresentedTerminal,
    images: &mut Assets<Image>,
    upload_state: &TerminalTextureState,
    surface: &TerminalSurface,
    full_redraw: &mut bool,
    dirty_rows: &mut Vec<usize>,
) -> Option<Handle<Image>> {
    if images.get_mut(&presented_terminal.image).is_none() {
        presented_terminal.image = images.add(create_terminal_image(upload_state.texture_size));
        promote_to_full_redraw(surface, full_redraw, dirty_rows);
    }
    Some(presented_terminal.image.clone())
}

fn ensure_target_image_matches_upload_state(
    target_image: &mut Image,
    upload_state: &TerminalTextureState,
    surface: &TerminalSurface,
    full_redraw: &mut bool,
    dirty_rows: &mut Vec<usize>,
) {
    if target_image.texture_descriptor.size.width != upload_state.texture_size.x
        || target_image.texture_descriptor.size.height != upload_state.texture_size.y
    {
        *target_image = create_terminal_image(upload_state.texture_size);
        promote_to_full_redraw(surface, full_redraw, dirty_rows);
    }
}

fn ensure_terminal_pixel_buffer<'a>(
    target_image: &'a mut Image,
    upload_state: &TerminalTextureState,
    surface: &TerminalSurface,
    full_redraw: &mut bool,
    dirty_rows: &mut Vec<usize>,
) -> &'a mut Vec<u8> {
    let expected_len = (upload_state.texture_size.x * upload_state.texture_size.y * 4) as usize;
    let pixels = target_image.data.get_or_insert_with(|| {
        vec![
            DEFAULT_BG.r(),
            DEFAULT_BG.g(),
            DEFAULT_BG.b(),
            DEFAULT_BG.a(),
        ]
    });
    if pixels.len() != expected_len {
        pixels.resize(expected_len, DEFAULT_BG.a());
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[
                DEFAULT_BG.r(),
                DEFAULT_BG.g(),
                DEFAULT_BG.b(),
                DEFAULT_BG.a(),
            ]);
        }
        promote_to_full_redraw(surface, full_redraw, dirty_rows);
    }
    pixels
}

#[allow(
    clippy::too_many_arguments,
    reason = "incremental translation eligibility depends on surface, uploaded revisions, and pending damage together"
)]
fn maybe_translate_existing_pixels(
    pixels: &mut [u8],
    terminal: &super::registry::ManagedTerminal,
    surface: &TerminalSurface,
    presented_terminal: &PresentedTerminal,
    upload_state: &TerminalTextureState,
    active_override_revision: Option<u64>,
    terminal_selection_revision: Option<u64>,
    full_redraw: bool,
) -> Option<Vec<usize>> {
    if full_redraw
        || terminal.pending_damage != Some(TerminalDamage::Full)
        || presented_terminal.uploaded_active_override_revision != active_override_revision
        || presented_terminal.uploaded_text_selection_revision != terminal_selection_revision
    {
        return None;
    }
    presented_terminal
        .uploaded_surface
        .as_ref()
        .and_then(|previous| {
            translate_terminal_viewport_pixels(
                pixels,
                upload_state.texture_size,
                upload_state.cell_size,
                previous,
                surface,
            )
        })
}

fn finalize_terminal_upload(
    terminal: &mut super::registry::ManagedTerminal,
    presented_terminal: &mut PresentedTerminal,
    uploaded_surface: TerminalSurface,
    plan: &TerminalRasterWorkPlan,
) {
    presented_terminal.texture_state = plan.upload_state.clone();
    presented_terminal.uploaded_revision = terminal.surface_revision;
    presented_terminal.uploaded_active_override_revision = plan.active_override_revision;
    presented_terminal.uploaded_text_selection_revision = plan.terminal_selection_revision;
    presented_terminal.uploaded_surface = Some(uploaded_surface);
    terminal.pending_damage = None;
}

#[allow(
    clippy::too_many_arguments,
    reason = "one-terminal raster sync still coordinates terminal state, presentation state, renderer, images, and selection"
)]
fn sync_one_terminal_texture(
    terminal_id: super::registry::TerminalId,
    terminal: &mut super::registry::ManagedTerminal,
    presentation_store: &mut TerminalPresentationStore,
    active_target: ActiveRasterTarget,
    font_state: &TerminalFontState,
    font_state_changed: bool,
    active_terminal_content: &ActiveTerminalContentState,
    verification_overrides: Option<&VerificationTerminalSurfaceOverrides>,
    terminal_text_selection: &TerminalTextSelectionState,
    images: &mut Assets<Image>,
    glyph_cache: &mut TerminalGlyphCache,
    text_renderer: &mut TerminalTextRenderer,
) {
    let Some(surface) = raster_source_surface(
        terminal_id,
        terminal,
        active_target,
        active_terminal_content,
        verification_overrides,
    ) else {
        terminal.pending_damage = None;
        return;
    };
    let Some(presented_terminal) = presentation_store.get_mut(terminal_id) else {
        terminal.pending_damage = None;
        return;
    };

    let upload_state = upload_state_for_terminal(
        terminal_id,
        surface,
        presented_terminal,
        active_target,
        font_state,
    );
    let active_override_revision = active_override_revision_for_terminal(
        terminal_id,
        active_terminal_content,
        verification_overrides,
    );
    let terminal_selection_revision =
        terminal_text_selection.presentation_revision_for(terminal_id);
    let mut plan = terminal_raster_work_plan(
        terminal,
        surface,
        presented_terminal,
        upload_state,
        active_override_revision,
        terminal_selection_revision,
        font_state_changed,
    );

    if plan.dirty_rows.is_empty() {
        return;
    }

    let Some(image_handle) = ensure_terminal_target_image(
        presented_terminal,
        images,
        &plan.upload_state,
        surface,
        &mut plan.full_redraw,
        &mut plan.dirty_rows,
    ) else {
        append_debug_log("texture sync: target image missing in assets");
        return;
    };
    let Some(target_image) = images.get_mut(&image_handle) else {
        append_debug_log("texture sync: target image missing in assets");
        return;
    };

    ensure_target_image_matches_upload_state(
        target_image,
        &plan.upload_state,
        surface,
        &mut plan.full_redraw,
        &mut plan.dirty_rows,
    );
    let pixels = ensure_terminal_pixel_buffer(
        target_image,
        &plan.upload_state,
        surface,
        &mut plan.full_redraw,
        &mut plan.dirty_rows,
    );

    if let Some(rows) = maybe_translate_existing_pixels(
        pixels,
        terminal,
        surface,
        presented_terminal,
        &plan.upload_state,
        plan.active_override_revision,
        plan.terminal_selection_revision,
        plan.full_redraw,
    ) {
        plan.dirty_rows = rows;
    }

    if plan.full_redraw {
        clear_terminal_pixels(pixels);
    }

    let compose_started = std::time::Instant::now();
    repaint_terminal_pixels(
        pixels,
        plan.upload_state.texture_size.x,
        surface,
        terminal_text_selection.override_selection_for(terminal_id),
        &plan.dirty_rows,
        plan.upload_state.cell_size,
        text_renderer,
        glyph_cache,
        font_state,
    );
    let compose_elapsed = compose_started.elapsed();
    terminal
        .bridge
        .note_compose(plan.dirty_rows.len(), compose_elapsed.as_micros() as u64);

    if env::var_os("NEOZEUS_DUMP_TEXTURE").is_some() {
        let dump_path = resolve_debug_texture_dump_path();
        let _ = dump_terminal_image_ppm(target_image, dump_path.as_path());
    }

    let uploaded_surface = surface.clone();
    finalize_terminal_upload(terminal, presented_terminal, uploaded_surface, &plan);
}

#[allow(
    clippy::too_many_arguments,
    reason = "texture sync needs terminal, presentation, font, HUD layout, window, image, and renderer state together"
)]
/// Re-rasterizes terminal surfaces into Bevy images when new terminal state, layout, or font state
/// demands it.
///
/// The sync path picks an upload texture contract per terminal, resizes images when necessary, limits
/// repaint work to damaged rows when possible, and clears/rebuilds the glyph cache when font state
/// changes.
pub(crate) fn sync_terminal_texture(
    mut terminal_manager: ResMut<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    font_state: Res<TerminalFontState>,
    view_state: Res<TerminalViewState>,
    layout_state: Res<HudLayoutState>,
    active_terminal_content: Res<ActiveTerminalContentState>,
    verification_overrides: Option<Res<VerificationTerminalSurfaceOverrides>>,
    terminal_text_selection: Res<TerminalTextSelectionState>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut glyph_cache: ResMut<TerminalGlyphCache>,
    mut images: ResMut<Assets<Image>>,
    mut text_renderer: ResMut<TerminalTextRenderer>,
) {
    if text_renderer.font_system.is_none() {
        append_debug_log("texture sync: no font system");
        return;
    }

    if font_state.is_changed() {
        append_debug_log("texture sync: font state changed, clearing glyph cache");
        glyph_cache.glyphs.clear();
    }

    let active_target = active_raster_target(
        &terminal_manager,
        &focus_state,
        &primary_window,
        &layout_state,
        &view_state,
        &font_state,
    );
    let verification_overrides = verification_overrides.as_deref();
    for (terminal_id, terminal) in terminal_manager.iter_mut() {
        sync_one_terminal_texture(
            terminal_id,
            terminal,
            &mut presentation_store,
            active_target,
            &font_state,
            font_state.is_changed(),
            &active_terminal_content,
            verification_overrides,
            &terminal_text_selection,
            &mut images,
            &mut glyph_cache,
            &mut text_renderer,
        );
    }
}

/// Fills the entire terminal texture buffer with the default background color.
fn clear_terminal_pixels(buffer: &mut [u8]) {
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[
            DEFAULT_BG.r(),
            DEFAULT_BG.g(),
            DEFAULT_BG.b(),
            DEFAULT_BG.a(),
        ]);
    }
}

/// Tries to reuse already-rasterized pixels for pure viewport scroll translation.
///
/// When the visible surface only changed by a vertical `display_offset` shift and the overlapping
/// rows still match exactly, the existing texture can be shifted by whole cell rows and only the
/// newly exposed rows plus cursor rows need repainting.
fn translate_terminal_viewport_pixels(
    buffer: &mut [u8],
    texture_size: UVec2,
    cell_size: UVec2,
    previous: &TerminalSurface,
    current: &TerminalSurface,
) -> Option<Vec<usize>> {
    let shift_rows = detect_vertical_surface_translation(previous, current)?;
    let shift_px = shift_rows
        .unsigned_abs()
        .saturating_mul(cell_size.y as usize);
    if shift_px == 0 || shift_px >= texture_size.y as usize {
        return None;
    }

    shift_terminal_pixel_rows(buffer, texture_size, shift_rows);

    let mut dirty = exposed_rows_for_translation(current.rows, shift_rows);
    if let Some(cursor) = previous
        .cursor
        .as_ref()
        .filter(|cursor| cursor.y < current.rows)
    {
        dirty.push(cursor.y);
    }
    if let Some(cursor) = current
        .cursor
        .as_ref()
        .filter(|cursor| cursor.y < current.rows)
    {
        dirty.push(cursor.y);
    }
    dirty.sort_unstable();
    dirty.dedup();
    Some(dirty)
}

fn detect_vertical_surface_translation(
    previous: &TerminalSurface,
    current: &TerminalSurface,
) -> Option<isize> {
    if previous.cols != current.cols || previous.rows != current.rows {
        return None;
    }

    let shift_rows = current.display_offset as isize - previous.display_offset as isize;
    if shift_rows == 0 || shift_rows.unsigned_abs() >= current.rows {
        return None;
    }

    if shift_rows > 0 {
        let shift = shift_rows as usize;
        for row in shift..current.rows {
            if !surface_rows_match(previous, row - shift, current, row) {
                return None;
            }
        }
    } else {
        let shift = shift_rows.unsigned_abs();
        for row in 0..(current.rows - shift) {
            if !surface_rows_match(previous, row + shift, current, row) {
                return None;
            }
        }
    }

    Some(shift_rows)
}

fn surface_rows_match(
    previous: &TerminalSurface,
    previous_row: usize,
    current: &TerminalSurface,
    current_row: usize,
) -> bool {
    (0..current.cols).all(|x| previous.cell(x, previous_row) == current.cell(x, current_row))
}

fn exposed_rows_for_translation(rows: usize, shift_rows: isize) -> Vec<usize> {
    if shift_rows > 0 {
        (0..shift_rows as usize).collect()
    } else {
        ((rows - shift_rows.unsigned_abs())..rows).collect()
    }
}

fn shift_terminal_pixel_rows(buffer: &mut [u8], texture_size: UVec2, shift_rows: isize) {
    let stride = texture_size.x as usize * 4;
    let total_rows = texture_size.y as usize;
    let shift = shift_rows.unsigned_abs();
    if shift == 0 || shift >= total_rows {
        return;
    }

    let blank_pixel = [
        DEFAULT_BG.r(),
        DEFAULT_BG.g(),
        DEFAULT_BG.b(),
        DEFAULT_BG.a(),
    ];
    let fill_rows = |buffer: &mut [u8], start_row: usize, end_row: usize| {
        for row in start_row..end_row.min(total_rows) {
            let row_slice = &mut buffer[row * stride..(row + 1) * stride];
            for pixel in row_slice.chunks_exact_mut(4) {
                pixel.copy_from_slice(&blank_pixel);
            }
        }
    };

    if shift_rows > 0 {
        buffer.copy_within(0..(total_rows - shift) * stride, shift * stride);
        fill_rows(buffer, 0, shift);
    } else {
        buffer.copy_within(shift * stride..total_rows * stride, 0);
        fill_rows(buffer, total_rows - shift, total_rows);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal row repaint needs renderer/cache/font state together"
)]
/// Repaints the specified terminal rows into the texture buffer.
///
/// Each cell redraw paints background first, then glyphs, and finally cursor overlay for rows that
/// contain the cursor.
fn repaint_terminal_pixels(
    buffer: &mut [u8],
    texture_width: u32,
    surface: &TerminalSurface,
    selection: Option<&crate::text_selection::TerminalTextSelection>,
    rows: &[usize],
    cell_size: UVec2,
    text_renderer: &mut TerminalTextRenderer,
    glyph_cache: &mut TerminalGlyphCache,
    font_state: &TerminalFontState,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let stride = texture_width as usize * 4;

    for &y in rows {
        if y >= surface.rows {
            continue;
        }

        for x in 0..surface.cols {
            let cell = surface.cell(x, y);
            let origin_x = x as u32 * cell_size.x;
            let origin_y = y as u32 * cell_size.y;
            let selected = cell.selected
                || selection.is_some_and(|selection| terminal_cell_selected(selection, x, y));
            let effective_fg = selected_foreground_color(cell, selected);
            fill_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                cell_size.x,
                cell_size.y,
                selected_background_color(cell, selected),
            );

            if cell.width != 0 && !cell.content.is_empty() {
                let (font_role, preserve_color) =
                    select_terminal_font_role(&cell.content, font_state);
                let cache_key = TerminalGlyphCacheKey {
                    content: cell.content.clone(),
                    font_role,
                    bold: cell.style.bold,
                    italic: cell.style.italic,
                    width_cells: cell.width,
                    cell_width: cell_size.x,
                    cell_height: cell_size.y,
                };

                if !glyph_cache.glyphs.contains_key(&cache_key) {
                    let glyph =
                        try_rasterize_box_drawing(&cell.content, cell_size).unwrap_or_else(|| {
                            rasterize_terminal_glyph(
                                &cache_key,
                                font_role,
                                preserve_color,
                                text_renderer,
                                font_state,
                            )
                        });
                    glyph_cache.glyphs.insert(cache_key.clone(), glyph);
                }

                if let Some(glyph) = glyph_cache.glyphs.get(&cache_key) {
                    blit_cached_glyph_in_buffer(
                        buffer,
                        stride,
                        origin_x,
                        origin_y,
                        glyph,
                        effective_fg,
                        1.0,
                    );
                }
            }

            if cell.width != 0 {
                paint_cell_decorations(
                    buffer,
                    stride,
                    origin_x,
                    origin_y,
                    cell_size,
                    cell,
                    effective_fg,
                );
            }
        }
    }

    if let Some(cursor) = &surface.cursor {
        if cursor.visible && rows.binary_search(&cursor.y).is_ok() {
            draw_cursor_in_buffer(buffer, stride, cursor, cell_size);
        }
    }
}

fn terminal_cell_selected(
    selection: &crate::text_selection::TerminalTextSelection,
    x: usize,
    y: usize,
) -> bool {
    let start = (selection.anchor.row, selection.anchor.col);
    let end = (selection.focus.row, selection.focus.col);
    let ((start_row, start_col), (end_row, end_col)) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    (y, x) >= (start_row, start_col) && (y, x) <= (end_row, end_col)
}

fn selected_background_color(cell: &TerminalCell, selected: bool) -> egui::Color32 {
    if !selected {
        return effective_background_color(cell);
    }
    let bg = effective_background_color(cell);
    let fg = effective_foreground_color(cell);
    egui::Color32::from_rgba_unmultiplied(
        ((u16::from(bg.r()) + u16::from(fg.r()) + 255) / 3) as u8,
        ((u16::from(bg.g()) + u16::from(fg.g()) + 255) / 3) as u8,
        ((u16::from(bg.b()) + u16::from(fg.b()) + 255) / 3) as u8,
        255,
    )
}

fn selected_foreground_color(cell: &TerminalCell, selected: bool) -> egui::Color32 {
    if !selected {
        return effective_foreground_color(cell);
    }
    let bg = selected_background_color(cell, true);
    let luminance =
        (u32::from(bg.r()) * 212 + u32::from(bg.g()) * 715 + u32::from(bg.b()) * 72) / 1000;
    if luminance > 140 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    }
}

fn effective_foreground_color(cell: &TerminalCell) -> egui::Color32 {
    apply_dim_to_color(cell.fg, cell.bg, cell.style.dim)
}

fn effective_background_color(cell: &TerminalCell) -> egui::Color32 {
    cell.bg
}

fn effective_underline_color(cell: &TerminalCell) -> egui::Color32 {
    apply_dim_to_color(
        cell.style.underline_color.unwrap_or(cell.fg),
        cell.bg,
        cell.style.dim,
    )
}

fn apply_dim_to_color(color: egui::Color32, background: egui::Color32, dim: bool) -> egui::Color32 {
    if dim {
        blend_color_toward_background(color, background, 10, 11)
    } else {
        color
    }
}

fn blend_color_toward_background(
    color: egui::Color32,
    background: egui::Color32,
    color_weight: u16,
    total_weight: u16,
) -> egui::Color32 {
    let bg_weight = total_weight.saturating_sub(color_weight);
    let blend_channel = |fg: u8, bg: u8| -> u8 {
        (((u16::from(fg) * color_weight) + (u16::from(bg) * bg_weight)) / total_weight) as u8
    };
    egui::Color32::from_rgba_unmultiplied(
        blend_channel(color.r(), background.r()),
        blend_channel(color.g(), background.g()),
        blend_channel(color.b(), background.b()),
        color.a(),
    )
}

fn paint_cell_decorations(
    buffer: &mut [u8],
    stride: usize,
    origin_x: u32,
    origin_y: u32,
    cell_size: UVec2,
    cell: &TerminalCell,
    effective_fg: egui::Color32,
) {
    let width = cell_size.x * u32::from(cell.width.max(1));
    if width == 0 {
        return;
    }

    draw_underline_in_buffer(
        buffer,
        stride,
        origin_x,
        origin_y,
        width,
        cell_size.y,
        cell.style.underline,
        effective_underline_color(cell),
    );

    if cell.style.strikeout {
        draw_strikeout_in_buffer(
            buffer,
            stride,
            origin_x,
            origin_y,
            width,
            cell_size.y,
            effective_fg,
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "underline painting needs buffer geometry plus per-cell style inputs together"
)]
fn draw_underline_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    origin_x: u32,
    origin_y: u32,
    width: u32,
    cell_height: u32,
    underline: TerminalUnderlineStyle,
    color: egui::Color32,
) {
    if underline == TerminalUnderlineStyle::None {
        return;
    }

    let thickness = (cell_height / 16).max(1);
    let baseline_y =
        origin_y + cell_height.saturating_sub((cell_height / 9).max(1).saturating_add(thickness));
    match underline {
        TerminalUnderlineStyle::None => {}
        TerminalUnderlineStyle::Single => {
            fill_rect_in_buffer(
                buffer, stride, origin_x, baseline_y, width, thickness, color,
            );
        }
        TerminalUnderlineStyle::Double => {
            let gap = thickness.max(1);
            fill_rect_in_buffer(
                buffer, stride, origin_x, baseline_y, width, thickness, color,
            );
            fill_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                baseline_y.saturating_sub(gap + thickness),
                width,
                thickness,
                color,
            );
        }
        TerminalUnderlineStyle::Curly => {
            let period = (thickness * 4).max(4);
            let crest_y = baseline_y.saturating_sub(thickness.max(1));
            for dx in 0..width {
                let phase = (dx % period) * 4 / period;
                let wave_y = if phase == 1 || phase == 2 {
                    crest_y
                } else {
                    baseline_y
                };
                fill_rect_in_buffer(buffer, stride, origin_x + dx, wave_y, 1, thickness, color);
            }
        }
        TerminalUnderlineStyle::Dotted => {
            let dot = thickness.max(1);
            let step = (dot * 2).max(2) as usize;
            for dx in (0..width).step_by(step) {
                fill_rect_in_buffer(
                    buffer,
                    stride,
                    origin_x + dx,
                    baseline_y,
                    dot,
                    thickness,
                    color,
                );
            }
        }
        TerminalUnderlineStyle::Dashed => {
            let dash = (thickness * 4).max(3);
            let gap = (thickness * 2).max(2);
            let step = (dash + gap) as usize;
            for dx in (0..width).step_by(step) {
                fill_rect_in_buffer(
                    buffer,
                    stride,
                    origin_x + dx,
                    baseline_y,
                    dash.min(width - dx),
                    thickness,
                    color,
                );
            }
        }
    }
}

fn draw_strikeout_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    origin_x: u32,
    origin_y: u32,
    width: u32,
    cell_height: u32,
    color: egui::Color32,
) {
    let thickness = (cell_height / 16).max(1);
    let strike_y = origin_y + (cell_height / 2).saturating_sub(thickness / 2);
    fill_rect_in_buffer(buffer, stride, origin_x, strike_y, width, thickness, color);
}

/// Attempts to rasterize a box-drawing glyph without going through font shaping.
fn try_rasterize_box_drawing(
    content: &TerminalCellContent,
    cell_size: UVec2,
) -> Option<CachedTerminalGlyph> {
    let ch = match content {
        TerminalCellContent::Single(ch) if is_box_drawing(*ch) => *ch,
        _ => return None,
    };
    rasterize_box_drawing(ch, cell_size.x, cell_size.y)
}

/// Chooses which font role should render a cell's content and whether that glyph should preserve its
/// own color.
///
/// Emoji uses color-preserving rendering when an emoji fallback exists; private-use glyphs prefer the
/// private-use fallback when available.
fn select_terminal_font_role(
    content: &TerminalCellContent,
    font_state: &TerminalFontState,
) -> (TerminalFontRole, bool) {
    if content.any_char(is_emoji_like) && font_state.has_emoji_font() {
        return (TerminalFontRole::Emoji, true);
    }

    if content.any_char(is_private_use_like) && font_state.has_private_use_font() {
        return (TerminalFontRole::PrivateUse, false);
    }

    (TerminalFontRole::Primary, false)
}

/// Handles text attrs.
fn terminal_text_attrs<'a>(
    font_role: TerminalFontRole,
    bold: bool,
    italic: bool,
    font_state: &'a TerminalFontState,
) -> CtAttrs<'a> {
    let family = match font_role {
        TerminalFontRole::Primary => CtFamily::Monospace,
        TerminalFontRole::PrivateUse => font_state
            .fallback_family_name("private-use")
            .map(CtFamily::Name)
            .unwrap_or(CtFamily::Monospace),
        TerminalFontRole::Emoji => font_state
            .fallback_family_name("emoji")
            .map(CtFamily::Name)
            .unwrap_or(CtFamily::Monospace),
    };
    CtAttrs::new()
        .family(family)
        .weight(if bold {
            CtWeight::BOLD
        } else {
            CtWeight::NORMAL
        })
        .style(if italic {
            CtStyle::Italic
        } else {
            CtStyle::Normal
        })
}

/// Rasterizes one glyph-cache entry into an RGBA pixel buffer using cosmic-text and swash.
///
/// The cached glyph is rendered in white alpha unless `preserve_color` is requested, in which case
/// embedded glyph colors are kept.
pub(crate) fn rasterize_terminal_glyph(
    cache_key: &TerminalGlyphCacheKey,
    font_role: TerminalFontRole,
    preserve_color: bool,
    text_renderer: &mut TerminalTextRenderer,
    font_state: &TerminalFontState,
) -> CachedTerminalGlyph {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let width = cache_key.cell_width * u32::from(cache_key.width_cells.max(1));
    let height = cache_key.cell_height.max(1);
    let mut pixels = vec![0; (width * height * 4) as usize];

    let Some(font_system) = text_renderer.font_system.as_mut() else {
        return CachedTerminalGlyph {
            width,
            height,
            pixels,
            preserve_color,
        };
    };

    let metrics = font_state.glyph_metrics();
    let mut buffer = CtBuffer::new_empty(metrics);
    {
        let mut borrowed = buffer.borrow_with(font_system);
        borrowed.set_size(Some(width as f32), Some(height as f32));
        let attrs = terminal_text_attrs(font_role, cache_key.bold, cache_key.italic, font_state)
            .metrics(metrics);
        let text = cache_key.content.to_owned_string();
        borrowed.set_text(text.as_str(), &attrs, CtShaping::Advanced, None);
        borrowed.shape_until_scroll(false);
    }

    let base_color = CtColor::rgb(0xFF, 0xFF, 0xFF);
    let baseline_offset = font_state.baseline_offset();
    for run in buffer.layout_runs() {
        let snapped_baseline_y = (run.line_y + baseline_offset).round();
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, snapped_baseline_y), 1.0);
            text_renderer.swash_cache.with_pixels(
                font_system,
                physical.cache_key,
                base_color,
                |x, y, color| {
                    let rgba = color.as_rgba();
                    let source = if preserve_color {
                        rgba
                    } else {
                        [255, 255, 255, rgba[3]]
                    };
                    let target_x = physical.x + x;
                    let target_y = physical.y + y;
                    if target_x < 0
                        || target_y < 0
                        || target_x >= width as i32
                        || target_y >= height as i32
                    {
                        return;
                    }
                    blend_over_pixel(&mut pixels, width, target_x as u32, target_y as u32, source);
                },
            );
        }
    }

    CachedTerminalGlyph {
        width,
        height,
        pixels,
        preserve_color,
    }
}

/// Blends a cached glyph bitmap into the destination terminal texture buffer at the requested cell
/// origin.
///
/// Non-color glyphs are tinted with the cell foreground color at blit time.
fn blit_cached_glyph_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    origin_x: u32,
    origin_y: u32,
    glyph: &CachedTerminalGlyph,
    fg: egui::Color32,
    alpha_scale: f32,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let max_height = buffer.len() / stride;
    for y in 0..glyph.height as usize {
        let target_y = origin_y as usize + y;
        if target_y >= max_height {
            break;
        }
        let dst_row = &mut buffer[target_y * stride..(target_y + 1) * stride];
        let src_row =
            &glyph.pixels[y * glyph.width as usize * 4..(y + 1) * glyph.width as usize * 4];
        for x in 0..glyph.width as usize {
            let src = &src_row[x * 4..x * 4 + 4];
            if src[3] == 0 {
                continue;
            }

            let scaled_alpha = (src[3] as f32 * alpha_scale).round().clamp(0.0, 255.0) as u8;
            let source = if glyph.preserve_color {
                [src[0], src[1], src[2], scaled_alpha]
            } else {
                [fg.r(), fg.g(), fg.b(), scaled_alpha]
            };
            let dst_start = (origin_x as usize + x) * 4;
            if dst_start + 4 > dst_row.len() {
                break;
            }
            blend_rgba_in_place(&mut dst_row[dst_start..dst_start + 4], source);
        }
    }
}

/// Fills a solid rectangle inside the raw RGBA terminal texture buffer.
fn fill_rect_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: egui::Color32,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let pixel = [color.r(), color.g(), color.b(), color.a()];
    let max_height = buffer.len() / stride;
    for row in y as usize..(y as usize).saturating_add(height as usize).min(max_height) {
        let row_slice = &mut buffer[row * stride..(row + 1) * stride];
        let start = x as usize * 4;
        let end = ((x + width) as usize * 4).min(row_slice.len());
        if start >= end {
            continue;
        }
        for dst in row_slice[start..end].chunks_exact_mut(4) {
            dst.copy_from_slice(&pixel);
        }
    }
}

/// Draws the terminal cursor overlay into the raw RGBA texture buffer according to the cursor shape.
fn draw_cursor_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    cursor: &TerminalCursor,
    cell_size: UVec2,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    let origin_x = cursor.x as u32 * cell_size.x;
    let origin_y = cursor.y as u32 * cell_size.y;
    let color = [cursor.color.r(), cursor.color.g(), cursor.color.b(), 255];

    match cursor.shape {
        TerminalCursorShape::Block => {
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                cell_size.x,
                cell_size.y,
                color,
            );
        }
        TerminalCursorShape::Underline => {
            let height = (cell_size.y / 8).max(1);
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y + cell_size.y.saturating_sub(height),
                cell_size.x,
                height,
                color,
            );
        }
        TerminalCursorShape::Beam => {
            let width = (cell_size.x / 10).max(1);
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                width,
                cell_size.y,
                color,
            );
        }
    }
}

/// Alpha-blends a solid rectangle into the raw RGBA terminal texture buffer.
fn fill_alpha_rect_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let max_height = buffer.len() / stride;
    for row in y as usize..(y as usize).saturating_add(height as usize).min(max_height) {
        let row_slice = &mut buffer[row * stride..(row + 1) * stride];
        let start = x as usize * 4;
        let end = ((x + width) as usize * 4).min(row_slice.len());
        if start >= end {
            continue;
        }
        for dst in row_slice[start..end].chunks_exact_mut(4) {
            blend_rgba_in_place(dst, color);
        }
    }
}

/// Alpha-blends one source RGBA pixel over the destination pixel at `(x, y)` inside a tightly packed
/// RGBA image buffer.
fn blend_over_pixel(buffer: &mut [u8], width: u32, x: u32, y: u32, source: [u8; 4]) {
    let index = ((y * width + x) * 4) as usize;
    blend_rgba_in_place(&mut buffer[index..index + 4], source);
}

/// Alpha-composites one RGBA source pixel over a mutable destination pixel slice in place.
///
/// Both colors are treated as straight alpha.
fn blend_rgba_in_place(dst: &mut [u8], source: [u8; 4]) {
    let src_alpha = source[3] as f32 / 255.0;
    let dst_alpha = dst[3] as f32 / 255.0;
    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);

    if out_alpha <= f32::EPSILON {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }

    for channel in 0..3 {
        let src = source[channel] as f32 / 255.0;
        let dst_value = dst[channel] as f32 / 255.0;
        let out = (src * src_alpha + dst_value * dst_alpha * (1.0 - src_alpha)) / out_alpha;
        dst[channel] = (out * 255.0).round() as u8;
    }

    dst[3] = (out_alpha * 255.0).round() as u8;
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_config::{
            load_neozeus_config, resolve_terminal_baseline_offset_px, resolve_terminal_font_path,
            resolve_terminal_font_size_px, DEFAULT_BG, DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX,
        },
        hud::{HudState, HudWidgetKey},
        terminals::{active_terminal_cell_size, active_terminal_dimensions, active_terminal_layout},
    };
    use bevy::{ecs::system::RunSystemOnce, prelude::*, window::PrimaryWindow};
    use bevy_egui::egui;
    use cosmic_text::{Style as CtStyle, Weight as CtWeight};
    use std::{
        fs,
        path::Path,
        sync::{mpsc, Arc, Mutex},
    };

    use super::super::{
        bridge::TerminalBridge,
        debug::TerminalDebugStats,
        fonts::{
            initialize_terminal_text_renderer_with_locale, measure_monospace_cell,
            resolve_terminal_font_report_for_family, resolve_terminal_font_report_for_path,
            TerminalFontRasterConfig,
        },
        mailbox::TerminalUpdateMailbox,
        presentation_state::{
            PresentedTerminal, TerminalDisplayMode, TerminalPresentationStore, TerminalTextureState,
            TerminalViewState,
        },
        registry::TerminalManager,
        types::{
            TerminalCell, TerminalCellContent, TerminalCellStyle, TerminalDamage, TerminalDimensions,
            TerminalFontReport, TerminalSurface, TerminalUnderlineStyle,
        },
    };

    /// Resolves the host's effective monospace terminal font stack for raster tests.
    fn test_terminal_font_report() -> TerminalFontReport {
        resolve_terminal_font_report_for_family("monospace")
            .expect("failed to resolve terminal fonts for test family")
    }

    /// Resolves the configured terminal font report when one is explicitly configured, otherwise falls
    /// back to the host default.
    fn configured_terminal_font_report() -> TerminalFontReport {
        let config = load_neozeus_config().unwrap_or_default();
        if let Some(path) = resolve_terminal_font_path(&config) {
            resolve_terminal_font_report_for_path(&path)
                .expect("failed to resolve configured terminal font report")
        } else {
            test_terminal_font_report()
        }
    }

    /// Initializes a terminal text renderer for tests with a fixed locale.
    fn initialize_test_terminal_text_renderer(
        report: &TerminalFontReport,
        renderer: &mut TerminalTextRenderer,
    ) {
        initialize_terminal_text_renderer_with_locale(report, renderer, "en-US")
            .expect("failed to initialize terminal text renderer");
    }

    /// Computes the raster font sizing config that raster tests should use.
    fn configured_test_font_raster() -> TerminalFontRasterConfig {
        let config = load_neozeus_config().unwrap_or_default();
        let defaults = TerminalFontRasterConfig::default();
        TerminalFontRasterConfig {
            font_size_px: resolve_terminal_font_size_px(&config, defaults.font_size_px),
            baseline_offset_px: resolve_terminal_baseline_offset_px(
                &config,
                defaults.baseline_offset_px,
            ),
        }
    }

    /// Builds a fully initialized font state with measured cell metrics for raster tests.
    fn configured_test_font_state(
        report: TerminalFontReport,
        renderer: &mut TerminalTextRenderer,
    ) -> TerminalFontState {
        let raster = configured_test_font_raster();
        let cell_metrics = renderer
            .font_system
            .as_mut()
            .and_then(|fs| measure_monospace_cell(fs, raster.font_size_px))
            .unwrap_or_default();
        TerminalFontState {
            report: Some(Ok(report)),
            raster,
            cell_metrics,
        }
    }

    /// Creates a bare test terminal bridge suitable for raster-only tests.
    fn test_bridge() -> TerminalBridge {
        test_bridge_with_stats().0
    }

    fn test_bridge_with_stats() -> (TerminalBridge, Arc<Mutex<TerminalDebugStats>>) {
        let (input_tx, _input_rx) = mpsc::channel();
        let stats = Arc::new(Mutex::new(TerminalDebugStats::default()));
        (
            TerminalBridge::new(
                input_tx,
                Arc::new(TerminalUpdateMailbox::default()),
                stats.clone(),
            ),
            stats,
        )
    }

    /// Inserts the terminal manager together with the mirrored focus-state test resource.
    fn insert_terminal_manager_resources(world: &mut World, terminal_manager: TerminalManager) {
        world.insert_resource(terminal_manager.clone_focus_state());
        world.insert_resource(terminal_manager);
    }

    /// Writes colored single-width text into a terminal surface row for rasterization tests.
    fn set_colored_text(
        surface: &mut TerminalSurface,
        row: usize,
        col: usize,
        text: &str,
        fg: egui::Color32,
    ) {
        for (offset, ch) in text.chars().enumerate() {
            if col + offset >= surface.cols {
                break;
            }
            surface.set_cell(
                col + offset,
                row,
                TerminalCell {
                    content: TerminalCellContent::Single(ch),
                    fg,
                    bg: DEFAULT_BG,
                    style: Default::default(),
                    width: 1,
                    selected: false,
                },
            );
        }
    }

    #[test]
    fn terminal_raster_work_plan_preserves_row_damage_for_incremental_uploads() {
        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal(test_bridge());
        let terminal = manager.get_mut(terminal_id).expect("terminal should exist");
        terminal.snapshot.surface = Some(TerminalSurface::new(4, 3));
        terminal.surface_revision = 2;
        terminal.pending_damage = Some(TerminalDamage::Rows(vec![1, 2]));

        let surface = terminal.snapshot.surface.as_ref().unwrap();
        let upload_state = TerminalTextureState {
            texture_size: UVec2::new(40, 30),
            cell_size: UVec2::new(10, 10),
        };
        let presented = PresentedTerminal {
            image: Default::default(),
            texture_state: upload_state.clone(),
            desired_texture_state: upload_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        };

        let plan = terminal_raster_work_plan(
            terminal,
            surface,
            &presented,
            upload_state,
            None,
            None,
            false,
        );

        assert!(!plan.full_redraw);
        assert_eq!(plan.dirty_rows, vec![1, 2]);
    }

    #[test]
    fn terminal_raster_work_plan_forces_full_redraw_when_texture_contract_changes() {
        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal(test_bridge());
        let terminal = manager.get_mut(terminal_id).expect("terminal should exist");
        terminal.snapshot.surface = Some(TerminalSurface::new(4, 3));
        terminal.surface_revision = 1;
        terminal.pending_damage = Some(TerminalDamage::Rows(vec![2]));

        let surface = terminal.snapshot.surface.as_ref().unwrap();
        let presented = PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(40, 30),
                cell_size: UVec2::new(10, 10),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: UVec2::new(40, 30),
                cell_size: UVec2::new(10, 10),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        };
        let upload_state = TerminalTextureState {
            texture_size: UVec2::new(80, 60),
            cell_size: UVec2::new(20, 20),
        };

        let plan = terminal_raster_work_plan(
            terminal,
            surface,
            &presented,
            upload_state,
            None,
            None,
            false,
        );

        assert!(plan.full_redraw);
        assert_eq!(plan.dirty_rows, vec![0, 1, 2]);
    }

    /// Runs the normal terminal-texture sync pipeline on a supplied surface and returns the rendered
    /// image plus the texture state it ended up using.
    fn render_surface_to_terminal_image(surface: TerminalSurface) -> (Image, TerminalTextureState) {
        let report = configured_terminal_font_report();
        let mut renderer = TerminalTextRenderer::default();
        initialize_test_terminal_text_renderer(&report, &mut renderer);
        let font_state = configured_test_font_state(report, &mut renderer);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let view_state = TerminalViewState::default();

        let bridge = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);
        let terminal = manager.get_mut(id).expect("terminal should exist");
        terminal.snapshot.surface = Some(surface);
        terminal.surface_revision = 1;
        terminal.pending_damage = Some(TerminalDamage::Full);

        let mut images = Assets::<Image>::default();
        let image = images.add(create_terminal_image(UVec2::ONE));
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: image.clone(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut world = World::default();
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(font_state);
        world.insert_resource(view_state);
        world.insert_resource(hud_state.layout_state());
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
        world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
        world.insert_resource(TerminalGlyphCache::default());
        world.insert_resource(renderer);
        world.insert_resource(images);
        world.spawn((window, PrimaryWindow));

        world
            .run_system_once(sync_terminal_texture)
            .expect("texture sync should succeed");

        let store = world.resource::<TerminalPresentationStore>();
        let presented = store.get(id).expect("missing presented terminal");
        let texture_state = presented.texture_state.clone();
        let images = world.resource::<Assets<Image>>();
        let image = images
            .get(&presented.image)
            .expect("rendered image should exist")
            .clone();
        (image, texture_state)
    }

    /// Renders a surface to a terminal image while starting from an explicit texture-state contract.
    fn render_surface_to_terminal_image_with_presentation_state(
        surface: TerminalSurface,
        presentation_state: TerminalTextureState,
    ) -> (Image, TerminalTextureState) {
        let report = configured_terminal_font_report();
        let mut renderer = TerminalTextRenderer::default();
        initialize_test_terminal_text_renderer(&report, &mut renderer);
        let mut best_size = configured_test_font_raster().font_size_px;
        let mut best_metrics = renderer
            .font_system
            .as_mut()
            .and_then(|fs| measure_monospace_cell(fs, best_size))
            .unwrap_or_default();
        let mut best_score = u32::MAX;
        for step in 32..=160 {
            let size = step as f32 * 0.25;
            let Some(metrics) = renderer
                .font_system
                .as_mut()
                .and_then(|fs| measure_monospace_cell(fs, size))
            else {
                continue;
            };
            let score = metrics.cell_width.abs_diff(presentation_state.cell_size.x)
                + metrics.cell_height.abs_diff(presentation_state.cell_size.y);
            if score < best_score {
                best_score = score;
                best_size = size;
                best_metrics = metrics;
                if score == 0 {
                    break;
                }
            }
        }
        let font_state = TerminalFontState {
            report: Some(Ok(report)),
            raster: TerminalFontRasterConfig {
                font_size_px: best_size,
                baseline_offset_px: configured_test_font_raster().baseline_offset_px,
            },
            cell_metrics: best_metrics,
        };

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let view_state = TerminalViewState::default();

        let bridge = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);
        let terminal = manager.get_mut(id).expect("terminal should exist");
        terminal.snapshot.surface = Some(surface);
        terminal.surface_revision = 1;
        terminal.pending_damage = Some(TerminalDamage::Full);

        let mut images = Assets::<Image>::default();
        let image = images.add(create_terminal_image(presentation_state.texture_size));
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: image.clone(),
                texture_state: presentation_state.clone(),
                desired_texture_state: presentation_state.clone(),
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 1,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut world = World::default();
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(font_state);
        world.insert_resource(view_state);
        world.insert_resource(hud_state.layout_state());
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
        world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
        world.insert_resource(TerminalGlyphCache::default());
        world.insert_resource(renderer);
        world.insert_resource(images);
        world.spawn((window, PrimaryWindow));

        world
            .run_system_once(sync_terminal_texture)
            .expect("texture sync should succeed");

        let store = world.resource::<TerminalPresentationStore>();
        let presented = store.get(id).expect("missing presented terminal");
        let texture_state = presented.texture_state.clone();
        let images = world.resource::<Assets<Image>>();
        let image = images
            .get(&presented.image)
            .expect("rendered image should exist")
            .clone();
        (image, texture_state)
    }

    /// Counts non-background pixels in a contiguous horizontal band.
    fn count_non_background_pixels_in_band(image: &Image, y_start: u32, y_end: u32) -> usize {
        let size = image.texture_descriptor.size;
        let data = image.data.as_ref().expect("image data should exist");
        let mut count = 0;
        for y in y_start..y_end.min(size.height) {
            for x in 0..size.width {
                let idx = ((y * size.width + x) * 4) as usize;
                let pixel = &data[idx..idx + 4];
                if pixel
                    != [
                        DEFAULT_BG.r(),
                        DEFAULT_BG.g(),
                        DEFAULT_BG.b(),
                        DEFAULT_BG.a(),
                    ]
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// Verifies combined bold+italic styling keeps both font attributes instead of silently dropping italics.
    #[test]
    fn terminal_text_attrs_preserve_bold_and_italic_together() {
        let font_state = TerminalFontState::default();
        let attrs = terminal_text_attrs(TerminalFontRole::Primary, true, true, &font_state);

        assert_eq!(attrs.weight, CtWeight::BOLD);
        assert_eq!(attrs.style, CtStyle::Italic);
    }

    /// Counts non-background pixels inside a rectangular cell-aligned crop.
    fn count_non_background_pixels_in_rect(
        image: &Image,
        x_start: u32,
        y_start: u32,
        width: u32,
        height: u32,
    ) -> usize {
        let size = image.texture_descriptor.size;
        let data = image.data.as_ref().expect("image data should exist");
        let mut count = 0;
        for y in y_start..(y_start + height).min(size.height) {
            for x in x_start..(x_start + width).min(size.width) {
                let idx = ((y * size.width + x) * 4) as usize;
                let pixel = &data[idx..idx + 4];
                if pixel
                    != [
                        DEFAULT_BG.r(),
                        DEFAULT_BG.g(),
                        DEFAULT_BG.b(),
                        DEFAULT_BG.a(),
                    ]
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// Sums visible ink intensity inside a rectangular crop while ignoring untouched background pixels.
    fn summed_non_background_rgb(
        image: &Image,
        x_start: u32,
        y_start: u32,
        width: u32,
        height: u32,
    ) -> u64 {
        let size = image.texture_descriptor.size;
        let data = image.data.as_ref().expect("image data should exist");
        let mut total = 0u64;
        for y in y_start..(y_start + height).min(size.height) {
            for x in x_start..(x_start + width).min(size.width) {
                let idx = ((y * size.width + x) * 4) as usize;
                let pixel = &data[idx..idx + 4];
                if pixel
                    == [
                        DEFAULT_BG.r(),
                        DEFAULT_BG.g(),
                        DEFAULT_BG.b(),
                        DEFAULT_BG.a(),
                    ]
                {
                    continue;
                }
                total += u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]);
            }
        }
        total
    }

    /// Reads a binary `P6` PPM image from disk.
    fn read_binary_ppm(path: &Path) -> (u32, u32, Vec<u8>) {
        let bytes = fs::read(path).expect("ppm should read");
        let mut idx = 0;
        let mut tokens = Vec::new();
        while tokens.len() < 4 {
            while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
                idx += 1;
            }
            if idx < bytes.len() && bytes[idx] == b'#' {
                while idx < bytes.len() && bytes[idx] != b'\n' {
                    idx += 1;
                }
                continue;
            }
            let start = idx;
            while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                idx += 1;
            }
            tokens.push(String::from_utf8(bytes[start..idx].to_vec()).expect("ppm token utf8"));
        }
        assert_eq!(tokens[0], "P6");
        let width = tokens[1].parse::<u32>().expect("ppm width");
        let height = tokens[2].parse::<u32>().expect("ppm height");
        assert_eq!(tokens[3].parse::<u32>().expect("ppm max value"), 255);
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        (width, height, bytes[idx..].to_vec())
    }

    /// Crops a rectangular RGB region from packed row-major RGB bytes.
    fn crop_rgb_rows(data: &[u8], width: u32, x: u32, y: u32, crop_w: u32, crop_h: u32) -> Vec<u8> {
        let stride = width as usize * 3;
        let start_x = x as usize * 3;
        let row_bytes = crop_w as usize * 3;
        let mut out = Vec::with_capacity(crop_h as usize * row_bytes);
        for row in 0..crop_h as usize {
            let row_start = (y as usize + row) * stride + start_x;
            out.extend_from_slice(&data[row_start..row_start + row_bytes]);
        }
        out
    }

    /// Crops a rectangular RGB region from a Bevy RGBA image, discarding alpha.
    fn crop_image_rgb(image: &Image, x: u32, y: u32, crop_w: u32, crop_h: u32) -> Vec<u8> {
        let size = image.texture_descriptor.size;
        let data = image.data.as_ref().expect("image data should exist");
        let mut out = Vec::with_capacity((crop_w * crop_h * 3) as usize);
        for row in 0..crop_h {
            for col in 0..crop_w {
                let px = x + col;
                let py = y + row;
                assert!(
                    px < size.width && py < size.height,
                    "crop exceeds image bounds"
                );
                let idx = ((py * size.width + px) * 4) as usize;
                out.extend_from_slice(&data[idx..idx + 3]);
            }
        }
        out
    }

    /// Loads the reference ANSI screen used by the deterministic PI screenshot raster test.
    fn surface_from_pi_screen_reference_ansi() -> TerminalSurface {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/assets/pi-screen-reference-20260328.ansi");
        let bytes = fs::read(path).expect("pi screen ansi should exist");
        let dimensions = TerminalDimensions {
            cols: 106,
            rows: 38,
        };
        let config = alacritty_terminal::term::Config {
            scrolling_history: 5000,
            ..alacritty_terminal::term::Config::default()
        };
        let mut terminal =
            alacritty_terminal::term::Term::<alacritty_terminal::event::VoidListener>::new(
                config,
                &dimensions,
                alacritty_terminal::event::VoidListener,
            );
        let mut parser = alacritty_terminal::vte::ansi::Processor::<
            alacritty_terminal::vte::ansi::StdSyncHandler,
        >::new();
        parser.advance(&mut terminal, &bytes);
        crate::terminals::build_surface(&terminal)
    }

    /// Verifies the alpha blender leaves fully transparent glyph pixels untouched and accumulates alpha
    /// for partially transparent pixels.
    #[test]
    fn alpha_blend_preserves_transparent_glyph_background() {
        let mut pixel = [0, 0, 0, 0];
        blend_rgba_in_place(&mut pixel, [255, 255, 255, 0]);
        assert_eq!(pixel, [0, 0, 0, 0]);

        blend_rgba_in_place(&mut pixel, [255, 255, 255, 128]);
        assert_eq!(pixel[3], 128);
    }

    /// Verifies the raster path preserves the cached active texture for a switched-to terminal until a
    /// surface matching the new active layout arrives.
    #[test]
    fn sync_terminal_texture_keeps_cached_switch_frame_until_resized_surface_arrives() {
        let report = test_terminal_font_report();
        let mut renderer = TerminalTextRenderer::default();
        initialize_test_terminal_text_renderer(&report, &mut renderer);
        let font_state = TerminalFontState {
            report: Some(Ok(report)),
            ..Default::default()
        };

        let bridge_one = test_bridge();
        let bridge_two = test_bridge();
        let mut manager = TerminalManager::default();
        let id_one = manager.create_terminal(bridge_one);
        let id_two = manager.create_terminal(bridge_two);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let mut hud_state = HudState::default();
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let rect = crate::hud::docked_agent_list_rect(&window);
        hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

        let view_state = TerminalViewState::default();
        let active_dimensions =
            active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state, &font_state);
        let active_cell_size = active_terminal_cell_size(&window, &view_state);
        let active_texture_state = TerminalTextureState {
            texture_size: UVec2::new(
                active_dimensions.cols as u32 * active_cell_size.x,
                active_dimensions.rows as u32 * active_cell_size.y,
            ),
            cell_size: active_cell_size,
        };
        let cached_background_state = TerminalTextureState {
            texture_size: UVec2::new(80 * DEFAULT_CELL_WIDTH_PX, 24 * DEFAULT_CELL_HEIGHT_PX),
            cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
        };

        manager.focus_terminal(id_two);
        let first = manager.get_mut(id_one).unwrap();
        first.snapshot.surface = Some(TerminalSurface::new(
            active_dimensions.cols,
            active_dimensions.rows,
        ));
        first.surface_revision = 1;
        let second = manager.get_mut(id_two).unwrap();
        second.snapshot.surface = Some(TerminalSurface::new(80, 24));
        second.surface_revision = 1;

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id_one,
            PresentedTerminal {
                image: Default::default(),
                texture_state: active_texture_state.clone(),
                desired_texture_state: active_texture_state.clone(),
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 1,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
        presentation_store.register(
            id_two,
            PresentedTerminal {
                image: Default::default(),
                texture_state: cached_background_state.clone(),
                desired_texture_state: cached_background_state.clone(),
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 1,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut world = World::default();
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(font_state);
        world.insert_resource(view_state);
        world.insert_resource(hud_state.layout_state());
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
        world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
        world.insert_resource(TerminalGlyphCache::default());
        world.insert_resource(renderer);
        world.insert_resource(Assets::<Image>::default());
        world.spawn((window, PrimaryWindow));

        world.run_system_once(sync_terminal_texture).unwrap();

        let store = world.resource::<TerminalPresentationStore>();
        let inactive = store.get(id_one).expect("missing inactive terminal");
        assert_eq!(
            inactive.texture_state,
            TerminalTextureState {
                texture_size: UVec2::new(
                    active_dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
                    active_dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
                ),
                cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
            }
        );
        assert_eq!(inactive.desired_texture_state, inactive.texture_state);

        let active = store.get(id_two).expect("missing active terminal");
        assert_eq!(active.texture_state, cached_background_state);
        assert_eq!(active.desired_texture_state, cached_background_state);
    }

    /// Verifies that once the resized active-layout surface finally arrives, texture sync promotes the
    /// active terminal to the new texture contract and revision.
    #[test]
    fn sync_terminal_texture_promotes_active_terminal_once_resized_surface_arrives() {
        let report = test_terminal_font_report();
        let mut renderer = TerminalTextRenderer::default();
        initialize_test_terminal_text_renderer(&report, &mut renderer);
        let font_state = TerminalFontState {
            report: Some(Ok(report)),
            ..Default::default()
        };

        let bridge = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let mut hud_state = HudState::default();
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let rect = crate::hud::docked_agent_list_rect(&window);
        hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

        let view_state = TerminalViewState::default();
        let active_dimensions =
            active_terminal_dimensions(&window, &hud_state.layout_state(), &view_state, &font_state);
        let active_cell_size = active_terminal_cell_size(&window, &view_state);
        let active_texture_state = TerminalTextureState {
            texture_size: UVec2::new(
                active_dimensions.cols as u32 * active_cell_size.x,
                active_dimensions.rows as u32 * active_cell_size.y,
            ),
            cell_size: active_cell_size,
        };
        let cached_background_state = TerminalTextureState {
            texture_size: UVec2::new(
                active_dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
                active_dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
            ),
            cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
        };

        let terminal = manager.get_mut(id).unwrap();
        terminal.snapshot.surface = Some(TerminalSurface::new(
            active_dimensions.cols,
            active_dimensions.rows,
        ));
        terminal.surface_revision = 2;
        terminal.pending_damage = Some(TerminalDamage::Full);

        let mut images = Assets::<Image>::default();
        let image = images.add(create_terminal_image(UVec2::ONE));

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image,
                texture_state: cached_background_state.clone(),
                desired_texture_state: cached_background_state,
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 1,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut world = World::default();
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(font_state);
        world.insert_resource(view_state);
        world.insert_resource(hud_state.layout_state());
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
        world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
        world.insert_resource(TerminalGlyphCache::default());
        world.insert_resource(renderer);
        world.insert_resource(images);
        world.spawn((window, PrimaryWindow));

        world.run_system_once(sync_terminal_texture).unwrap();

        let store = world.resource::<TerminalPresentationStore>();
        let presented = store.get(id).expect("missing presented terminal");
        assert_eq!(presented.texture_state, active_texture_state);
        assert_eq!(presented.desired_texture_state, active_texture_state);
        assert_eq!(presented.uploaded_revision, 2);
    }

    /// Verifies underline decoration paints visible pixels even when the cell carries only styling.
    #[test]
    fn sync_terminal_texture_draws_underline_for_styled_blank_cell() {
        let mut surface = TerminalSurface::new(2, 1);
        surface.set_cell(
            0,
            0,
            TerminalCell {
                content: TerminalCellContent::Empty,
                fg: egui::Color32::from_rgb(170, 220, 200),
                bg: DEFAULT_BG,
                style: TerminalCellStyle {
                    underline: TerminalUnderlineStyle::Single,
                    underline_color: Some(egui::Color32::from_rgb(32, 180, 140)),
                    ..Default::default()
                },
                width: 1,
                selected: false,
            },
        );

        let (image, texture_state) = render_surface_to_terminal_image(surface);
        let underline_band_y = texture_state
            .cell_size
            .y
            .saturating_sub((texture_state.cell_size.y / 4).max(1));
        let underline_pixels = count_non_background_pixels_in_rect(
            &image,
            0,
            underline_band_y,
            texture_state.cell_size.x,
            (texture_state.cell_size.y / 4).max(1),
        );
        assert!(
            underline_pixels > 0,
            "styled blank cell should paint underline pixels"
        );
    }

    /// Verifies strikeout decoration paints visible pixels even when the cell carries only styling.
    #[test]
    fn sync_terminal_texture_draws_strikeout_for_styled_blank_cell() {
        let mut surface = TerminalSurface::new(2, 1);
        surface.set_cell(
            0,
            0,
            TerminalCell {
                content: TerminalCellContent::Empty,
                fg: egui::Color32::from_rgb(210, 210, 210),
                bg: DEFAULT_BG,
                style: TerminalCellStyle {
                    strikeout: true,
                    ..Default::default()
                },
                width: 1,
                selected: false,
            },
        );

        let (image, texture_state) = render_surface_to_terminal_image(surface);
        let strike_y = texture_state.cell_size.y / 2;
        let strike_pixels = count_non_background_pixels_in_rect(
            &image,
            0,
            strike_y.saturating_sub(1),
            texture_state.cell_size.x,
            3,
        );
        assert!(
            strike_pixels > 0,
            "styled blank cell should paint strikeout pixels"
        );
    }

    /// Verifies selected-foreground luminance computation does not overflow on bright cells.
    #[test]
    fn selected_foreground_color_handles_bright_selection_without_overflow() {
        let cell = TerminalCell {
            content: TerminalCellContent::Single('A'),
            fg: egui::Color32::from_rgb(255, 255, 255),
            bg: egui::Color32::from_rgb(255, 255, 255),
            style: TerminalCellStyle::default(),
            width: 1,
            selected: false,
        };

        assert_eq!(selected_foreground_color(&cell, true), egui::Color32::BLACK);
    }

    /// Verifies dim styling darkens the visible glyph output compared with the same un-dimmed glyph.
    #[test]
    fn sync_terminal_texture_dims_foreground_ink() {
        let mut surface = TerminalSurface::new(2, 1);
        let fg = egui::Color32::from_rgb(220, 220, 220);
        surface.set_cell(
            0,
            0,
            TerminalCell {
                content: TerminalCellContent::Single('A'),
                fg,
                bg: DEFAULT_BG,
                style: TerminalCellStyle::default(),
                width: 1,
                selected: false,
            },
        );
        surface.set_cell(
            1,
            0,
            TerminalCell {
                content: TerminalCellContent::Single('A'),
                fg,
                bg: DEFAULT_BG,
                style: TerminalCellStyle {
                    dim: true,
                    ..Default::default()
                },
                width: 1,
                selected: false,
            },
        );

        let (image, texture_state) = render_surface_to_terminal_image(surface);
        let normal_sum = summed_non_background_rgb(
            &image,
            0,
            0,
            texture_state.cell_size.x,
            texture_state.cell_size.y,
        );
        let dim_sum = summed_non_background_rgb(
            &image,
            texture_state.cell_size.x,
            0,
            texture_state.cell_size.x,
            texture_state.cell_size.y,
        );
        assert!(
            dim_sum < normal_sum,
            "dim glyph should emit less visible ink than regular glyph"
        );
    }

    /// Verifies every non-empty character cell in the provided `pi` screenshot crop exactly.
    #[test]
    fn rendered_pi_screen_matches_reference_per_character_pixels() {
        let reference_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets/pi-screen-reference-20260328.ppm");
        let (width, height, reference) = read_binary_ppm(&reference_path);
        assert_eq!((width, height), (1378, 1064));

        let cell_size = UVec2::new(13, 28);
        let surface = surface_from_pi_screen_reference_ansi();
        let (image, texture_state) = render_surface_to_terminal_image_with_presentation_state(
            surface,
            TerminalTextureState {
                texture_size: UVec2::new(width, height),
                cell_size,
            },
        );
        assert_eq!(texture_state.cell_size, cell_size);
        let actual = crop_image_rgb(&image, 0, 0, width, height);
        let reference_bg = [reference[0], reference[1], reference[2]];

        let mut ppm = Vec::with_capacity(actual.len() + 64);
        ppm.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
        ppm.extend_from_slice(&actual);
        fs::write("/tmp/neozeus-pi-screen-actual.ppm", ppm).expect("actual ppm should write");

        for row in 0..(height / cell_size.y) {
            for col in 0..(width / cell_size.x) {
                let x = col * cell_size.x;
                let y = row * cell_size.y;
                let expected = crop_rgb_rows(&reference, width, x, y, cell_size.x, cell_size.y);
                if expected
                    .chunks_exact(3)
                    .all(|pixel| [pixel[0], pixel[1], pixel[2]] == reference_bg)
                {
                    continue;
                }
                let actual_cell = crop_rgb_rows(&actual, width, x, y, cell_size.x, cell_size.y);
                assert_eq!(
                    actual_cell, expected,
                    "pixel mismatch for screenshot cell row={row} col={col}"
                );
            }
        }
    }

    /// Verifies that texture sync paints visible glyph pixels on the last terminal row, which is a
    /// common active-input case.
    #[test]
    fn sync_terminal_texture_renders_visible_text_on_last_row() {
        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

        let mut surface = TerminalSurface::new(active_layout.0.cols, active_layout.0.rows);
        set_colored_text(
            &mut surface,
            active_layout.0.rows - 1,
            0,
            "typed text",
            egui::Color32::from_rgb(220, 220, 220),
        );

        let (image, texture_state) = render_surface_to_terminal_image(surface);
        let y_start = (active_layout.0.rows as u32 - 1) * texture_state.cell_size.y;
        let visible_pixels =
            count_non_background_pixels_in_band(&image, y_start, y_start + texture_state.cell_size.y);
        assert!(
            visible_pixels > 0,
            "last terminal row rendered no visible text pixels"
        );
    }

    /// Verifies that incremental rasterization of a scrolled viewport matches a fresh full render.
    #[test]
    fn incremental_scroll_render_matches_fresh_render() {
        let mut before = TerminalSurface::new(4, 3);
        set_colored_text(
            &mut before,
            0,
            0,
            "4",
            egui::Color32::from_rgb(220, 220, 220),
        );
        set_colored_text(
            &mut before,
            1,
            0,
            "5",
            egui::Color32::from_rgb(220, 220, 220),
        );
        set_colored_text(
            &mut before,
            2,
            0,
            "6",
            egui::Color32::from_rgb(220, 220, 220),
        );

        let mut after = TerminalSurface::new(4, 3);
        set_colored_text(
            &mut after,
            0,
            0,
            "3",
            egui::Color32::from_rgb(220, 220, 220),
        );
        set_colored_text(
            &mut after,
            1,
            0,
            "4",
            egui::Color32::from_rgb(220, 220, 220),
        );
        set_colored_text(
            &mut after,
            2,
            0,
            "5",
            egui::Color32::from_rgb(220, 220, 220),
        );
        after.display_offset = 1;

        let report = configured_terminal_font_report();
        let mut renderer = TerminalTextRenderer::default();
        initialize_test_terminal_text_renderer(&report, &mut renderer);
        let font_state = configured_test_font_state(report, &mut renderer);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let view_state = TerminalViewState::default();

        let bridge = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);
        let terminal = manager.get_mut(id).expect("terminal should exist");
        terminal.snapshot.surface = Some(before.clone());
        terminal.surface_revision = 1;
        terminal.pending_damage = Some(TerminalDamage::Full);

        let mut images = Assets::<Image>::default();
        let image = images.add(create_terminal_image(UVec2::ONE));
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: image.clone(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut world = World::default();
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(font_state);
        world.insert_resource(view_state);
        world.insert_resource(hud_state.layout_state());
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
        world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
        world.insert_resource(TerminalGlyphCache::default());
        world.insert_resource(renderer);
        world.insert_resource(images);
        world.spawn((window, PrimaryWindow));

        world.run_system_once(sync_terminal_texture).unwrap();
        let first_texture_state = world
            .resource::<TerminalPresentationStore>()
            .get(id)
            .expect("presented terminal should exist")
            .texture_state
            .clone();

        {
            let mut manager = world.resource_mut::<TerminalManager>();
            let terminal = manager.get_mut(id).expect("terminal should exist");
            terminal.snapshot.surface = Some(after.clone());
            terminal.surface_revision = 2;
            terminal.pending_damage = Some(TerminalDamage::Full);
        }
        world.run_system_once(sync_terminal_texture).unwrap();

        let incremental_image = {
            let store = world.resource::<TerminalPresentationStore>();
            let presented = store.get(id).expect("presented terminal should exist");
            world
                .resource::<Assets<Image>>()
                .get(&presented.image)
                .expect("rendered image should exist")
                .clone()
        };
        let (fresh_image, _) =
            render_surface_to_terminal_image_with_presentation_state(after, first_texture_state);

        let incremental = incremental_image
            .data
            .as_ref()
            .expect("incremental image data should exist");
        let fresh = fresh_image
            .data
            .as_ref()
            .expect("fresh image data should exist");
        assert_eq!(
            incremental, fresh,
            "incremental scroll render diverged from fresh render"
        );
    }

    #[test]
    fn incremental_scroll_render_with_cursor_matches_fresh_render() {
        let mut before = TerminalSurface::new(4, 3);
        set_colored_text(
            &mut before,
            0,
            0,
            "4",
            egui::Color32::from_rgb(220, 220, 220),
        );
        set_colored_text(
            &mut before,
            1,
            0,
            "5",
            egui::Color32::from_rgb(220, 220, 220),
        );
        set_colored_text(
            &mut before,
            2,
            0,
            "6",
            egui::Color32::from_rgb(220, 220, 220),
        );
        before.cursor = Some(TerminalCursor {
            x: 0,
            y: 1,
            shape: TerminalCursorShape::Block,
            visible: true,
            color: egui::Color32::WHITE,
        });

        let mut after = TerminalSurface::new(4, 3);
        set_colored_text(
            &mut after,
            0,
            0,
            "3",
            egui::Color32::from_rgb(220, 220, 220),
        );
        set_colored_text(
            &mut after,
            1,
            0,
            "4",
            egui::Color32::from_rgb(220, 220, 220),
        );
        set_colored_text(
            &mut after,
            2,
            0,
            "5",
            egui::Color32::from_rgb(220, 220, 220),
        );
        after.display_offset = 1;
        after.cursor = Some(TerminalCursor {
            x: 0,
            y: 2,
            shape: TerminalCursorShape::Block,
            visible: true,
            color: egui::Color32::WHITE,
        });

        let report = configured_terminal_font_report();
        let mut renderer = TerminalTextRenderer::default();
        initialize_test_terminal_text_renderer(&report, &mut renderer);
        let font_state = configured_test_font_state(report, &mut renderer);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let view_state = TerminalViewState::default();

        let bridge = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);
        let terminal = manager.get_mut(id).expect("terminal should exist");
        terminal.snapshot.surface = Some(before.clone());
        terminal.surface_revision = 1;
        terminal.pending_damage = Some(TerminalDamage::Full);

        let mut images = Assets::<Image>::default();
        let image = images.add(create_terminal_image(UVec2::ONE));
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: image.clone(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut world = World::default();
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(font_state);
        world.insert_resource(view_state);
        world.insert_resource(hud_state.layout_state());
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        world.insert_resource(crate::text_selection::TerminalTextSelectionState::default());
        world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
        world.insert_resource(TerminalGlyphCache::default());
        world.insert_resource(renderer);
        world.insert_resource(images);
        world.spawn((window, PrimaryWindow));

        world.run_system_once(sync_terminal_texture).unwrap();
        let first_texture_state = world
            .resource::<TerminalPresentationStore>()
            .get(id)
            .expect("presented terminal should exist")
            .texture_state
            .clone();

        {
            let mut manager = world.resource_mut::<TerminalManager>();
            let terminal = manager.get_mut(id).expect("terminal should exist");
            terminal.snapshot.surface = Some(after.clone());
            terminal.surface_revision = 2;
            terminal.pending_damage = Some(TerminalDamage::Full);
        }
        world.run_system_once(sync_terminal_texture).unwrap();

        let incremental_image = {
            let store = world.resource::<TerminalPresentationStore>();
            let presented = store.get(id).expect("presented terminal should exist");
            world
                .resource::<Assets<Image>>()
                .get(&presented.image)
                .expect("rendered image should exist")
                .clone()
        };
        let (fresh_image, _) =
            render_surface_to_terminal_image_with_presentation_state(after, first_texture_state);
        assert_eq!(
            incremental_image.data.as_ref().unwrap(),
            fresh_image.data.as_ref().unwrap(),
            "incremental scroll render with cursor diverged from fresh render"
        );
    }

    #[test]
    fn sync_terminal_texture_updates_pixels_when_last_row_text_changes() {
        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

        let mut before = TerminalSurface::new(active_layout.0.cols, active_layout.0.rows);
        set_colored_text(
            &mut before,
            active_layout.0.rows - 1,
            0,
            "$ ",
            egui::Color32::from_rgb(220, 220, 220),
        );
        let (before_image, texture_state) = render_surface_to_terminal_image(before);

        let mut after = TerminalSurface::new(active_layout.0.cols, active_layout.0.rows);
        set_colored_text(
            &mut after,
            active_layout.0.rows - 1,
            0,
            "$ abc",
            egui::Color32::from_rgb(220, 220, 220),
        );
        let (after_image, _) = render_surface_to_terminal_image(after);

        let before_data = before_image
            .data
            .as_ref()
            .expect("before image data should exist");
        let after_data = after_image
            .data
            .as_ref()
            .expect("after image data should exist");
        assert_ne!(
            before_data, after_data,
            "last-row text change should alter texture pixels"
        );

        let y_start = (active_layout.0.rows as u32 - 1) * texture_state.cell_size.y;
        let before_pixels = count_non_background_pixels_in_band(
            &before_image,
            y_start,
            y_start + texture_state.cell_size.y,
        );
        let after_pixels = count_non_background_pixels_in_band(
            &after_image,
            y_start,
            y_start + texture_state.cell_size.y,
        );
        assert!(
            after_pixels > before_pixels,
            "longer prompt line should draw more pixels"
        );
    }
}
