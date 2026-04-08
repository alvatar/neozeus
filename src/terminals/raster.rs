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
    prelude::{Assets, DetectChanges, Image, Res, ResMut, Resource, Single, UVec2, Window, With},
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
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    if text_renderer.font_system.is_none() {
        append_debug_log("texture sync: no font system");
        return;
    }

    if font_state.is_changed() {
        append_debug_log("texture sync: font state changed, clearing glyph cache");
        glyph_cache.glyphs.clear();
    }

    #[allow(unused_mut)]
    let mut active_id = focus_state.active_id();
    #[cfg(test)]
    {
        active_id = terminal_manager
            .clone_focus_state()
            .active_id()
            .or(active_id);
    }
    let active_layout = active_id.map(|_| {
        active_terminal_layout_for_dimensions(
            &primary_window,
            &layout_state,
            &view_state,
            target_active_terminal_dimensions(&primary_window, &layout_state, &font_state),
            &font_state,
        )
    });
    for (terminal_id, terminal) in terminal_manager.iter_mut() {
        let override_surface = (Some(terminal_id) == active_id)
            .then_some(active_terminal_content.owned_tmux_surface_for(terminal_id))
            .flatten()
            .or_else(|| {
                verification_overrides
                    .as_ref()
                    .and_then(|overrides| overrides.surface_for(terminal_id))
            });
        let Some(surface) = override_surface.or(terminal.snapshot.surface.as_ref()) else {
            terminal.pending_damage = None;
            continue;
        };
        let Some(presented_terminal) = presentation_store.get_mut(terminal_id) else {
            terminal.pending_damage = None;
            continue;
        };

        let upload_state = if Some(terminal_id) == active_id {
            let active_layout = active_layout.expect("active layout missing for active terminal");
            let active_target_state = TerminalTextureState {
                texture_size: active_layout.texture_size,
                cell_size: active_layout.cell_size,
            };
            if can_render_active_layout(surface, active_layout.dimensions) {
                presented_terminal.desired_texture_state = active_target_state.clone();
                active_target_state
            } else {
                let cached =
                    cached_or_default_texture_state(presented_terminal, surface, &font_state);
                presented_terminal.desired_texture_state = cached.clone();
                cached
            }
        } else {
            let cached = if active_id.is_some() {
                default_texture_state_for_surface(surface, &font_state)
            } else {
                cached_or_default_texture_state(presented_terminal, surface, &font_state)
            };
            presented_terminal.desired_texture_state = cached.clone();
            cached
        };

        let active_override_revision = active_terminal_content
            .presentation_override_revision_for(terminal_id)
            .or_else(|| {
                verification_overrides
                    .as_ref()
                    .and_then(|overrides| overrides.presentation_override_revision_for(terminal_id))
            });
        let terminal_selection_revision =
            terminal_text_selection.presentation_revision_for(terminal_id);
        let has_pending_surface = terminal.surface_revision != presented_terminal.uploaded_revision
            || presented_terminal.uploaded_active_override_revision != active_override_revision
            || presented_terminal.uploaded_text_selection_revision != terminal_selection_revision;
        let mut full_redraw =
            font_state.is_changed() || presented_terminal.texture_state != upload_state;
        let mut dirty_rows = if full_redraw {
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

        if dirty_rows.is_empty() {
            continue;
        }

        if images.get_mut(&presented_terminal.image).is_none() {
            presented_terminal.image = images.add(create_terminal_image(upload_state.texture_size));
            full_redraw = true;
            dirty_rows = (0..surface.rows).collect();
        }

        if let Some(target_image) = images.get_mut(&presented_terminal.image) {
            if target_image.texture_descriptor.size.width != upload_state.texture_size.x
                || target_image.texture_descriptor.size.height != upload_state.texture_size.y
            {
                *target_image = create_terminal_image(upload_state.texture_size);
                full_redraw = true;
                dirty_rows = (0..surface.rows).collect();
            }

            let expected_len =
                (upload_state.texture_size.x * upload_state.texture_size.y * 4) as usize;
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
                full_redraw = true;
                dirty_rows = (0..surface.rows).collect();
            }

            if full_redraw {
                clear_terminal_pixels(pixels);
            }

            let compose_started = std::time::Instant::now();
            repaint_terminal_pixels(
                pixels,
                upload_state.texture_size.x,
                surface,
                terminal_text_selection.override_selection_for(terminal_id),
                &dirty_rows,
                upload_state.cell_size,
                &mut text_renderer,
                &mut glyph_cache,
                &font_state,
            );
            let compose_elapsed = compose_started.elapsed();
            terminal
                .bridge
                .note_compose(dirty_rows.len(), compose_elapsed.as_micros() as u64);

            if env::var_os("NEOZEUS_DUMP_TEXTURE").is_some() {
                let dump_path = resolve_debug_texture_dump_path();
                let _ = dump_terminal_image_ppm(target_image, dump_path.as_path());
            }

            presented_terminal.texture_state = upload_state;
            presented_terminal.uploaded_revision = terminal.surface_revision;
            presented_terminal.uploaded_active_override_revision = active_override_revision;
            presented_terminal.uploaded_text_selection_revision = terminal_selection_revision;
            terminal.pending_damage = None;
        } else {
            append_debug_log("texture sync: target image missing in assets");
        }
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
mod tests;
