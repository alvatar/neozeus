use crate::{
    app_config::{DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX},
    hud::{HudLayoutState, HudModuleId, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        append_debug_log, raster::create_terminal_image, TerminalDimensions, TerminalDisplayMode,
        TerminalFocusState, TerminalHudSurfaceMarker, TerminalId, TerminalManager, TerminalPanel,
        TerminalPanelFrame, TerminalPanelSprite, TerminalPresentation, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalTextureState, TerminalViewState,
    },
};
use bevy::{prelude::*, window::PrimaryWindow};

pub(crate) const HUD_FRAME_PADDING: Vec2 = Vec2::ZERO;
pub(crate) const ACTIVE_TERMINAL_MARGIN: Vec2 = Vec2::splat(16.0);
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

// Handles home position.
fn terminal_home_position(slot: usize) -> Vec2 {
    const COLUMNS: usize = 3;
    const STEP_X: f32 = 360.0;
    const STEP_Y: f32 = 220.0;
    let column = slot % COLUMNS;
    let row = slot / COLUMNS;
    Vec2::new(-360.0 + column as f32 * STEP_X, 120.0 - row as f32 * STEP_Y)
}

// Spawns terminal presentation.
pub(crate) fn spawn_terminal_presentation(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    presentation_store: &mut TerminalPresentationStore,
    id: TerminalId,
    slot: usize,
) {
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

    presentation_store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: image_handle,
            texture_state: TerminalTextureState {
                texture_size: UVec2::ONE,
                cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: UVec2::ONE,
                cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            panel_entity,
            frame_entity,
        },
    );
}

// Implements window scale factor.
fn window_scale_factor(window: &Window) -> f32 {
    window.scale_factor().max(f32::EPSILON)
}

// Implements logical to physical size.
fn logical_to_physical_size(size: Vec2, window: &Window) -> Vec2 {
    size * window_scale_factor(window)
}

// Implements physical to logical size.
fn physical_to_logical_size(size: Vec2, window: &Window) -> Vec2 {
    size / window_scale_factor(window)
}

// Implements active terminal viewport.
pub(crate) fn active_terminal_viewport(
    window: &Window,
    layout_state: &HudLayoutState,
) -> (Vec2, Vec2) {
    let reserved_left = layout_state
        .get(HudModuleId::AgentList)
        .filter(|module| module.shell.enabled)
        .map(|module| module.shell.current_rect.w)
        .unwrap_or(0.0)
        .clamp(0.0, window.width());
    let usable_size = Vec2::new((window.width() - reserved_left).max(64.0), window.height());
    let center = Vec2::new(reserved_left * 0.5, 0.0);
    (usable_size, center)
}

// Implements active terminal fit area.
fn active_terminal_fit_area(window: &Window, layout_state: &HudLayoutState) -> (Vec2, Vec2) {
    let (viewport_size, viewport_center) = active_terminal_viewport(window, layout_state);
    let fit_size = Vec2::new(
        (viewport_size.x - ACTIVE_TERMINAL_MARGIN.x * 2.0).max(64.0),
        (viewport_size.y - ACTIVE_TERMINAL_MARGIN.y * 2.0).max(64.0),
    );
    (fit_size, viewport_center)
}

// Handles zoom scale.
fn terminal_zoom_scale(view_state: &TerminalViewState) -> f32 {
    10.0 / view_state.distance.max(0.1)
}

// Implements active terminal cell size.
pub(crate) fn active_terminal_cell_size(window: &Window, view_state: &TerminalViewState) -> UVec2 {
    let zoom_scale = terminal_zoom_scale(view_state);
    let cell_size = logical_to_physical_size(
        Vec2::new(DEFAULT_CELL_WIDTH_PX as f32, DEFAULT_CELL_HEIGHT_PX as f32) * zoom_scale,
        window,
    );
    UVec2::new(
        cell_size.x.round().max(1.0) as u32,
        cell_size.y.round().max(1.0) as u32,
    )
}

