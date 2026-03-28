use crate::hud::{HudLayoutState, HudWidgetKey, TerminalVisibilityPolicy, TerminalVisibilityState};

use super::{
    fonts::{TerminalCellMetrics, TerminalFontState},
    presentation_state::{
        PresentedTerminal, TerminalDisplayMode, TerminalHudSurfaceMarker, TerminalPanel,
        TerminalPanelFrame, TerminalPanelSprite, TerminalPresentation, TerminalPresentationStore,
        TerminalTextureState, TerminalViewState,
    },
    raster::create_terminal_image,
    registry::{ManagedTerminal, TerminalFocusState, TerminalId, TerminalManager},
    runtime::TerminalRuntimeSpawner,
    types::{TerminalDimensions, TerminalLifecycle, TerminalSurface},
};
use bevy::{prelude::*, window::PrimaryWindow};

const HUD_FRAME_PADDING: Vec2 = Vec2::ZERO;
const ACTIVE_TERMINAL_MARGIN: Vec2 = Vec2::splat(16.0);
pub(crate) const DIRECT_INPUT_FRAME_OUTSET: f32 = 6.0;
const INACTIVE_RUNTIME_FRAME_OUTSET: f32 = 4.0;
const STARTUP_PLACEHOLDER_COLS: u32 = 120;
const STARTUP_PLACEHOLDER_ROWS: u32 = 38;
const STARTUP_PLACEHOLDER_COLOR: Color = Color::srgb(0.10, 0.13, 0.18);
const STARTUP_PLACEHOLDER_ACTIVE_COLOR: Color = Color::srgb(0.16, 0.18, 0.22);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveTerminalLayout {
    pub(crate) cell_size: UVec2,
    pub(crate) dimensions: TerminalDimensions,
    pub(crate) texture_size: UVec2,
}

/// Returns the default grid position used for a terminal's background/home slot.
///
/// The layout is a simple 3-column staging grid used before a terminal becomes the active focused
/// presentation.
fn terminal_home_position(slot: usize) -> Vec2 {
    const COLUMNS: usize = 3;
    const STEP_X: f32 = 360.0;
    const STEP_Y: f32 = 220.0;
    let column = slot % COLUMNS;
    let row = slot / COLUMNS;
    Vec2::new(-360.0 + column as f32 * STEP_X, 120.0 - row as f32 * STEP_Y)
}

/// Spawns the Bevy entities and retained presentation record for a newly created terminal.
///
/// Each terminal gets both a panel sprite and a frame sprite, plus an initial placeholder image in the
/// presentation store.
pub(crate) fn spawn_terminal_presentation(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    presentation_store: &mut TerminalPresentationStore,
    id: TerminalId,
    slot: usize,
) {
    // Walk the lifecycle in explicit stages so each side effect happens only after its prerequisites have been established.
    let home_position = terminal_home_position(slot);
    let presentation = TerminalPresentation {
        home_position,
        current_position: home_position,
        target_position: home_position,
        current_size: Vec2::ONE,
        target_size: Vec2::ONE,
        current_alpha: 0.82,
        target_alpha: 0.82,
        current_z: -0.05,
        target_z: -0.05,
    };

    let image_handle = images.add(create_terminal_image(UVec2::ONE));
    let frame_entity = commands
        .spawn((
            Sprite {
                color: Color::srgba(0.08, 0.08, 0.09, 0.94),
                custom_size: Some(Vec2::ONE),
                ..default()
            },
            Transform::from_xyz(
                home_position.x,
                home_position.y,
                presentation.current_z - 0.01,
            ),
            TerminalPanelFrame { id },
        ))
        .id();
    let panel_entity = commands
        .spawn((
            Sprite::from_image(image_handle.clone()),
            Transform::from_xyz(home_position.x, home_position.y, presentation.current_z),
            TerminalPanelSprite,
            TerminalPanel { id },
            presentation,
        ))
        .id();

    let fallback = TerminalCellMetrics::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: image_handle,
            texture_state: TerminalTextureState {
                texture_size: UVec2::ONE,
                cell_size: UVec2::new(fallback.cell_width, fallback.cell_height),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: UVec2::ONE,
                cell_size: UVec2::new(fallback.cell_width, fallback.cell_height),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity,
            frame_entity,
        },
    );
}

