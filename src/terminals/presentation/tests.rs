use super::*;

/// Returns the active terminal cell size.
pub(crate) fn active_terminal_cell_size(
    _window: &Window,
    _view_state: &TerminalViewState,
) -> UVec2 {
    fixed_terminal_cell_size(&TerminalFontState::default())
}

/// Returns the active terminal dimensions.
pub(crate) fn active_terminal_dimensions(
    window: &Window,
    layout_state: &HudLayoutState,
    _view_state: &TerminalViewState,
    font_state: &TerminalFontState,
) -> TerminalDimensions {
    target_active_terminal_dimensions(window, layout_state, font_state)
}

/// Returns the active terminal layout.
pub(crate) fn active_terminal_layout(
    window: &Window,
    layout_state: &HudLayoutState,
    view_state: &TerminalViewState,
    font_state: &TerminalFontState,
) -> ActiveTerminalLayout {
    active_terminal_layout_for_dimensions(
        window,
        layout_state,
        view_state,
        target_active_terminal_dimensions(window, layout_state, font_state),
        font_state,
    )
}

/// Computes the raster cell size chosen for pixel-perfect scaling of a fixed terminal geometry.
pub(crate) fn pixel_perfect_cell_size(
    cols: usize,
    rows: usize,
    window: &Window,
    layout_state: &HudLayoutState,
    font_state: &TerminalFontState,
) -> UVec2 {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let base_w = font_state.cell_metrics.cell_width;
    let base_h = font_state.cell_metrics.cell_height;
    let base_texture_width = (cols as u32).max(1) as f32 * base_w as f32;
    let base_texture_height = (rows as u32).max(1) as f32 * base_h as f32;
    let (fit_size_logical, _) = active_terminal_fit_area(window, layout_state);
    let fit_size_physical = logical_to_physical_size(fit_size_logical, window);
    let raster_scale = (fit_size_physical.x / base_texture_width)
        .min(fit_size_physical.y / base_texture_height)
        .max(1.0 / base_h as f32);

    UVec2::new(
        (base_w as f32 * raster_scale).floor().max(1.0) as u32,
        (base_h as f32 * raster_scale).floor().max(1.0) as u32,
    )
}

/// Snaps a logical position onto the physical pixel grid implied by the window scale factor.
///
/// Pixel-perfect terminal presentation uses this to avoid subpixel blur.
pub(crate) fn snap_to_pixel_grid(position: Vec2, window: &Window) -> Vec2 {
    let scale_factor = window_scale_factor(window);
    (position * scale_factor).round() / scale_factor
}

/// Exposes the logical on-screen size implied by a pixel-perfect texture state.
pub(crate) fn pixel_perfect_terminal_logical_size(
    texture_state: &TerminalTextureState,
    window: &Window,
) -> Vec2 {
    terminal_logical_size(texture_state, window)
}