// Implements active terminal dimensions.
#[cfg(test)]
pub(crate) fn active_terminal_dimensions(
    window: &Window,
    layout_state: &HudLayoutState,
    view_state: &TerminalViewState,
) -> TerminalDimensions {
    active_terminal_layout(window, layout_state, view_state).dimensions
}

// Implements active terminal layout.
pub(crate) fn active_terminal_layout(
    window: &Window,
    layout_state: &HudLayoutState,
    view_state: &TerminalViewState,
) -> ActiveTerminalLayout {
    let cell_size = active_terminal_cell_size(window, view_state);
    let (fit_size_logical, _) = active_terminal_fit_area(window, layout_state);
    let fit_size_physical = logical_to_physical_size(fit_size_logical, window);
    let dimensions = TerminalDimensions {
        cols: ((fit_size_physical.x / cell_size.x.max(1) as f32).floor() as usize).max(1),
        rows: ((fit_size_physical.y / cell_size.y.max(1) as f32).floor() as usize).max(1),
    };
    ActiveTerminalLayout {
        cell_size,
        dimensions,
        texture_size: UVec2::new(
            dimensions.cols as u32 * cell_size.x.max(1),
            dimensions.rows as u32 * cell_size.y.max(1),
        ),
    }
}

// Implements active layout texture state.
fn active_layout_texture_state(layout: ActiveTerminalLayout) -> TerminalTextureState {
    TerminalTextureState {
        texture_size: layout.texture_size,
        cell_size: layout.cell_size,
    }
}

// Implements startup placeholder texture state.
fn startup_placeholder_texture_state(
    surface: Option<&crate::terminals::TerminalSurface>,
    presented_terminal: &crate::terminals::PresentedTerminal,
) -> TerminalTextureState {
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
    let (cols, rows) = surface
        .map(|surface| (surface.cols as u32, surface.rows as u32))
        .unwrap_or((STARTUP_PLACEHOLDER_COLS, STARTUP_PLACEHOLDER_ROWS));
    TerminalTextureState {
        texture_size: UVec2::new(
            cols.max(1) * DEFAULT_CELL_WIDTH_PX,
            rows.max(1) * DEFAULT_CELL_HEIGHT_PX,
        ),
        cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
    }
}