/// Reconciles terminal presentation entities against the authoritative terminal registry.
///
/// Terminal creation/removal mutates only the terminal manager. This projection sync owns the Bevy
/// panel/frame entities and the retained presentation-store entries, creating missing projections and
/// removing stale ones.
pub(crate) fn sync_terminal_projection_entities(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    terminal_manager: Res<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    for terminal_id in presentation_store.terminal_ids() {
        if terminal_manager.contains_terminal(terminal_id) {
            continue;
        }
        if let Some(presented) = presentation_store.remove(terminal_id) {
            commands.entity(presented.panel_entity).despawn();
            commands.entity(presented.frame_entity).despawn();
        }
    }

    for (slot, terminal_id) in terminal_manager.terminal_ids().iter().copied().enumerate() {
        if presentation_store.get(terminal_id).is_some() {
            continue;
        }
        spawn_terminal_presentation(
            &mut commands,
            &mut images,
            &mut presentation_store,
            terminal_id,
            slot,
        );
    }
}

/// Returns a non-zero window scale factor for logical/physical conversions.
///
/// `f32::EPSILON` is used as a defensive floor to avoid division by zero in broken environments.
fn window_scale_factor(window: &Window) -> f32 {
    window.scale_factor().max(f32::EPSILON)
}

/// Converts a logical-size vector into physical pixels using the window scale factor.
fn logical_to_physical_size(size: Vec2, window: &Window) -> Vec2 {
    size * window_scale_factor(window)
}

/// Converts a physical-pixel size vector back into logical window units.
fn physical_to_logical_size(size: Vec2, window: &Window) -> Vec2 {
    size / window_scale_factor(window)
}

/// Returns the logical viewport size/center available for the active terminal after reserving HUD
/// chrome.
///
/// The docked agent list claims space on the left when enabled.
pub(crate) fn active_terminal_viewport(
    window: &Window,
    layout_state: &HudLayoutState,
) -> (Vec2, Vec2) {
    let reserved_left = layout_state
        .get(HudWidgetKey::AgentList)
        .filter(|module| module.shell.enabled)
        .map(|module| module.shell.current_rect.w)
        .unwrap_or(0.0)
        .clamp(0.0, window.width());
    let usable_size = Vec2::new((window.width() - reserved_left).max(64.0), window.height());
    let center = Vec2::new(reserved_left * 0.5, 0.0);
    (usable_size, center)
}

/// Shrinks the active-terminal viewport by the fixed outer margin used for focused presentation.
fn active_terminal_fit_area(window: &Window, layout_state: &HudLayoutState) -> (Vec2, Vec2) {
    let (viewport_size, viewport_center) = active_terminal_viewport(window, layout_state);
    let fit_size = Vec2::new(
        (viewport_size.x - ACTIVE_TERMINAL_MARGIN.x * 2.0).max(64.0),
        (viewport_size.y - ACTIVE_TERMINAL_MARGIN.y * 2.0).max(64.0),
    );
    (fit_size, viewport_center)
}

/// Converts the shared view distance into a scalar zoom factor.
fn terminal_zoom_scale(view_state: &TerminalViewState) -> f32 {
    10.0 / view_state.distance.max(0.1)
}

/// Returns the fixed terminal cell size.
fn fixed_terminal_cell_size(font_state: &TerminalFontState) -> UVec2 {
    UVec2::new(
        font_state.cell_metrics.cell_width.max(1),
        font_state.cell_metrics.cell_height.max(1),
    )
}

/// Returns the target active terminal dimensions.
pub(crate) fn target_active_terminal_dimensions(
    window: &Window,
    layout_state: &HudLayoutState,
    font_state: &TerminalFontState,
) -> TerminalDimensions {
    let cell_size = fixed_terminal_cell_size(font_state);
    let (fit_size_logical, _) = active_terminal_fit_area(window, layout_state);
    let fit_size_physical = logical_to_physical_size(fit_size_logical, window);
    TerminalDimensions {
        cols: ((fit_size_physical.x.floor() as u32) / cell_size.x.max(1)).max(1) as usize,
        rows: ((fit_size_physical.y.floor() as u32) / cell_size.y.max(1)).max(1) as usize,
    }
}

/// Returns the active terminal layout for dimensions.
pub(crate) fn active_terminal_layout_for_dimensions(
    _window: &Window,
    _layout_state: &HudLayoutState,
    _view_state: &TerminalViewState,
    dimensions: TerminalDimensions,
    font_state: &TerminalFontState,
) -> ActiveTerminalLayout {
    let cell_size = fixed_terminal_cell_size(font_state);
    ActiveTerminalLayout {
        cell_size,
        dimensions,
        texture_size: UVec2::new(
            dimensions.cols as u32 * cell_size.x,
            dimensions.rows as u32 * cell_size.y,
        ),
    }
}

/// Converts an `ActiveTerminalLayout` into the texture-state record used by the presentation store.
fn active_layout_texture_state(layout: ActiveTerminalLayout) -> TerminalTextureState {
    TerminalTextureState {
        texture_size: layout.texture_size,
        cell_size: layout.cell_size,
    }
}

/// Chooses a temporary texture-state contract while a startup-loading terminal has not yet produced a
/// presentable uploaded frame.
///
/// Existing desired/current texture state wins when available; otherwise the helper falls back to
/// either the known surface size or the fixed startup placeholder dimensions.
fn startup_placeholder_texture_state(
    surface: Option<&TerminalSurface>,
    presented_terminal: &PresentedTerminal,
) -> TerminalTextureState {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if presented_terminal.desired_texture_state.texture_size != UVec2::ZERO
        && presented_terminal.desired_texture_state.texture_size != UVec2::ONE
        && presented_terminal.desired_texture_state.cell_size != UVec2::ZERO
    {
        return presented_terminal.desired_texture_state.clone();
    }
    if presented_terminal.texture_state.texture_size != UVec2::ZERO
        && presented_terminal.texture_state.texture_size != UVec2::ONE
        && presented_terminal.texture_state.cell_size != UVec2::ZERO
    {
        return presented_terminal.texture_state.clone();
    }
    let fallback = TerminalCellMetrics::default();
    let (cols, rows) = surface
        .map(|surface| (surface.cols as u32, surface.rows as u32))
        .unwrap_or((STARTUP_PLACEHOLDER_COLS, STARTUP_PLACEHOLDER_ROWS));
    TerminalTextureState {
        texture_size: UVec2::new(
            cols.max(1) * fallback.cell_width,
            rows.max(1) * fallback.cell_height,
        ),
        cell_size: UVec2::new(fallback.cell_width, fallback.cell_height),
    }
}

/// Returns whether the active terminal already has the exact surface and uploaded texture state the
/// focused layout currently expects.
fn active_terminal_ready_for_presentation(
    terminal: &ManagedTerminal,
    presented_terminal: &PresentedTerminal,
    layout: ActiveTerminalLayout,
) -> bool {
    let Some(surface) = terminal.snapshot.surface.as_ref() else {
        return false;
    };
    surface.cols == layout.dimensions.cols
        && surface.rows == layout.dimensions.rows
        && presented_terminal.texture_state == active_layout_texture_state(layout)
        && presented_terminal.uploaded_revision == terminal.surface_revision
}

/// Returns whether a terminal has any uploaded frame that is coherent enough to be shown, even if it
/// does not yet match the latest active-layout contract exactly.
fn terminal_has_presentable_uploaded_frame(
    terminal: &ManagedTerminal,
    presented_terminal: &PresentedTerminal,
) -> bool {
    terminal.snapshot.surface.is_some()
        && presented_terminal.uploaded_revision == terminal.surface_revision
        && presented_terminal.texture_state.texture_size != UVec2::ONE
        && presented_terminal.texture_state.cell_size != UVec2::ZERO
}