// Implements active terminal ready for presentation.
fn active_terminal_ready_for_presentation(
    terminal: &crate::terminals::registry::ManagedTerminal,
    presented_terminal: &crate::terminals::PresentedTerminal,
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

// Handles has presentable uploaded frame.
fn terminal_has_presentable_uploaded_frame(
    terminal: &crate::terminals::registry::ManagedTerminal,
    presented_terminal: &crate::terminals::PresentedTerminal,
) -> bool {
    terminal.snapshot.surface.is_some()
        && presented_terminal.uploaded_revision == terminal.surface_revision
        && presented_terminal.texture_state.texture_size != UVec2::ONE
        && presented_terminal.texture_state.cell_size != UVec2::ZERO
}

// Synchronizes active terminal dimensions.
pub(crate) fn sync_active_terminal_dimensions(
    mut terminal_manager: ResMut<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    view_state: Res<TerminalViewState>,
    layout_state: Res<HudLayoutState>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
) {
    let Some(active_id) = focus_state.active_id() else {
        return;
    };
    let desired_layout = active_terminal_layout(&primary_window, &layout_state, &view_state);
    let Some(terminal) = terminal_manager.get_mut(active_id) else {
        return;
    };
    let current_dimensions_match = terminal
        .snapshot
        .surface
        .as_ref()
        .map(|surface| {
            surface.cols == desired_layout.dimensions.cols
                && surface.rows == desired_layout.dimensions.rows
        })
        .unwrap_or(false);
    if current_dimensions_match || terminal.requested_dimensions == Some(desired_layout.dimensions)
    {
        return;
    }
    if let Err(error) = runtime_spawner.resize_session(
        &terminal.session_name,
        desired_layout.dimensions.cols,
        desired_layout.dimensions.rows,
    ) {
        append_debug_log(format!(
            "active terminal resize failed session={} cols={} rows={}: {error}",
            terminal.session_name, desired_layout.dimensions.cols, desired_layout.dimensions.rows,
        ));
        return;
    }
    terminal.requested_dimensions = Some(desired_layout.dimensions);
}

// Implements pixel perfect cell size.
#[cfg(test)]
pub(crate) fn pixel_perfect_cell_size(
    cols: usize,
    rows: usize,
    window: &Window,
    layout_state: &HudLayoutState,
) -> UVec2 {
    let base_texture_width = (cols as u32).max(1) as f32 * DEFAULT_CELL_WIDTH_PX as f32;
    let base_texture_height = (rows as u32).max(1) as f32 * DEFAULT_CELL_HEIGHT_PX as f32;
    let (fit_size_logical, _) = active_terminal_fit_area(window, layout_state);
    let fit_size_physical = logical_to_physical_size(fit_size_logical, window);
    let raster_scale = (fit_size_physical.x / base_texture_width)
        .min(fit_size_physical.y / base_texture_height)
        .max(1.0 / DEFAULT_CELL_HEIGHT_PX as f32);

    UVec2::new(
        (DEFAULT_CELL_WIDTH_PX as f32 * raster_scale)
            .floor()
            .max(1.0) as u32,
        (DEFAULT_CELL_HEIGHT_PX as f32 * raster_scale)
            .floor()
            .max(1.0) as u32,
    )
}

// Implements snap to pixel grid.
pub(crate) fn snap_to_pixel_grid(position: Vec2, window: &Window) -> Vec2 {
    let scale_factor = window_scale_factor(window);
    (position * scale_factor).round() / scale_factor
}

// Handles logical size.
fn terminal_logical_size(texture_state: &TerminalTextureState, window: &Window) -> Vec2 {
    physical_to_logical_size(
        Vec2::new(
            texture_state.texture_size.x.max(1) as f32,
            texture_state.texture_size.y.max(1) as f32,
        ),
        window,
    )
}

// Implements smooth terminal screen size.
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
    let zoom_scale = 10.0 / view_state.distance.max(0.1);
    Vec2::new(texture_width, texture_height) * fit_scale * zoom_scale
}

// Handles terminal target position.
fn hud_terminal_target_position(window: &Window, layout_state: &HudLayoutState) -> Vec2 {
    let (_, center) = active_terminal_viewport(window, layout_state);
    snap_to_pixel_grid(center, window)
}

// Handles surface size.
fn hud_surface_size(terminal_size: Vec2) -> Vec2 {
    terminal_size + HUD_FRAME_PADDING * 2.0
}

// Implements pixel perfect terminal logical size.
#[cfg(test)]
pub(crate) fn pixel_perfect_terminal_logical_size(
    texture_state: &TerminalTextureState,
    window: &Window,
) -> Vec2 {
    terminal_logical_size(texture_state, window)
}

// Handles texture screen size.
pub(crate) fn terminal_texture_screen_size(
    texture_state: &TerminalTextureState,
    _view_state: &TerminalViewState,
    window: &Window,
    _layout_state: &HudLayoutState,
    _pixel_perfect: bool,
) -> Vec2 {
    terminal_logical_size(texture_state, window)
}

// Implements effective visibility policy.
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
// Synchronizes terminal presentations.
pub(crate) fn sync_terminal_presentations(
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
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
    let active_layout = active_terminal_layout(&primary_window, &layout_state, &view_state);
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
                presentation.target_position =
                    hud_terminal_target_position(&primary_window, &layout_state);
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
// Synchronizes terminal panel frames.
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
                crate::terminals::TerminalLifecycle::Exited { .. } => {
                    Color::srgba(0.90, 0.72, 0.18, 0.92)
                }
                crate::terminals::TerminalLifecycle::Disconnected => {
                    Color::srgba(0.86, 0.20, 0.20, 0.92)
                }
                crate::terminals::TerminalLifecycle::Failed => Color::srgba(0.96, 0.10, 0.10, 0.94),
                crate::terminals::TerminalLifecycle::Running => unreachable!(),
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

// Synchronizes terminal HUD surface.
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