#[allow(
    clippy::too_many_arguments,
    reason = "active-terminal resize policy needs terminal, font, runtime, HUD, window, and local debounce state"
)]
/// Resizes the active PTY grid to the deterministic terminal dimensions implied by the available
/// HUD viewport and the fixed measured cell size.
///
/// Character size is locked by the measured font metrics; the remaining space decides cols/rows.
/// Zoom does not participate in this policy.
pub(crate) fn sync_active_terminal_dimensions(
    terminal_manager: ResMut<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    font_state: Res<TerminalFontState>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    _view_state: Res<TerminalViewState>,
    layout_state: Res<HudLayoutState>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut pending_resize: Local<Option<(TerminalId, TerminalDimensions)>>,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let Some(active_id) = focus_state.active_id() else {
        *pending_resize = None;
        return;
    };
    let Some(terminal) = terminal_manager.get(active_id) else {
        *pending_resize = None;
        return;
    };
    if !terminal.snapshot.runtime.is_interactive() {
        *pending_resize = None;
        return;
    }

    let target = target_active_terminal_dimensions(&primary_window, &layout_state, &font_state);
    let current = terminal
        .snapshot
        .surface
        .as_ref()
        .map(|surface| TerminalDimensions {
            cols: surface.cols,
            rows: surface.rows,
        });
    if current == Some(target) {
        *pending_resize = None;
        return;
    }
    if *pending_resize == Some((active_id, target)) {
        return;
    }
    if runtime_spawner
        .resize_session(&terminal.session_name, target.cols, target.rows)
        .is_ok()
    {
        *pending_resize = Some((active_id, target));
    }
}

/// Converts a texture-state's physical texture size into logical on-screen size.
fn terminal_logical_size(texture_state: &TerminalTextureState, window: &Window) -> Vec2 {
    physical_to_logical_size(
        Vec2::new(
            texture_state.texture_size.x.max(1) as f32,
            texture_state.texture_size.y.max(1) as f32,
        ),
        window,
    )
}

/// Computes the smooth-mode on-screen size for a terminal texture, combining fit-to-viewport scaling
/// with the shared zoom factor.
fn smooth_terminal_screen_size(
    texture_state: &TerminalTextureState,
    view_state: &TerminalViewState,
    window: &Window,
    layout_state: &HudLayoutState,
) -> Vec2 {
    let texture_width = texture_state.texture_size.x.max(1) as f32;
    let texture_height = texture_state.texture_size.y.max(1) as f32;
    let (fit_size, _) = active_terminal_fit_area(window, layout_state);
    let fit_scale = (fit_size.x / texture_width).min(fit_size.y / texture_height);
    let zoom_scale = terminal_zoom_scale(view_state);
    Vec2::new(texture_width, texture_height) * fit_scale * zoom_scale
}

/// Snaps axis for texture center.
fn snap_axis_for_texture_center(center: f32, physical_size: u32, window: &Window) -> f32 {
    let scale_factor = window_scale_factor(window);
    let center_physical = center * scale_factor;
    let snapped = if physical_size.is_multiple_of(2) {
        center_physical.round()
    } else {
        (center_physical - 0.5).round() + 0.5
    };
    snapped / scale_factor
}

/// Returns the snapped center position the active terminal should occupy inside the HUD viewport.
///
/// Snapping depends on the final physical texture size: even-sized textures center on whole pixels,
/// odd-sized textures center on half-pixels so both edges still land on pixel boundaries.
pub(crate) fn hud_terminal_target_position(
    window: &Window,
    layout_state: &HudLayoutState,
    texture_state: &TerminalTextureState,
) -> Vec2 {
    let (_, center) = active_terminal_viewport(window, layout_state);
    Vec2::new(
        snap_axis_for_texture_center(center.x, texture_state.texture_size.x.max(1), window),
        snap_axis_for_texture_center(center.y, texture_state.texture_size.y.max(1), window),
    )
}

/// Expands a terminal panel's size into the matching HUD-surface backing size.
fn hud_surface_size(terminal_size: Vec2) -> Vec2 {
    terminal_size + HUD_FRAME_PADDING * 2.0
}

/// Returns the logical on-screen size currently used for a terminal texture.
///
/// Today both smooth and pixel-perfect presentation paths expose the same logical size helper here.
pub(crate) fn terminal_texture_screen_size(
    texture_state: &TerminalTextureState,
    _view_state: &TerminalViewState,
    window: &Window,
    _layout_state: &HudLayoutState,
    _pixel_perfect: bool,
) -> Vec2 {
    terminal_logical_size(texture_state, window)
}

/// Returns the visibility policy that should actually be applied after checking whether the isolated
/// terminal still exists.
///
/// Stale isolate targets fall back to `ShowAll`.
fn effective_visibility_policy(
    terminal_manager: &TerminalManager,
    visibility_state: &TerminalVisibilityState,
) -> TerminalVisibilityPolicy {
    match visibility_state.policy {
        TerminalVisibilityPolicy::Isolate(id) if terminal_manager.get(id).is_none() => {
            TerminalVisibilityPolicy::ShowAll
        }
        policy => policy,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "presentation sync needs terminal/presentation/view state together"
)]
/// Recomputes each terminal panel's target size/position/visibility from focus, startup-loading,
/// uploaded-frame readiness, and display-mode state.
///
/// The system also decides when presentation state should snap immediately versus animate toward new
/// targets.
pub(crate) fn sync_terminal_presentations(
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    font_state: Option<Res<TerminalFontState>>,
    presentation_store: Res<TerminalPresentationStore>,
    mut startup_loading: Option<ResMut<crate::startup::StartupLoadingState>>,
    visibility_state: Res<TerminalVisibilityState>,
    view_state: Res<TerminalViewState>,
    layout_state: Res<HudLayoutState>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut last_active_id: Local<Option<TerminalId>>,
    mut last_visibility_policy: Local<Option<TerminalVisibilityPolicy>>,
    mut last_active_texture_state: Local<Option<TerminalTextureState>>,
    mut last_active_ready: Local<bool>,
    mut panels: Query<(
        &TerminalPanel,
        &mut TerminalPresentation,
        &mut Transform,
        &mut Sprite,
        &mut Visibility,
    )>,
) {
    let active_id = focus_state.active_id();
    let startup_show_all = startup_loading
        .as_ref()
        .is_some_and(|startup_loading| startup_loading.active());
    let visibility_policy = if startup_show_all {
        TerminalVisibilityPolicy::ShowAll
    } else {
        effective_visibility_policy(&terminal_manager, &visibility_state)
    };
    let default_font_state = TerminalFontState::default();
    let font_state = font_state.as_deref().unwrap_or(&default_font_state);
    let placeholder_dimensions = TerminalDimensions {
        cols: STARTUP_PLACEHOLDER_COLS as usize,
        rows: STARTUP_PLACEHOLDER_ROWS as usize,
    };
    let active_layout = active_id
        .map(|_| {
            active_terminal_layout_for_dimensions(
                &primary_window,
                &layout_state,
                &view_state,
                target_active_terminal_dimensions(&primary_window, &layout_state, font_state),
                font_state,
            )
        })
        .unwrap_or_else(|| {
            active_terminal_layout_for_dimensions(
                &primary_window,
                &layout_state,
                &view_state,
                placeholder_dimensions,
                font_state,
            )
        });
    let active_texture_state = active_layout_texture_state(active_layout);
    let active_ready = active_id
        .and_then(|id| {
            let terminal = terminal_manager.get(id)?;
            let presented_terminal = presentation_store.get(id)?;
            Some(active_terminal_ready_for_presentation(
                terminal,
                presented_terminal,
                active_layout,
            ))
        })
        .unwrap_or(false);
    // These locals are explicit transition gates rather than hidden domain state: when the
    // active terminal, visibility policy, or active texture contract changes we snap immediately
    // instead of animating through an invalid intermediate presentation.
    let snap_switch = *last_active_id != active_id
        || *last_visibility_policy != Some(visibility_policy)
        || *last_active_texture_state != active_id.map(|_| active_texture_state.clone())
        || *last_active_ready != active_ready;
    let active_size = physical_to_logical_size(
        Vec2::new(
            active_texture_state.texture_size.x.max(1) as f32,
            active_texture_state.texture_size.y.max(1) as f32,
        ),
        &primary_window,
    );
    let blend = 1.0 - (-time.delta_secs() * 10.0).exp();

    for (panel, mut presentation, mut transform, mut sprite, mut visibility) in &mut panels {
        let Some(terminal) = terminal_manager.get(panel.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let Some(presented_terminal) = presentation_store.get(panel.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let startup_placeholder = startup_loading
            .as_ref()
            .is_some_and(|startup_loading| startup_loading.is_pending(panel.id));
        if terminal.snapshot.surface.is_none() && !startup_placeholder {
            *visibility = Visibility::Hidden;
            continue;
        }
        if matches!(visibility_policy, TerminalVisibilityPolicy::Isolate(id) if id != panel.id) {
            *visibility = Visibility::Hidden;
            continue;
        }
        if active_id.is_some() && !startup_show_all && Some(panel.id) != active_id {
            *visibility = Visibility::Hidden;
            continue;
        }
        let terminal_presentable =
            terminal_has_presentable_uploaded_frame(terminal, presented_terminal);
        let active_ready = Some(panel.id) != active_id
            || active_terminal_ready_for_presentation(terminal, presented_terminal, active_layout)
            || terminal_presentable;
        if !active_ready && !startup_placeholder {
            *visibility = Visibility::Hidden;
            continue;
        }
        if terminal_presentable {
            if let Some(startup_loading) = startup_loading.as_mut() {
                startup_loading.resolve(panel.id);
            }
        }

        let placeholder_texture_state = startup_placeholder_texture_state(
            terminal.snapshot.surface.as_ref(),
            presented_terminal,
        );
        let terminal_texture_state = if startup_placeholder && !terminal_presentable {
            &placeholder_texture_state
        } else {
            &presented_terminal.texture_state
        };
        let smooth_size = smooth_terminal_screen_size(
            terminal_texture_state,
            &view_state,
            &primary_window,
            &layout_state,
        );
        let (_, viewport_center) = active_terminal_viewport(&primary_window, &layout_state);
        let pixel_perfect = !startup_placeholder
            && Some(panel.id) == active_id
            && presented_terminal.display_mode == TerminalDisplayMode::PixelPerfect;

        match active_id {
            Some(id) if id == panel.id => {
                presentation.target_alpha = 1.0;
                presentation.target_position = hud_terminal_target_position(
                    &primary_window,
                    &layout_state,
                    &active_texture_state,
                );
                presentation.target_size = active_size;
                presentation.target_z = if pixel_perfect { 3.0 } else { 0.3 };
            }
            _ => {
                presentation.target_position =
                    viewport_center + view_state.offset + presentation.home_position;
                presentation.target_size = smooth_size;
                presentation.target_alpha = 1.0;
                presentation.target_z = 0.0;
            }
        }

        if snap_switch {
            presentation.current_position = presentation.target_position;
            presentation.current_size = presentation.target_size;
            presentation.current_alpha = presentation.target_alpha;
            presentation.current_z = presentation.target_z;
        } else {
            presentation.current_position = presentation
                .current_position
                .lerp(presentation.target_position, blend);
            presentation.current_size = presentation
                .current_size
                .lerp(presentation.target_size, blend);
            presentation.current_alpha +=
                (presentation.target_alpha - presentation.current_alpha) * blend;
            presentation.current_z += (presentation.target_z - presentation.current_z) * blend;

            if pixel_perfect {
                if presentation
                    .current_position
                    .distance(presentation.target_position)
                    < 0.75
                {
                    presentation.current_position = presentation.target_position;
                }
                if presentation.current_size.distance(presentation.target_size) < 0.75 {
                    presentation.current_size = presentation.target_size;
                }
            }
        }

        *visibility = Visibility::Visible;
        sprite.custom_size = Some(presentation.current_size.max(Vec2::ONE));
        sprite.color = if startup_placeholder && !terminal_presentable {
            if Some(panel.id) == active_id {
                STARTUP_PLACEHOLDER_ACTIVE_COLOR
            } else {
                STARTUP_PLACEHOLDER_COLOR
            }
        } else {
            Color::WHITE
        };
        transform.translation = presentation.current_position.extend(presentation.current_z);
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;
    }

    *last_active_id = active_id;
    *last_visibility_policy = Some(visibility_policy);
    *last_active_texture_state = active_id.map(|_| active_texture_state);
    *last_active_ready = active_ready;
}

#[allow(
    clippy::type_complexity,
    reason = "frame sync needs disjoint panel/frame queries with explicit visibility borrowing"
)]
/// Shows and styles terminal frame sprites for direct-input mode or non-interactive runtime states.
///
/// Interactive terminals with no special state hide their frame entirely.
pub(crate) fn sync_terminal_panel_frames(
    input_capture: Res<crate::hud::HudInputCaptureState>,
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    panels: Query<
        (&TerminalPanel, &TerminalPresentation, &Visibility),
        (With<TerminalPanel>, Without<TerminalPanelFrame>),
    >,
    mut frames: Query<
        (&mut Transform, &mut Sprite, &mut Visibility),
        (With<TerminalPanelFrame>, Without<TerminalPanel>),
    >,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    for (_, _, mut frame_visibility) in &mut frames {
        *frame_visibility = Visibility::Hidden;
    }

    for (panel, presentation, panel_visibility) in &panels {
        if *panel_visibility != Visibility::Visible {
            continue;
        }
        let Some(terminal) = terminal_manager.get(panel.id) else {
            continue;
        };
        let Some(presented_terminal) = presentation_store.get(panel.id) else {
            continue;
        };
        let Ok((mut transform, mut sprite, mut visibility)) =
            frames.get_mut(presented_terminal.frame_entity)
        else {
            continue;
        };

        let direct_input = input_capture.direct_input_terminal == Some(panel.id);
        let runtime_interactive = terminal.snapshot.runtime.is_interactive();
        if !direct_input && runtime_interactive {
            continue;
        }

        let (outset, color) = if direct_input {
            (
                DIRECT_INPUT_FRAME_OUTSET,
                Color::srgba(1.0, 0.48, 0.08, 0.96),
            )
        } else {
            let color = match terminal.snapshot.runtime.lifecycle {
                TerminalLifecycle::Exited { .. } => Color::srgba(0.90, 0.72, 0.18, 0.92),
                TerminalLifecycle::Disconnected => Color::srgba(0.86, 0.20, 0.20, 0.92),
                TerminalLifecycle::Failed => Color::srgba(0.96, 0.10, 0.10, 0.94),
                TerminalLifecycle::Running => unreachable!(),
            };
            (INACTIVE_RUNTIME_FRAME_OUTSET, color)
        };

        *visibility = Visibility::Visible;
        sprite.custom_size =
            Some((presentation.current_size + Vec2::splat(outset * 2.0)).max(Vec2::ONE));
        sprite.color = color;
        transform.translation = presentation
            .current_position
            .extend(presentation.current_z - 0.02);
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;
    }
}

/// Keeps the HUD-surface backdrop aligned behind the active pixel-perfect terminal panel.
///
/// The surface is hidden for non-pixel-perfect display modes or when there is no valid active
/// terminal presentation.
pub(crate) fn sync_terminal_hud_surface(
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    presentation_store: Res<TerminalPresentationStore>,
    visibility_state: Res<TerminalVisibilityState>,
    panels: Query<&TerminalPresentation, With<TerminalPanel>>,
    mut hud_surface: Single<
        (&mut Transform, &mut Sprite, &mut Visibility),
        With<TerminalHudSurfaceMarker>,
    >,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let (transform, sprite, visibility) = &mut *hud_surface;
    let visibility_policy = effective_visibility_policy(&terminal_manager, &visibility_state);
    let Some(active_id) = focus_state.active_id() else {
        **visibility = Visibility::Hidden;
        return;
    };
    let Some(terminal) = terminal_manager.get(active_id) else {
        **visibility = Visibility::Hidden;
        return;
    };
    if matches!(visibility_policy, TerminalVisibilityPolicy::Isolate(id) if id != active_id) {
        **visibility = Visibility::Hidden;
        return;
    }
    let Some(presented_terminal) = presentation_store.get(active_id) else {
        **visibility = Visibility::Hidden;
        return;
    };
    if presented_terminal.display_mode != TerminalDisplayMode::PixelPerfect {
        **visibility = Visibility::Hidden;
        return;
    }
    let Ok(presentation) = panels.get(presented_terminal.panel_entity) else {
        **visibility = Visibility::Hidden;
        return;
    };
    if terminal.snapshot.surface.is_none() {
        **visibility = Visibility::Hidden;
        return;
    }

    **visibility = Visibility::Visible;
    sprite.custom_size = Some(hud_surface_size(presentation.current_size));
    sprite.color = Color::srgba(0.03, 0.03, 0.04, 0.94 * presentation.current_alpha);
    transform.translation = presentation
        .current_position
        .extend(presentation.current_z - 0.1);
    transform.rotation = Quat::IDENTITY;
    transform.scale = Vec3::ONE;
}

#[cfg(test)]
pub(crate) use tests::{
    active_terminal_cell_size, active_terminal_dimensions, active_terminal_layout,
    pixel_perfect_cell_size, pixel_perfect_terminal_logical_size, snap_to_pixel_grid,
};

#[cfg(test)]
mod tests;
