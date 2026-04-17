use crate::{
    app::AppPresentationMode,
    hud::{HudLayoutState, TerminalVisibilityPolicy, TerminalVisibilityState},
    visual_contract::{TerminalFrameVisualState, VisualContractState},
};

use super::{
    active_content::ActiveTerminalContentState,
    fonts::{TerminalCellMetrics, TerminalFontState},
    presentation_state::{
        PresentedTerminal, TerminalDisplayMode, TerminalHudSurfaceMarker, TerminalPanel,
        TerminalPanelFrame, TerminalPanelSprite, TerminalPresentation, TerminalPresentationStore,
        TerminalTextureState, TerminalViewState,
    },
    raster::create_terminal_image,
    readiness::{terminal_readiness_for_id, TerminalReadiness},
    registry::{ManagedTerminal, TerminalFocusState, TerminalId, TerminalManager},
    runtime::TerminalRuntimeSpawner,
    types::TerminalDimensions,
};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    sprite::{BorderRect, SliceScaleMode, SpriteImageMode, TextureSlicer},
    window::PrimaryWindow,
};

const HUD_FRAME_PADDING: Vec2 = Vec2::ZERO;
const TERMINAL_FRAME_Z_OFFSET: f32 = 0.02;
const TERMINAL_FRAME_BORDER_PX: u32 = 2;
const STARTUP_PLACEHOLDER_COLS: u32 = 120;
const STARTUP_PLACEHOLDER_ROWS: u32 = 38;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActiveTerminalLayout {
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
fn spawn_terminal_presentation(
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
                image: images.add(create_terminal_frame_image()),
                image_mode: SpriteImageMode::Sliced(TextureSlicer {
                    border: BorderRect::all(TERMINAL_FRAME_BORDER_PX as f32),
                    center_scale_mode: SliceScaleMode::Stretch,
                    sides_scale_mode: SliceScaleMode::Stretch,
                    ..default()
                }),
                color: Color::WHITE,
                custom_size: Some(Vec2::ONE),
                ..default()
            },
            Transform::from_xyz(
                home_position.x,
                home_position.y,
                presentation.current_z + TERMINAL_FRAME_Z_OFFSET,
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
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            uploaded_surface: None,
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
/// The docked agent list claims space on the left when enabled, and the info bar claims space on the
/// top edge when enabled.
fn active_terminal_viewport(window: &Window, layout_state: &HudLayoutState) -> (Vec2, Vec2) {
    let reserved_left = layout_state
        .docked_agent_list_width()
        .clamp(0.0, window.width());
    let reserved_top = layout_state
        .reserved_header_height()
        .clamp(0.0, window.height());
    let usable_size = Vec2::new(
        (window.width() - reserved_left).max(64.0),
        (window.height() - reserved_top).max(64.0),
    );
    let center = Vec2::new(reserved_left * 0.5, -reserved_top * 0.5);
    (usable_size, center)
}

/// Returns the full active-terminal viewport as the focused presentation fit area.
fn active_terminal_fit_area(window: &Window, layout_state: &HudLayoutState) -> (Vec2, Vec2) {
    active_terminal_viewport(window, layout_state)
}

/// Converts the shared view distance into a scalar zoom factor.
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
pub(super) fn active_terminal_layout_for_dimensions(
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
fn hud_terminal_target_position(
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

#[derive(Clone, Debug)]
struct PresentationTransitionContext {
    active_id: Option<TerminalId>,
    visibility_policy: TerminalVisibilityPolicy,
    active_layout: ActiveTerminalLayout,
    active_texture_state: TerminalTextureState,
    active_ready: bool,
    active_size: Vec2,
    snap_switch: bool,
    blend: f32,
}

#[derive(Clone, Debug)]
struct PresentationPlan {
    visible: bool,
    resolve_pending_presentation: bool,
    target_position: Vec2,
    target_size: Vec2,
    target_alpha: f32,
    target_z: f32,
    pixel_perfect: bool,
    sprite_color: Color,
}

#[allow(
    clippy::too_many_arguments,
    reason = "transition planning depends on active focus, previous transition gates, viewport state, and presentation store state together"
)]
fn build_presentation_transition_context(
    time: &Time,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    font_state: Option<&TerminalFontState>,
    presentation_store: &TerminalPresentationStore,
    visibility_state: &TerminalVisibilityState,
    view_state: &TerminalViewState,
    layout_state: &HudLayoutState,
    primary_window: &Window,
    last_active_id: Option<TerminalId>,
    last_visibility_policy: Option<TerminalVisibilityPolicy>,
    last_active_texture_state: Option<TerminalTextureState>,
    last_active_ready: bool,
) -> PresentationTransitionContext {
    let active_id = focus_state.active_id();
    let visibility_policy = effective_visibility_policy(terminal_manager, visibility_state);
    let default_font_state = TerminalFontState::default();
    let font_state = font_state.unwrap_or(&default_font_state);
    let placeholder_dimensions = TerminalDimensions {
        cols: STARTUP_PLACEHOLDER_COLS as usize,
        rows: STARTUP_PLACEHOLDER_ROWS as usize,
    };
    let active_layout = active_id
        .map(|_| {
            active_terminal_layout_for_dimensions(
                primary_window,
                layout_state,
                view_state,
                target_active_terminal_dimensions(primary_window, layout_state, font_state),
                font_state,
            )
        })
        .unwrap_or_else(|| {
            active_terminal_layout_for_dimensions(
                primary_window,
                layout_state,
                view_state,
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
    let snap_switch = last_active_id != active_id
        || last_visibility_policy != Some(visibility_policy)
        || last_active_texture_state != active_id.map(|_| active_texture_state.clone())
        || last_active_ready != active_ready;
    let active_size = physical_to_logical_size(
        Vec2::new(
            active_texture_state.texture_size.x.max(1) as f32,
            active_texture_state.texture_size.y.max(1) as f32,
        ),
        primary_window,
    );
    PresentationTransitionContext {
        active_id,
        visibility_policy,
        active_layout,
        active_texture_state,
        active_ready,
        active_size,
        snap_switch,
        blend: 1.0 - (-time.delta_secs() * 10.0).exp(),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "per-panel presentation planning depends on terminal readiness, viewport state, transition state, and retained presentation state together"
)]
fn build_presentation_plan(
    panel_id: TerminalId,
    terminal: &ManagedTerminal,
    presented_terminal: &PresentedTerminal,
    transition: &PresentationTransitionContext,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
    active_terminal_content: &ActiveTerminalContentState,
    _view_state: &TerminalViewState,
    layout_state: &HudLayoutState,
    primary_window: &Window,
    home_position: Vec2,
) -> PresentationPlan {
    let readiness = terminal_readiness_for_id(panel_id, terminal_manager, presentation_store, None);
    if matches!(
        readiness,
        TerminalReadiness::Missing | TerminalReadiness::StartupPending
    ) {
        return PresentationPlan {
            visible: false,
            resolve_pending_presentation: false,
            target_position: home_position,
            target_size: Vec2::ONE,
            target_alpha: 0.0,
            target_z: 0.0,
            pixel_perfect: false,
            sprite_color: Color::WHITE,
        };
    }
    if transition.active_id != Some(panel_id)
        || matches!(transition.visibility_policy, TerminalVisibilityPolicy::Isolate(id) if id != panel_id)
    {
        return PresentationPlan {
            visible: false,
            resolve_pending_presentation: false,
            target_position: home_position,
            target_size: Vec2::ONE,
            target_alpha: 0.0,
            target_z: 0.0,
            pixel_perfect: false,
            sprite_color: Color::WHITE,
        };
    }

    let terminal_presentable = readiness.is_ready_for_capture();
    let active_override_ready = active_terminal_content
        .presentation_override_revision_for(panel_id)
        .is_none_or(|revision| {
            presented_terminal.uploaded_active_override_revision == Some(revision)
        });
    let active_ready = Some(panel_id) != transition.active_id
        || (active_override_ready
            && (active_terminal_ready_for_presentation(
                terminal,
                presented_terminal,
                transition.active_layout,
            ) || terminal_presentable));
    if !active_ready {
        return PresentationPlan {
            visible: false,
            resolve_pending_presentation: false,
            target_position: home_position,
            target_size: Vec2::ONE,
            target_alpha: 0.0,
            target_z: 0.0,
            pixel_perfect: false,
            sprite_color: Color::WHITE,
        };
    }

    let pixel_perfect = presented_terminal.display_mode == TerminalDisplayMode::PixelPerfect;

    let (target_position, target_size, target_z) = (
        hud_terminal_target_position(
            primary_window,
            layout_state,
            &transition.active_texture_state,
        ),
        transition.active_size,
        if pixel_perfect { 3.0 } else { 0.3 },
    );

    PresentationPlan {
        visible: true,
        resolve_pending_presentation: terminal_presentable,
        target_position,
        target_size,
        target_alpha: 1.0,
        target_z,
        pixel_perfect,
        sprite_color: Color::WHITE,
    }
}

fn apply_presentation_plan(
    presentation: &mut TerminalPresentation,
    transform: &mut Transform,
    sprite: &mut Sprite,
    visibility: &mut Visibility,
    plan: &PresentationPlan,
    snap_switch: bool,
    blend: f32,
) {
    if !plan.visible {
        *visibility = Visibility::Hidden;
        return;
    }

    presentation.target_alpha = plan.target_alpha;
    presentation.target_position = plan.target_position;
    presentation.target_size = plan.target_size;
    presentation.target_z = plan.target_z;

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

        if plan.pixel_perfect {
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
    sprite.color = plan.sprite_color;
    transform.translation = presentation.current_position.extend(presentation.current_z);
    transform.rotation = Quat::IDENTITY;
    transform.scale = Vec3::ONE;
}

#[allow(
    clippy::too_many_arguments,
    reason = "presentation sync needs terminal/presentation/view state together"
)]
/// Recomputes each terminal panel's target size/position/visibility from focus, shared terminal
/// readiness, uploaded-frame state, and display-mode state.
///
/// The system also decides when presentation state should snap immediately versus animate toward new
/// targets.
pub(crate) fn sync_terminal_presentations(
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    font_state: Option<Res<TerminalFontState>>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    visibility_state: Res<TerminalVisibilityState>,
    active_terminal_content: Res<ActiveTerminalContentState>,
    view_state: Res<TerminalViewState>,
    layout_state: Res<HudLayoutState>,
    presentation_mode: Option<Res<AppPresentationMode>>,
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
    let presentation_mode = presentation_mode
        .as_deref()
        .copied()
        .unwrap_or(AppPresentationMode::Normal);
    if presentation_mode.blocks_normal_presentation() {
        for (_, _, _, _, mut visibility) in &mut panels {
            *visibility = Visibility::Hidden;
        }
        *last_active_id = None;
        *last_visibility_policy = None;
        *last_active_texture_state = None;
        *last_active_ready = false;
        return;
    }

    let transition = build_presentation_transition_context(
        &time,
        &terminal_manager,
        &focus_state,
        font_state.as_deref(),
        &presentation_store,
        &visibility_state,
        &view_state,
        &layout_state,
        &primary_window,
        *last_active_id,
        *last_visibility_policy,
        last_active_texture_state.clone(),
        *last_active_ready,
    );

    for (panel, mut presentation, mut transform, mut sprite, mut visibility) in &mut panels {
        let Some(terminal) = terminal_manager.get(panel.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let Some(presented_terminal) = presentation_store.get(panel.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let plan = build_presentation_plan(
            panel.id,
            terminal,
            presented_terminal,
            &transition,
            &terminal_manager,
            &presentation_store,
            &active_terminal_content,
            &view_state,
            &layout_state,
            &primary_window,
            presentation.home_position,
        );
        apply_presentation_plan(
            &mut presentation,
            &mut transform,
            &mut sprite,
            &mut visibility,
            &plan,
            transition.snap_switch,
            transition.blend,
        );
        if plan.resolve_pending_presentation {
            presentation_store.resolve_pending_presentation(panel.id);
        }
    }

    *last_active_id = transition.active_id;
    *last_visibility_policy = Some(transition.visibility_policy);
    *last_active_texture_state = transition
        .active_id
        .map(|_| transition.active_texture_state);
    *last_active_ready = transition.active_ready;
}

fn terminal_frame_style(state: TerminalFrameVisualState) -> Option<Color> {
    match state {
        TerminalFrameVisualState::Hidden => None,
        TerminalFrameVisualState::DirectInput => Some(Color::srgba(1.0, 0.48, 0.08, 0.96)),
        TerminalFrameVisualState::Exited => Some(Color::srgba(0.90, 0.72, 0.18, 0.92)),
        TerminalFrameVisualState::Disconnected => Some(Color::srgba(0.86, 0.20, 0.20, 0.92)),
        TerminalFrameVisualState::Failed => Some(Color::srgba(0.96, 0.10, 0.10, 0.94)),
    }
}

fn create_terminal_frame_image() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: 5,
            height: 5,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255,   0,   0,   0,   0, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

#[allow(
    clippy::type_complexity,
    reason = "frame sync needs disjoint panel/frame queries with explicit visibility borrowing"
)]
/// Shows and styles terminal frame sprites for direct-input mode or non-interactive runtime
/// states.
///
/// Interactive terminals hide their frame unless direct input is active.
pub(crate) fn sync_terminal_panel_frames(
    visual_contract: Res<VisualContractState>,
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    presentation_mode: Option<Res<AppPresentationMode>>,
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
    if presentation_mode
        .as_deref()
        .copied()
        .unwrap_or(AppPresentationMode::Normal)
        .blocks_normal_presentation()
    {
        return;
    }

    for (panel, presentation, panel_visibility) in &panels {
        if *panel_visibility != Visibility::Visible {
            continue;
        }
        if terminal_manager.get(panel.id).is_none() {
            continue;
        }
        let Some(presented_terminal) = presentation_store.get(panel.id) else {
            continue;
        };
        let Ok((mut transform, mut sprite, mut visibility)) =
            frames.get_mut(presented_terminal.frame_entity)
        else {
            continue;
        };

        let Some(color) = terminal_frame_style(visual_contract.frame_for_terminal(panel.id)) else {
            continue;
        };

        *visibility = Visibility::Visible;
        sprite.custom_size = Some(presentation.current_size.max(Vec2::splat(2.0)));
        sprite.color = color;
        transform.translation = presentation
            .current_position
            .extend(presentation.current_z + TERMINAL_FRAME_Z_OFFSET);
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
    presentation_mode: Option<Res<AppPresentationMode>>,
    panels: Query<&TerminalPresentation, With<TerminalPanel>>,
    mut hud_surface: Single<
        (&mut Transform, &mut Sprite, &mut Visibility),
        With<TerminalHudSurfaceMarker>,
    >,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let (transform, sprite, visibility) = &mut *hud_surface;
    if presentation_mode
        .as_deref()
        .copied()
        .unwrap_or(AppPresentationMode::Normal)
        .blocks_normal_presentation()
    {
        **visibility = Visibility::Hidden;
        return;
    }
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
};


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_config::{DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX},
        hud::{HudInputCaptureState, HudLayoutState, HudState, HudWidgetKey},
    };
    use bevy::ecs::system::RunSystemOnce;
    use std::{
        collections::BTreeSet,
        sync::{mpsc, Arc, Mutex},
        time::Duration,
    };

    use super::super::{
        bridge::TerminalBridge,
        daemon::{
            AttachedDaemonSession, DaemonSessionInfo, OwnedTmuxSessionInfo, TerminalDaemonClient,
            TerminalDaemonClientResource,
        },
        debug::TerminalDebugStats,
        mailbox::TerminalUpdateMailbox,
        presentation_state::{
            PresentedTerminal, TerminalDisplayMode, TerminalPresentationStore, TerminalTextureState,
            TerminalViewState,
        },
        registry::{TerminalFocusState, TerminalManager},
        runtime::TerminalRuntimeSpawner,
        types::{TerminalCommand, TerminalRuntimeState, TerminalSurface},
    };

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

    /// Returns the active terminal dimensions together with the derived texture state.
    pub(crate) fn active_terminal_layout(
        window: &Window,
        layout_state: &HudLayoutState,
        view_state: &TerminalViewState,
        font_state: &TerminalFontState,
    ) -> (TerminalDimensions, TerminalTextureState) {
        let layout = active_terminal_layout_for_dimensions(
            window,
            layout_state,
            view_state,
            target_active_terminal_dimensions(window, layout_state, font_state),
            font_state,
        );
        (layout.dimensions, active_layout_texture_state(layout))
    }

    /// Computes the raster cell size chosen for pixel-perfect scaling of a fixed terminal geometry.
    fn pixel_perfect_cell_size(
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
    fn snap_to_pixel_grid(position: Vec2, window: &Window) -> Vec2 {
        let scale_factor = window_scale_factor(window);
        (position * scale_factor).round() / scale_factor
    }

    /// Exposes the logical on-screen size implied by a pixel-perfect texture state.
    fn pixel_perfect_terminal_logical_size(
        texture_state: &TerminalTextureState,
        window: &Window,
    ) -> Vec2 {
        terminal_logical_size(texture_state, window)
    }

    /// Creates a test terminal bridge together with a mailbox tests can use for synthetic updates.
    fn test_bridge() -> (TerminalBridge, Arc<TerminalUpdateMailbox>) {
        let (input_tx, _input_rx) = mpsc::channel::<TerminalCommand>();
        let mailbox = Arc::new(TerminalUpdateMailbox::default());
        let bridge = TerminalBridge::new(
            input_tx,
            mailbox.clone(),
            Arc::new(Mutex::new(TerminalDebugStats::default())),
        );
        (bridge, mailbox)
    }

    /// Inserts a prepared terminal manager into a world together with the mirrored focus resource.
    fn insert_terminal_manager_resources(world: &mut World, terminal_manager: TerminalManager) {
        world.insert_resource(terminal_manager.clone_focus_state());
        world.insert_resource(terminal_manager);
        if !world.contains_resource::<crate::terminals::ActiveTerminalContentState>() {
            world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        }
    }

    /// App-level wrapper around [`insert_terminal_manager_resources`].
    fn insert_terminal_manager_resources_into_app(app: &mut App, terminal_manager: TerminalManager) {
        insert_terminal_manager_resources(app.world_mut(), terminal_manager);
    }

    /// Inserts the default HUD resources needed by presentation systems.
    fn insert_default_hud_resources(world: &mut World) {
        world.insert_resource(HudLayoutState::default());
        world.insert_resource(HudInputCaptureState::default());
        if !world.contains_resource::<TerminalFocusState>() {
            world.insert_resource(TerminalFocusState::default());
        }
        if !world.contains_resource::<crate::agents::AgentCatalog>() {
            world.insert_resource(crate::agents::AgentCatalog::default());
        }
        if !world.contains_resource::<crate::agents::AgentRuntimeIndex>() {
            world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        }
        if !world.contains_resource::<crate::agents::AgentStatusStore>() {
            world.insert_resource(crate::agents::AgentStatusStore::default());
        }
        if !world.contains_resource::<crate::visual_contract::VisualContractState>() {
            world.insert_resource(crate::visual_contract::VisualContractState::default());
        }
    }

    /// Restores the test HUD snapshot into the specific resources presentation tests read.
    fn insert_test_hud_state(world: &mut World, hud_state: HudState) {
        let (layout_state, _modal_state, input_capture) = hud_state.into_resources();
        world.insert_resource(layout_state);
        world.insert_resource(input_capture);
        if !world.contains_resource::<TerminalFocusState>() {
            world.insert_resource(TerminalFocusState::default());
        }
        if !world.contains_resource::<crate::agents::AgentCatalog>() {
            world.insert_resource(crate::agents::AgentCatalog::default());
        }
        if !world.contains_resource::<crate::agents::AgentRuntimeIndex>() {
            world.insert_resource(crate::agents::AgentRuntimeIndex::default());
        }
        if !world.contains_resource::<crate::agents::AgentStatusStore>() {
            world.insert_resource(crate::agents::AgentStatusStore::default());
        }
        if !world.contains_resource::<crate::visual_contract::VisualContractState>() {
            world.insert_resource(crate::visual_contract::VisualContractState::default());
        }
    }

    /// App-level wrapper around [`insert_test_hud_state`].
    fn insert_test_hud_state_into_app(app: &mut App, hud_state: HudState) {
        insert_test_hud_state(app.world_mut(), hud_state);
    }

    #[test]
    fn build_presentation_plan_hides_non_active_panels_when_another_panel_is_active() {
        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let layout_state = HudLayoutState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout = active_terminal_layout_for_dimensions(
            &window,
            &layout_state,
            &view_state,
            target_active_terminal_dimensions(&window, &layout_state, &font_state),
            &font_state,
        );
        let active_texture_state = active_layout_texture_state(active_layout);

        let mut terminal_manager = TerminalManager::default();
        let active_id = terminal_manager.create_terminal(test_bridge().0);
        let other_id = terminal_manager.create_terminal(test_bridge().0);
        terminal_manager
            .get_mut(active_id)
            .unwrap()
            .snapshot
            .surface = Some(TerminalSurface::new(80, 24));
        terminal_manager.get_mut(other_id).unwrap().snapshot.surface =
            Some(TerminalSurface::new(80, 24));

        let mut presentation_store = TerminalPresentationStore::default();
        for id in [active_id, other_id] {
            presentation_store.register(
                id,
                PresentedTerminal {
                    image: Default::default(),
                    texture_state: active_texture_state.clone(),
                    desired_texture_state: active_texture_state.clone(),
                    display_mode: TerminalDisplayMode::Smooth,
                    uploaded_revision: 0,
                    uploaded_active_override_revision: None,
                    uploaded_text_selection_revision: None,
                    uploaded_surface: None,
                    panel_entity: Entity::PLACEHOLDER,
                    frame_entity: Entity::PLACEHOLDER,
                },
            );
        }

        let active_terminal_content = crate::terminals::ActiveTerminalContentState::default();
        let plan = build_presentation_plan(
            other_id,
            terminal_manager.get(other_id).unwrap(),
            presentation_store.get(other_id).unwrap(),
            &PresentationTransitionContext {
                active_id: Some(active_id),
                visibility_policy: crate::hud::TerminalVisibilityPolicy::ShowAll,
                active_layout,
                active_texture_state: active_texture_state.clone(),
                active_ready: true,
                active_size: terminal_texture_screen_size(
                    &active_texture_state,
                    &view_state,
                    &window,
                    &layout_state,
                    false,
                ),
                snap_switch: false,
                blend: 0.5,
            },
            &terminal_manager,
            &presentation_store,
            &active_terminal_content,
            &view_state,
            &layout_state,
            &window,
            Vec2::new(12.0, -8.0),
        );

        assert!(!plan.visible);
        assert!(!plan.resolve_pending_presentation);
    }

    #[test]
    fn build_presentation_plan_hides_active_startup_pending_terminal_until_ready() {
        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let layout_state = HudLayoutState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout = active_terminal_layout_for_dimensions(
            &window,
            &layout_state,
            &view_state,
            target_active_terminal_dimensions(&window, &layout_state, &font_state),
            &font_state,
        );
        let active_texture_state = active_layout_texture_state(active_layout);

        let mut terminal_manager = TerminalManager::default();
        let active_id = terminal_manager.create_terminal(test_bridge().0);
        terminal_manager
            .get_mut(active_id)
            .unwrap()
            .snapshot
            .surface = Some(TerminalSurface::new(80, 24));

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            active_id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: TerminalDisplayMode::PixelPerfect,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
        presentation_store.mark_startup_bootstrap_pending(active_id);

        let active_terminal_content = crate::terminals::ActiveTerminalContentState::default();
        let plan = build_presentation_plan(
            active_id,
            terminal_manager.get(active_id).unwrap(),
            presentation_store.get(active_id).unwrap(),
            &PresentationTransitionContext {
                active_id: Some(active_id),
                visibility_policy: crate::hud::TerminalVisibilityPolicy::ShowAll,
                active_layout,
                active_texture_state: active_texture_state.clone(),
                active_ready: false,
                active_size: terminal_texture_screen_size(
                    &active_texture_state,
                    &view_state,
                    &window,
                    &layout_state,
                    false,
                ),
                snap_switch: true,
                blend: 1.0,
            },
            &terminal_manager,
            &presentation_store,
            &active_terminal_content,
            &view_state,
            &layout_state,
            &window,
            Vec2::ZERO,
        );

        assert!(!plan.visible);
        assert!(!plan.resolve_pending_presentation);
        assert_eq!(plan.sprite_color, Color::WHITE);
        assert!(!plan.pixel_perfect);
    }

    #[derive(Default)]
    struct FakeDaemonClient {
        sessions: Mutex<BTreeSet<String>>,
        resize_requests: Mutex<Vec<(String, usize, usize)>>,
    }

    impl TerminalDaemonClient for FakeDaemonClient {
        /// Returns fake session listings from the in-memory set.
        fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
            Ok(self
                .sessions
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .map(|session_id| DaemonSessionInfo {
                    session_id,
                    runtime: TerminalRuntimeState::running("fake daemon"),
                    revision: 0,
                    created_order: 0,
                    metadata: crate::shared::daemon_wire::DaemonSessionMetadata::default(),
                    metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                })
                .collect())
        }

        fn update_session_metadata(
            &self,
            _session_id: &str,
            _metadata: &crate::shared::daemon_wire::DaemonSessionMetadata,
        ) -> Result<(), String> {
            Ok(())
        }

        /// Creates a fake session with a fixed suffix and inserts it into the set.
        fn create_session_with_env(
            &self,
            prefix: &str,
            _cwd: Option<&str>,
            _env_overrides: &[(String, String)],
        ) -> Result<String, String> {
            let session_id = format!("{prefix}1");
            self.sessions.lock().unwrap().insert(session_id.clone());
            Ok(session_id)
        }

        fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String> {
            Ok(Vec::new())
        }

        fn create_owned_tmux_session(
            &self,
            _owner_agent_uid: &str,
            _display_name: &str,
            _cwd: Option<&str>,
            _command: &str,
        ) -> Result<OwnedTmuxSessionInfo, String> {
            Err("owned tmux not needed in presentation tests".into())
        }

        fn capture_owned_tmux_session(
            &self,
            _session_uid: &str,
            _lines: usize,
        ) -> Result<String, String> {
            Err("owned tmux not needed in presentation tests".into())
        }

        fn kill_owned_tmux_session(&self, _session_uid: &str) -> Result<(), String> {
            Err("owned tmux not needed in presentation tests".into())
        }

        fn kill_owned_tmux_sessions_for_agent(&self, _owner_agent_uid: &str) -> Result<(), String> {
            Err("owned tmux not needed in presentation tests".into())
        }

        /// Returns a dummy attached session with an empty snapshot and a disconnected update channel.
        fn attach_session(&self, _session_id: &str) -> Result<AttachedDaemonSession, String> {
            let (_tx, rx) = mpsc::channel();
            Ok(AttachedDaemonSession {
                snapshot: Default::default(),
                updates: rx,
            })
        }

        /// Accepts and discards the command.
        fn send_command(&self, _session_id: &str, _command: TerminalCommand) -> Result<(), String> {
            Ok(())
        }

        /// Records the resize request for later assertion.
        fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String> {
            self.resize_requests
                .lock()
                .unwrap()
                .push((session_id.to_owned(), cols, rows));
            Ok(())
        }

        /// Accepts and discards the kill request.
        fn kill_session(&self, _session_id: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// Builds a runtime spawner backed by the fake daemon client.
    fn fake_runtime_spawner(client: Arc<FakeDaemonClient>) -> TerminalRuntimeSpawner {
        TerminalRuntimeSpawner::for_tests(TerminalDaemonClientResource::from_client(client))
    }

    fn run_panel_frame_sync(world: &mut World) {
        world
            .run_system_once(crate::visual_contract::sync_visual_contract_state)
            .unwrap();
        world.run_system_once(sync_terminal_panel_frames).unwrap();
    }

    /// Verifies that pixel-perfect cell sizing never collapses to zero and keeps width/height scaling
    /// roughly uniform.
    #[test]
    fn pixel_perfect_cell_size_stays_positive_and_scales_uniformly() {
        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let font_state = TerminalFontState::default();
        let cell_size =
            pixel_perfect_cell_size(120, 38, &window, &hud_state.layout_state(), &font_state);
        assert!(cell_size.x >= 1);
        assert!(cell_size.y >= 1);

        let width_scale = cell_size.x as f32 / DEFAULT_CELL_WIDTH_PX as f32;
        let height_scale = cell_size.y as f32 / DEFAULT_CELL_HEIGHT_PX as f32;
        assert!((width_scale - height_scale).abs() < 0.1);
    }

    /// Verifies that pixel-grid snapping is performed in physical pixels and then mapped back to logical
    /// coordinates via the window scale factor.
    #[test]
    fn snap_to_pixel_grid_respects_window_scale_factor() {
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(1.5));
        let snapped = snap_to_pixel_grid(Vec2::new(10.2, -3.4), &window);
        assert_eq!(snapped, Vec2::new(10.0, -10.0 / 3.0));
    }

    /// Verifies that active terminal target position accounts for texture parity.
    #[test]
    fn active_terminal_target_position_accounts_for_texture_parity() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(1.0));
        window.resolution.set(1400.0, 900.0);

        let mut hud_state = HudState::default();
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let rect = crate::hud::docked_agent_list_rect(&window);
        hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

        let even = hud_terminal_target_position(
            &window,
            &hud_state.layout_state(),
            &TerminalTextureState {
                texture_size: UVec2::new(1000, 800),
                cell_size: UVec2::new(10, 16),
            },
        );
        let odd = hud_terminal_target_position(
            &window,
            &hud_state.layout_state(),
            &TerminalTextureState {
                texture_size: UVec2::new(999, 799),
                cell_size: UVec2::new(9, 17),
            },
        );

        assert_eq!(even, Vec2::new(150.0, 0.0));
        assert_eq!(odd, Vec2::new(150.5, -0.5));
    }

    /// Verifies that pixel-perfect logical sizing divides physical texture size by the window scale
    /// factor.
    #[test]
    fn pixel_perfect_terminal_logical_size_uses_scale_factor() {
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(2.0));
        let texture_state = TerminalTextureState {
            texture_size: UVec2::new(200, 120),
            ..Default::default()
        };
        assert_eq!(
            pixel_perfect_terminal_logical_size(&texture_state, &window),
            Vec2::new(100.0, 60.0)
        );
    }

    /// Verifies that the active-terminal viewport shrinks horizontally when the docked agent list is
    /// enabled.
    #[test]
    fn active_terminal_viewport_reserves_agent_list_column() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let mut hud_state = HudState::default();
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let rect = crate::hud::docked_agent_list_rect(&window);
        hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

        assert_eq!(
            active_terminal_viewport(&window, &hud_state.layout_state()),
            (Vec2::new(1100.0, 900.0), Vec2::new(150.0, 0.0))
        );
    }

    /// Verifies that the active-terminal viewport also reserves the top info-bar header when enabled.
    #[test]
    fn active_terminal_viewport_reserves_info_bar_header() {
        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let mut hud_state = HudState::default();
        hud_state.insert_default_module(HudWidgetKey::InfoBar);
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let info_bar_rect = crate::hud::docked_info_bar_rect(&window);
        let agent_list_rect =
            crate::hud::docked_agent_list_rect_with_top_inset(&window, info_bar_rect.h);
        hud_state.set_module_shell_state(
            HudWidgetKey::InfoBar,
            true,
            info_bar_rect,
            info_bar_rect,
            1.0,
            1.0,
        );
        hud_state.set_module_shell_state(
            HudWidgetKey::AgentList,
            true,
            agent_list_rect,
            agent_list_rect,
            1.0,
            1.0,
        );

        assert_eq!(
            active_terminal_viewport(&window, &hud_state.layout_state()),
            (Vec2::new(1100.0, 840.0), Vec2::new(150.0, -30.0))
        );
    }

    /// Verifies that the fixed two-row info-bar header reserves the same top band on narrower windows.
    #[test]
    fn active_terminal_viewport_uses_fixed_two_row_info_bar_header() {
        let window = Window {
            resolution: (1000, 900).into(),
            ..Default::default()
        };
        let mut hud_state = HudState::default();
        hud_state.insert_default_module(HudWidgetKey::InfoBar);
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let info_bar_rect = crate::hud::docked_info_bar_rect(&window);
        let agent_list_rect =
            crate::hud::docked_agent_list_rect_with_top_inset(&window, info_bar_rect.h);
        hud_state.set_module_shell_state(
            HudWidgetKey::InfoBar,
            true,
            info_bar_rect,
            info_bar_rect,
            1.0,
            1.0,
        );
        hud_state.set_module_shell_state(
            HudWidgetKey::AgentList,
            true,
            agent_list_rect,
            agent_list_rect,
            1.0,
            1.0,
        );

        assert_eq!(info_bar_rect.h, 60.0);
        assert_eq!(
            active_terminal_viewport(&window, &hud_state.layout_state()),
            (Vec2::new(700.0, 840.0), Vec2::new(150.0, -30.0))
        );
    }

    /// Verifies that the active terminal presentation uses the texture's logical size and snaps to the
    /// center of the usable viewport.
    #[test]
    fn active_terminal_presentation_uses_texture_logical_size_and_centers_in_viewport() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
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
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);
        for (_, terminal) in manager.iter_mut() {
            terminal.snapshot.surface = Some(TerminalSurface::new(
                active_layout.0.cols,
                active_layout.0.rows,
            ));
        }
        let texture_state = TerminalTextureState {
            texture_size: active_layout.1.texture_size,
            cell_size: active_layout.1.cell_size,
        };
        let expected_size = terminal_texture_screen_size(
            &texture_state,
            &view_state,
            &window,
            &hud_state.layout_state(),
            false,
        );

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: texture_state.clone(),
                desired_texture_state: texture_state.clone(),
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
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(view_state);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((window, PrimaryWindow));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPresentation, &Transform)>();
        let (presentation, transform) = query.single(&world).unwrap();
        assert!(presentation.current_size.distance(expected_size) < 0.2);
        assert!(
            presentation
                .current_position
                .distance(Vec2::new(150.0, 0.0))
                < 0.2
        );
        assert!((transform.translation.x - 150.0).abs() < 0.2);
        assert!(transform.translation.y.abs() < 0.2);
        assert!((transform.translation.z - 0.3).abs() < 0.01);
    }

    /// Verifies that changing the active terminal layout contract causes immediate presentation snapping
    /// instead of animating through stale geometry.
    #[test]
    fn active_terminal_snaps_immediately_when_active_layout_changes() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);

        let initial_window = Window {
            resolution: (800, 600).into(),
            ..Default::default()
        };
        let final_window = Window {
            resolution: bevy::window::WindowResolution::new(2880, 1800).with_scale_factor_override(1.5),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let initial_layout = active_terminal_layout(
            &initial_window,
            &hud_state.layout_state(),
            &view_state,
            &font_state,
        );
        let final_layout = active_terminal_layout(
            &final_window,
            &hud_state.layout_state(),
            &view_state,
            &font_state,
        );
        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            initial_layout.0.cols,
            initial_layout.0.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 1;

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: initial_layout.1.texture_size,
                    cell_size: initial_layout.1.cell_size,
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: initial_layout.1.texture_size,
                    cell_size: initial_layout.1.cell_size,
                },
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 1,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        app.insert_resource(time);
        insert_terminal_manager_resources_into_app(&mut app, manager);
        app.insert_resource(presentation_store);
        app.insert_resource(crate::hud::TerminalVisibilityState::default());
        app.insert_resource(view_state);
        insert_test_hud_state_into_app(&mut app, hud_state);
        app.add_systems(Update, sync_terminal_presentations);
        let window_entity = app.world_mut().spawn((initial_window, PrimaryWindow)).id();
        app.world_mut().spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        app.update();

        {
            let world = app.world_mut();
            let mut window = world.get_mut::<Window>(window_entity).unwrap();
            *window = final_window.clone();
        }
        {
            let world = app.world_mut();
            let mut manager = world.resource_mut::<TerminalManager>();
            manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
                final_layout.0.cols,
                final_layout.0.rows,
            ));
            manager.get_mut(id).unwrap().surface_revision = 2;
        }
        {
            let world = app.world_mut();
            let mut store = world.resource_mut::<TerminalPresentationStore>();
            let presented = store.get_mut(id).unwrap();
            presented.texture_state = TerminalTextureState {
                texture_size: final_layout.1.texture_size,
                cell_size: final_layout.1.cell_size,
            };
            presented.desired_texture_state = presented.texture_state.clone();
            presented.uploaded_revision = 2;
        }

        app.update();

        let expected_size = terminal_texture_screen_size(
            &TerminalTextureState {
                texture_size: final_layout.1.texture_size,
                cell_size: final_layout.1.cell_size,
            },
            &TerminalViewState::default(),
            &final_window,
            &HudState::default().layout_state(),
            false,
        );
        let world = app.world_mut();
        let mut query = world.query::<&TerminalPresentation>();
        let presentation = query.single(world).unwrap();
        assert_eq!(presentation.current_size, expected_size);
        assert_eq!(presentation.target_size, expected_size);
    }

    /// Verifies that changing active focus/isolation snaps the new active terminal immediately instead of
    /// blending from its old background presentation.
    #[test]
    fn switching_active_terminal_snaps_immediately_without_animation() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge_one, _) = test_bridge();
        let (bridge_two, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id_one = manager.create_terminal(bridge_one);
        let id_two = manager.create_terminal(bridge_two);
        manager.focus_terminal(id_one);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let mut hud_state = HudState::default();
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let rect = crate::hud::docked_agent_list_rect(&window);
        hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);
        let dimensions = active_layout.0;
        let active_texture_state = TerminalTextureState {
            texture_size: active_layout.1.texture_size,
            cell_size: active_layout.1.cell_size,
        };
        let stale_background_texture_state = TerminalTextureState {
            texture_size: UVec2::new(
                dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
                dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
            ),
            cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
        };
        let expected_size = terminal_texture_screen_size(
            &active_texture_state,
            &view_state,
            &window,
            &hud_state.layout_state(),
            false,
        );

        for (_, terminal) in manager.iter_mut() {
            terminal.snapshot.surface = Some(TerminalSurface::new(dimensions.cols, dimensions.rows));
        }

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id_one,
            PresentedTerminal {
                image: Default::default(),
                texture_state: active_texture_state.clone(),
                desired_texture_state: active_texture_state,
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
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
                texture_state: stale_background_texture_state.clone(),
                desired_texture_state: stale_background_texture_state,
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        app.insert_resource(time);
        insert_terminal_manager_resources_into_app(&mut app, manager);
        app.insert_resource(presentation_store);
        app.insert_resource(crate::hud::TerminalVisibilityState {
            policy: crate::hud::TerminalVisibilityPolicy::ShowAll,
        });
        app.insert_resource(view_state);
        insert_test_hud_state_into_app(&mut app, hud_state);
        app.add_systems(Update, sync_terminal_presentations);
        app.world_mut().spawn((window, PrimaryWindow));
        app.world_mut().spawn((
            TerminalPanel { id: id_one },
            TerminalPresentation {
                home_position: Vec2::new(-360.0, 120.0),
                current_position: Vec2::new(-360.0, 120.0),
                target_position: Vec2::new(-360.0, 120.0),
                current_size: Vec2::new(200.0, 120.0),
                target_size: Vec2::new(200.0, 120.0),
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.3,
                target_z: 0.3,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));
        app.world_mut().spawn((
            TerminalPanel { id: id_two },
            TerminalPresentation {
                home_position: Vec2::new(0.0, 120.0),
                current_position: Vec2::new(0.0, 120.0),
                target_position: Vec2::new(0.0, 120.0),
                current_size: Vec2::new(200.0, 120.0),
                target_size: Vec2::new(200.0, 120.0),
                current_alpha: 0.84,
                target_alpha: 0.84,
                current_z: -0.05,
                target_z: -0.05,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        app.update();

        {
            let focus_state = {
                let mut manager = app.world_mut().resource_mut::<TerminalManager>();
                manager.focus_terminal(id_two);
                manager.clone_focus_state()
            };
            app.world_mut().insert_resource(focus_state);
        }
        app.world_mut()
            .resource_mut::<crate::hud::TerminalVisibilityState>()
            .policy = crate::hud::TerminalVisibilityPolicy::Isolate(id_two);

        app.update();

        let world = app.world_mut();
        let mut query = world.query::<(&TerminalPanel, &TerminalPresentation, &Visibility)>();
        let rows = query.iter(world).collect::<Vec<_>>();
        let first = rows
            .iter()
            .find(|(panel, _, _)| panel.id == id_one)
            .unwrap();
        let second = rows
            .iter()
            .find(|(panel, _, _)| panel.id == id_two)
            .unwrap();
        assert_eq!(*first.2, Visibility::Hidden);
        assert_eq!(second.1.current_position, Vec2::new(150.0, 0.0));
        assert_eq!(second.1.current_size, expected_size);
        assert_eq!(second.1.current_alpha, 1.0);
        assert_eq!(second.1.current_z, 0.3);
    }

    /// Verifies that when focus switches to a terminal whose active-layout upload is not ready yet, the
    /// cached frame stays visible rather than disappearing.
    #[test]
    fn switching_active_terminal_keeps_cached_frame_visible_until_resized_surface_arrives() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge_one, _) = test_bridge();
        let (bridge_two, _) = test_bridge();
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

        let mut view_state = TerminalViewState::default();
        view_state.distance = 5.0;
        let layout_state = hud_state.layout_state();
        let font_state = TerminalFontState::default();
        let active_layout = active_terminal_layout(&window, &layout_state, &view_state, &font_state);
        let dimensions = active_layout.0;
        let active_texture_state = TerminalTextureState {
            texture_size: active_layout.1.texture_size,
            cell_size: active_layout.1.cell_size,
        };
        let cached_background_state = TerminalTextureState {
            texture_size: UVec2::new(
                dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
                dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
            ),
            cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
        };
        let expected_size = terminal_texture_screen_size(
            &cached_background_state,
            &view_state,
            &window,
            &layout_state,
            false,
        );

        manager.focus_terminal(id_one);
        for (_, terminal) in manager.iter_mut() {
            terminal.snapshot.surface = Some(TerminalSurface::new(dimensions.cols, dimensions.rows));
            terminal.surface_revision = 1;
        }

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

        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        app.insert_resource(time);
        insert_terminal_manager_resources_into_app(&mut app, manager);
        app.insert_resource(presentation_store);
        app.insert_resource(crate::hud::TerminalVisibilityState {
            policy: crate::hud::TerminalVisibilityPolicy::ShowAll,
        });
        app.insert_resource(view_state);
        insert_test_hud_state_into_app(&mut app, hud_state);
        app.add_systems(Update, sync_terminal_presentations);
        app.world_mut().spawn((window, PrimaryWindow));
        app.world_mut().spawn((
            TerminalPanel { id: id_one },
            TerminalPresentation {
                home_position: Vec2::new(-360.0, 120.0),
                current_position: Vec2::new(-360.0, 120.0),
                target_position: Vec2::new(-360.0, 120.0),
                current_size: Vec2::new(200.0, 120.0),
                target_size: Vec2::new(200.0, 120.0),
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.3,
                target_z: 0.3,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));
        app.world_mut().spawn((
            TerminalPanel { id: id_two },
            TerminalPresentation {
                home_position: Vec2::new(0.0, 120.0),
                current_position: Vec2::new(0.0, 120.0),
                target_position: Vec2::new(0.0, 120.0),
                current_size: Vec2::new(200.0, 120.0),
                target_size: Vec2::new(200.0, 120.0),
                current_alpha: 0.84,
                target_alpha: 0.84,
                current_z: -0.05,
                target_z: -0.05,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        app.update();

        {
            let focus_state = {
                let mut manager = app.world_mut().resource_mut::<TerminalManager>();
                manager.focus_terminal(id_two);
                manager.clone_focus_state()
            };
            app.world_mut().insert_resource(focus_state);
        }
        app.world_mut()
            .resource_mut::<crate::hud::TerminalVisibilityState>()
            .policy = crate::hud::TerminalVisibilityPolicy::Isolate(id_two);

        app.update();

        let world = app.world_mut();
        let mut query = world.query::<(&TerminalPanel, &TerminalPresentation, &Visibility)>();
        let rows = query.iter(world).collect::<Vec<_>>();
        let first = rows
            .iter()
            .find(|(panel, _, _)| panel.id == id_one)
            .unwrap();
        let second = rows
            .iter()
            .find(|(panel, _, _)| panel.id == id_two)
            .unwrap();
        assert_eq!(*first.2, Visibility::Hidden);
        assert_eq!(*second.2, Visibility::Visible);
        assert_eq!(second.1.current_position, Vec2::new(150.0, 0.0));
        assert_eq!(second.1.current_size, expected_size);
        assert_eq!(second.1.current_alpha, 1.0);
        assert_eq!(second.1.current_z, 0.3);
    }

    /// Verifies that the active PTY is resized to the fixed-cell grid that fits the remaining HUD
    /// viewport, independent of zoom distance.
    #[test]
    fn active_terminal_resize_requests_follow_viewport_grid_policy() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let client = Arc::new(FakeDaemonClient::default());
        client
            .sessions
            .lock()
            .unwrap()
            .insert("neozeus-session-1".into());
        let runtime_spawner = fake_runtime_spawner(client.clone());
        let (bridge, _) = test_bridge();

        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-1".into());
        manager.get_mut(terminal_id).unwrap().snapshot.surface = Some(TerminalSurface::new(120, 38));

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let mut hud_state = HudState::default();
        hud_state.insert_default_module(HudWidgetKey::AgentList);
        let rect = crate::hud::docked_agent_list_rect(&window);
        hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

        let mut view_state = TerminalViewState::default();
        view_state.distance = 5.0;

        let mut world = World::default();
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(TerminalFontState::default());
        world.insert_resource(runtime_spawner);
        world.insert_resource(view_state);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((window, PrimaryWindow));

        world
            .run_system_once(sync_active_terminal_dimensions)
            .unwrap();

        let requests = client.resize_requests.lock().unwrap().clone();
        assert_eq!(requests, vec![("neozeus-session-1".into(), 122, 56)]);
    }

    /// Verifies that with no active terminal the panel hides in place instead of being re-laid out into
    /// a background/home slot.
    #[test]
    fn no_active_terminal_hides_panel_without_repositioning_it() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal_without_focus(bridge);
        for (_, terminal) in manager.iter_mut() {
            terminal.snapshot.surface = Some(TerminalSurface::new(2, 2));
        }

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: UVec2::new(100, 100),
                    cell_size: UVec2::new(10, 20),
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: UVec2::new(100, 100),
                    cell_size: UVec2::new(10, 20),
                },
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
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(TerminalViewState::default());
        insert_default_hud_resources(&mut world);
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..Default::default()
            },
            PrimaryWindow,
        ));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::new(-360.0, 120.0),
                current_position: Vec2::new(111.0, -77.0),
                target_position: Vec2::new(111.0, -77.0),
                current_size: Vec2::new(222.0, 140.0),
                target_size: Vec2::new(222.0, 140.0),
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.3,
                target_z: 0.3,
            },
            Transform::from_xyz(111.0, -77.0, 0.3),
            Sprite::default(),
            Visibility::Visible,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPanel, &TerminalPresentation, &Transform, &Visibility)>();
        let panels = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].0.id, id);
        assert_eq!(*panels[0].3, Visibility::Hidden);
        assert_eq!(panels[0].1.current_position, Vec2::new(111.0, -77.0));
        assert_eq!(panels[0].1.target_position, Vec2::new(111.0, -77.0));
        assert_eq!(panels[0].2.translation, Vec3::new(111.0, -77.0, 0.3));
    }

    /// Verifies that panel frame sprites default to hidden when no direct-input or runtime-status frame
    /// should be shown.
    #[test]
    fn terminal_panel_frames_are_hidden_without_direct_input_mode() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let mut world = World::default();
        insert_default_hud_resources(&mut world);
        world.insert_resource(TerminalManager::default());
        world.insert_resource(TerminalPresentationStore::default());
        world.spawn((
            TerminalPanelFrame {
                id: crate::terminals::TerminalId(1),
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        run_panel_frame_sync(&mut world);

        let mut query = world.query::<(&TerminalPanelFrame, &Visibility)>();
        let vis = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(vis.len(), 1);
        assert_eq!(*vis[0].1, Visibility::Hidden);
    }

    /// Verifies that direct-input mode shows the orange focus frame around the active terminal panel.
    #[test]
    fn direct_input_mode_shows_orange_terminal_frame() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal(bridge);

        let mut hud_state = crate::hud::HudState::default();
        hud_state.open_direct_terminal_input(terminal_id);

        let mut world = World::default();
        insert_test_hud_state(&mut world, hud_state);
        world.insert_resource(manager);
        let panel_entity = world
            .spawn((
                TerminalPanel { id: terminal_id },
                TerminalPresentation {
                    home_position: Vec2::ZERO,
                    current_position: Vec2::new(30.0, -20.0),
                    target_position: Vec2::ZERO,
                    current_size: Vec2::new(320.0, 180.0),
                    target_size: Vec2::ZERO,
                    current_alpha: 1.0,
                    target_alpha: 1.0,
                    current_z: 0.5,
                    target_z: 0.0,
                },
                Visibility::Visible,
            ))
            .id();
        let frame_entity = world
            .spawn((
                TerminalPanelFrame { id: terminal_id },
                Transform::default(),
                Sprite::default(),
                Visibility::Hidden,
            ))
            .id();
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            terminal_id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity,
                frame_entity,
            },
        );
        world.insert_resource(presentation_store);

        run_panel_frame_sync(&mut world);

        let mut query = world.query::<(&TerminalPanelFrame, &Transform, &Sprite, &Visibility)>();
        let frames = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(frames.len(), 1);
        assert_eq!(*frames[0].3, Visibility::Visible);
        assert_eq!(frames[0].1.translation, Vec3::new(30.0, -20.0, 0.52));
        assert_eq!(frames[0].2.custom_size, Some(Vec2::new(320.0, 180.0)));
        assert_eq!(frames[0].2.color, Color::srgba(1.0, 0.48, 0.08, 0.96));
    }

    /// Verifies that working activity does not draw a terminal frame when direct input is closed.
    #[test]
    fn working_terminal_keeps_frame_hidden_without_direct_input() {
        let mut world = World::default();
        insert_default_hud_resources(&mut world);
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal(bridge);
        manager
            .get_mut(terminal_id)
            .expect("terminal should exist")
            .snapshot
            .surface = Some({
            let mut surface = TerminalSurface::new(120, 8);
            surface.set_text_cell(0, 0, "header");
            surface.set_text_cell(1, 3, "⠋ Working...");
            surface
        });

        let mut catalog = crate::agents::AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("alpha".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
        );
        let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
        runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        world.insert_resource(time);
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
        world.insert_resource(crate::agents::AgentStatusStore::default());
        world.insert_resource(manager);
        world
            .run_system_once(crate::agents::sync_agent_status)
            .expect("status sync should succeed");
        let panel_entity = world
            .spawn((
                TerminalPanel { id: terminal_id },
                TerminalPresentation {
                    home_position: Vec2::ZERO,
                    current_position: Vec2::new(10.0, 15.0),
                    target_position: Vec2::ZERO,
                    current_size: Vec2::new(300.0, 160.0),
                    target_size: Vec2::ZERO,
                    current_alpha: 1.0,
                    target_alpha: 1.0,
                    current_z: 0.5,
                    target_z: 0.0,
                },
                Visibility::Visible,
            ))
            .id();
        let frame_entity = world
            .spawn((
                TerminalPanelFrame { id: terminal_id },
                Transform::default(),
                Sprite::default(),
                Visibility::Visible,
            ))
            .id();
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            terminal_id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity,
                frame_entity,
            },
        );
        world.insert_resource(presentation_store);

        run_panel_frame_sync(&mut world);

        let mut query = world.query::<(&TerminalPanelFrame, &Visibility)>();
        let frames = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(frames.len(), 1);
        assert_eq!(*frames[0].1, Visibility::Hidden);
    }

    /// Verifies that direct input keeps the orange frame even when the terminal is working.
    #[test]
    fn direct_input_mode_keeps_orange_frame_when_terminal_is_working() {
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal(bridge);
        manager
            .get_mut(terminal_id)
            .expect("terminal should exist")
            .snapshot
            .surface = Some({
            let mut surface = TerminalSurface::new(120, 8);
            surface.set_text_cell(0, 0, "header");
            surface.set_text_cell(1, 3, "⠋ Working...");
            surface
        });

        let mut hud_state = crate::hud::HudState::default();
        hud_state.open_direct_terminal_input(terminal_id);

        let mut catalog = crate::agents::AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("alpha".into()),
            crate::agents::AgentKind::Pi,
            crate::agents::AgentKind::Pi.capabilities(),
        );
        let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
        runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

        let mut world = World::default();
        insert_test_hud_state(&mut world, hud_state);
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        world.insert_resource(time);
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
        world.insert_resource(crate::agents::AgentStatusStore::default());
        world.insert_resource(manager);
        world
            .run_system_once(crate::agents::sync_agent_status)
            .expect("status sync should succeed");
        let panel_entity = world
            .spawn((
                TerminalPanel { id: terminal_id },
                TerminalPresentation {
                    home_position: Vec2::ZERO,
                    current_position: Vec2::new(30.0, -20.0),
                    target_position: Vec2::ZERO,
                    current_size: Vec2::new(320.0, 180.0),
                    target_size: Vec2::ZERO,
                    current_alpha: 1.0,
                    target_alpha: 1.0,
                    current_z: 0.5,
                    target_z: 0.0,
                },
                Visibility::Visible,
            ))
            .id();
        let frame_entity = world
            .spawn((
                TerminalPanelFrame { id: terminal_id },
                Transform::default(),
                Sprite::default(),
                Visibility::Hidden,
            ))
            .id();
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            terminal_id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity,
                frame_entity,
            },
        );
        world.insert_resource(presentation_store);

        run_panel_frame_sync(&mut world);

        let mut query = world.query::<(&TerminalPanelFrame, &Transform, &Sprite, &Visibility)>();
        let frames = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(frames.len(), 1);
        assert_eq!(*frames[0].3, Visibility::Visible);
        assert_eq!(frames[0].1.translation, Vec3::new(30.0, -20.0, 0.52));
        assert_eq!(frames[0].2.custom_size, Some(Vec2::new(320.0, 180.0)));
        assert_eq!(frames[0].2.color, Color::srgba(1.0, 0.48, 0.08, 0.96));
    }

    /// Verifies that a disconnected terminal shows the red runtime-status frame instead of the direct
    /// input frame styling.
    #[test]
    fn disconnected_terminal_shows_red_status_frame() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let mut world = World::default();
        insert_default_hud_resources(&mut world);
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal(bridge);
        manager
            .get_mut(terminal_id)
            .expect("terminal should exist")
            .snapshot
            .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
        world.insert_resource(manager);
        let panel_entity = world
            .spawn((
                TerminalPanel { id: terminal_id },
                TerminalPresentation {
                    home_position: Vec2::ZERO,
                    current_position: Vec2::new(10.0, 15.0),
                    target_position: Vec2::ZERO,
                    current_size: Vec2::new(300.0, 160.0),
                    target_size: Vec2::ZERO,
                    current_alpha: 1.0,
                    target_alpha: 1.0,
                    current_z: 0.5,
                    target_z: 0.0,
                },
                Visibility::Visible,
            ))
            .id();
        let frame_entity = world
            .spawn((
                TerminalPanelFrame { id: terminal_id },
                Transform::default(),
                Sprite::default(),
                Visibility::Hidden,
            ))
            .id();
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            terminal_id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity,
                frame_entity,
            },
        );
        world.insert_resource(presentation_store);

        run_panel_frame_sync(&mut world);

        let mut query = world.query::<(&TerminalPanelFrame, &Transform, &Sprite, &Visibility)>();
        let frames = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(frames.len(), 1);
        assert_eq!(*frames[0].3, Visibility::Visible);
        assert_eq!(frames[0].1.translation, Vec3::new(10.0, 15.0, 0.52));
        assert_eq!(frames[0].2.custom_size, Some(Vec2::new(300.0, 160.0)));
        assert_eq!(frames[0].2.color, Color::srgba(0.86, 0.20, 0.20, 0.92));
    }

    /// Verifies that startup-loading terminals stay hidden until they have a correct presentable frame.
    #[test]
    fn startup_loading_hides_active_terminal_until_first_real_frame_arrives() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
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

        presentation_store.mark_startup_bootstrap_pending(id);

        let mut world = World::default();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(TerminalViewState::default());
        insert_test_hud_state(&mut world, HudState::default());
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..Default::default()
            },
            PrimaryWindow,
        ));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Hidden,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPanel, &Sprite, &Visibility)>();
        let (_, sprite, visibility) = query.single(&world).unwrap();
        assert_eq!(*visibility, Visibility::Hidden);
        assert_eq!(sprite.color, Color::WHITE);
    }

    #[test]
    fn startup_presentation_mode_hides_ready_active_terminal_until_startup_overlay_owns_output() {
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);
        manager
            .get_mut(id)
            .expect("terminal exists")
            .snapshot
            .surface = Some(crate::tests::surface_with_text(8, 120, 0, "ready"));

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: UVec2::new(1200, 160),
                    cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: UVec2::new(1200, 160),
                    cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
                },
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
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
        world.insert_resource(TerminalViewState::default());
        world.insert_resource(crate::app::AppPresentationMode::StartupOverlay);
        insert_test_hud_state(&mut world, HudState::default());
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..Default::default()
            },
            PrimaryWindow,
        ));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPanel, &Visibility)>();
        let (_, visibility) = query.single(&world).unwrap();
        assert_eq!(*visibility, Visibility::Hidden);
    }

    /// Verifies that startup-loading does not broaden terminal visibility before any terminal is ready.
    #[test]
    fn startup_loading_does_not_override_isolate_to_show_pending_terminals() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge_one, _) = test_bridge();
        let (bridge_two, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id_one = manager.create_terminal_without_focus(bridge_one);
        let id_two = manager.create_terminal(bridge_two);

        let mut presentation_store = TerminalPresentationStore::default();
        for id in [id_one, id_two] {
            presentation_store.register(
                id,
                PresentedTerminal {
                    image: Default::default(),
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
        }

        presentation_store.mark_startup_bootstrap_pending(id_one);
        presentation_store.mark_startup_bootstrap_pending(id_two);

        let visibility_state = crate::hud::TerminalVisibilityState {
            policy: crate::hud::TerminalVisibilityPolicy::Isolate(id_two),
        };

        let mut world = World::default();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(visibility_state);
        world.insert_resource(TerminalViewState::default());
        insert_test_hud_state(&mut world, HudState::default());
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..Default::default()
            },
            PrimaryWindow,
        ));
        for id in [id_one, id_two] {
            world.spawn((
                TerminalPanel { id },
                TerminalPresentation {
                    home_position: Vec2::ZERO,
                    current_position: Vec2::ZERO,
                    target_position: Vec2::ZERO,
                    current_size: Vec2::ONE,
                    target_size: Vec2::ONE,
                    current_alpha: 1.0,
                    target_alpha: 1.0,
                    current_z: 0.0,
                    target_z: 0.0,
                },
                Transform::default(),
                Sprite::default(),
                Visibility::Hidden,
            ));
        }

        world.run_system_once(sync_terminal_presentations).unwrap();

        let visible_count = world
            .query::<(&TerminalPanel, &Visibility)>()
            .iter(&world)
            .filter(|(_, visibility)| **visibility == Visibility::Visible)
            .count();
        assert_eq!(visible_count, 0);
    }

    /// Verifies that the active terminal does not disappear while its desired active-layout upload is
    /// still pending; the cached frame stays visible.
    #[test]
    fn active_terminal_presentation_keeps_cached_frame_visible_until_active_layout_upload_is_ready() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);
        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(80, 24));
        manager.get_mut(id).unwrap().surface_revision = 1;

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = crate::hud::HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: UVec2::new(80 * DEFAULT_CELL_WIDTH_PX, 24 * DEFAULT_CELL_HEIGHT_PX),
                    cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
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
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(view_state);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((window, PrimaryWindow));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPanel, &Visibility)>();
        let vis = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(vis.len(), 1);
        assert_eq!(*vis[0].1, Visibility::Visible);
    }

    /// Verifies that once a terminal becomes ready for the new active layout, it reappears already
    /// snapped to the final geometry rather than animating in.
    #[test]
    fn active_terminal_reappears_snapped_after_becoming_ready_for_new_layout() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);

        let initial_window = Window {
            resolution: (800, 600).into(),
            ..Default::default()
        };
        let final_window = Window {
            resolution: bevy::window::WindowResolution::new(2880, 1800).with_scale_factor_override(1.5),
            ..Default::default()
        };
        let hud_state = HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let initial_layout = active_terminal_layout(
            &initial_window,
            &hud_state.layout_state(),
            &view_state,
            &font_state,
        );
        let final_layout = active_terminal_layout(
            &final_window,
            &hud_state.layout_state(),
            &view_state,
            &font_state,
        );

        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            initial_layout.0.cols,
            initial_layout.0.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 1;

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: initial_layout.1.texture_size,
                    cell_size: initial_layout.1.cell_size,
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: initial_layout.1.texture_size,
                    cell_size: initial_layout.1.cell_size,
                },
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 1,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        app.insert_resource(time);
        insert_terminal_manager_resources_into_app(&mut app, manager);
        app.insert_resource(presentation_store);
        app.insert_resource(crate::hud::TerminalVisibilityState::default());
        app.insert_resource(view_state);
        insert_test_hud_state_into_app(&mut app, hud_state);
        app.add_systems(Update, sync_terminal_presentations);
        let window_entity = app.world_mut().spawn((initial_window, PrimaryWindow)).id();
        app.world_mut().spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        app.update();

        {
            let world = app.world_mut();
            let mut window = world.get_mut::<Window>(window_entity).unwrap();
            *window = final_window.clone();
        }
        app.update();

        {
            let world = app.world_mut();
            let mut manager = world.resource_mut::<TerminalManager>();
            manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
                final_layout.0.cols,
                final_layout.0.rows,
            ));
            manager.get_mut(id).unwrap().surface_revision = 2;
        }
        {
            let world = app.world_mut();
            let mut store = world.resource_mut::<TerminalPresentationStore>();
            let presented = store.get_mut(id).unwrap();
            presented.texture_state = TerminalTextureState {
                texture_size: final_layout.1.texture_size,
                cell_size: final_layout.1.cell_size,
            };
            presented.desired_texture_state = presented.texture_state.clone();
            presented.uploaded_revision = 2;
        }

        app.update();

        let expected_size = terminal_texture_screen_size(
            &TerminalTextureState {
                texture_size: final_layout.1.texture_size,
                cell_size: final_layout.1.cell_size,
            },
            &TerminalViewState::default(),
            &final_window,
            &HudState::default().layout_state(),
            false,
        );
        let world = app.world_mut();
        let mut query = world.query::<(&TerminalPresentation, &Visibility)>();
        let (presentation, visibility) = query.single(world).unwrap();
        assert_eq!(*visibility, Visibility::Visible);
        assert_eq!(presentation.current_size, expected_size);
        assert_eq!(presentation.target_size, expected_size);
    }

    /// Verifies that selecting an owned tmux child does not expose the owner's stale terminal frame
    /// before the tmux override itself has been uploaded.
    #[test]
    fn selected_tmux_child_hides_owner_panel_until_override_surface_is_ready() {
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = crate::hud::HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            active_layout.0.cols,
            active_layout.0.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 1;

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 1,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                uploaded_surface: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );

        let mut active_terminal_content = crate::terminals::ActiveTerminalContentState::default();
        active_terminal_content.select_owned_tmux("tmux-session-1".into(), Some(id));

        let mut world = World::default();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(active_terminal_content);
        world.insert_resource(view_state);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((window, PrimaryWindow));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPanel, &Visibility)>();
        let vis = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(vis.len(), 1);
        assert_eq!(vis[0].0.id, id);
        assert_eq!(
            *vis[0].1,
            Visibility::Hidden,
            "owner terminal frame must stay hidden until the selected tmux content is actually ready"
        );
    }

    /// Verifies that an active terminal presentation becomes visible as soon as its uploaded texture
    /// contract matches the active layout.
    #[test]
    fn active_terminal_presentation_becomes_visible_once_active_layout_upload_is_ready() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = crate::hud::HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            active_layout.0.cols,
            active_layout.0.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 1;

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
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
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(view_state);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((window, PrimaryWindow));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Hidden,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPanel, &Visibility)>();
        let vis = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(vis.len(), 1);
        assert_eq!(*vis[0].1, Visibility::Visible);
    }

    /// Verifies that a startup-pending active terminal reveals directly at final geometry once its
    /// uploaded texture contract becomes ready, without requiring any later interaction-driven fixup.
    #[test]
    fn startup_loading_reveals_active_terminal_directly_at_final_geometry_once_ready() {
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = crate::hud::HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);
        let expected_size = terminal_texture_screen_size(
            &TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            },
            &view_state,
            &window,
            &hud_state.layout_state(),
            false,
        );

        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            active_layout.0.cols,
            active_layout.0.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 1;

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
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
        presentation_store.mark_startup_bootstrap_pending(id);

        let mut world = World::default();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(view_state);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((window.clone(), PrimaryWindow));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Hidden,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();
        {
            let mut query = world.query::<(&TerminalPresentation, &Visibility)>();
            let (presentation, visibility) = query.single(&world).unwrap();
            assert_eq!(*visibility, Visibility::Hidden);
            assert_eq!(presentation.current_position, Vec2::ZERO);
        }

        {
            let mut store = world.resource_mut::<TerminalPresentationStore>();
            let presented = store.get_mut(id).unwrap();
            presented.texture_state = TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            };
            presented.desired_texture_state = presented.texture_state.clone();
            presented.uploaded_revision = 1;
        }

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPresentation, &Visibility)>();
        let (presentation, visibility) = query.single(&world).unwrap();
        assert_eq!(*visibility, Visibility::Visible);
        assert_eq!(presentation.current_size, expected_size);
        assert_eq!(presentation.target_size, expected_size);
        assert_eq!(presentation.current_position, Vec2::new(-0.5, 0.0));
        assert_eq!(presentation.target_position, Vec2::new(-0.5, 0.0));
    }

    /// Verifies that opening the message box does not itself hide the underlying terminal presentation.
    #[test]
    fn message_box_keeps_terminal_presentations_visible() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal(bridge);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let mut hud_state = crate::hud::HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);
        hud_state.open_message_box(id);

        let terminal = manager.get_mut(id).unwrap();
        terminal.snapshot.surface = Some(TerminalSurface::new(
            active_layout.0.cols,
            active_layout.0.rows,
        ));
        terminal.surface_revision = 1;

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
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
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState::default());
        world.insert_resource(view_state);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((window, PrimaryWindow));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPanel, &Visibility)>();
        let vis = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(vis.len(), 1);
        assert_eq!(*vis[0].1, Visibility::Visible);
    }

    /// Verifies that a stale isolate target no longer falls back to background presentation; with no
    /// active terminal the panel hides in place.
    #[test]
    fn isolate_visibility_policy_with_missing_terminal_hides_in_place_without_active_terminal() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id = manager.create_terminal_without_focus(bridge);
        for (_, terminal) in manager.iter_mut() {
            terminal.snapshot.surface = Some(TerminalSurface::new(2, 2));
        }

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: UVec2::new(100, 100),
                    cell_size: UVec2::new(10, 20),
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: UVec2::new(100, 100),
                    cell_size: UVec2::new(10, 20),
                },
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
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState {
            policy: crate::hud::TerminalVisibilityPolicy::Isolate(crate::terminals::TerminalId(999)),
        });
        world.insert_resource(TerminalViewState::default());
        insert_default_hud_resources(&mut world);
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..Default::default()
            },
            PrimaryWindow,
        ));
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::new(-360.0, 120.0),
                current_position: Vec2::new(25.0, -35.0),
                target_position: Vec2::new(25.0, -35.0),
                current_size: Vec2::new(210.0, 130.0),
                target_size: Vec2::new(210.0, 130.0),
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.3,
                target_z: 0.3,
            },
            Transform::from_xyz(25.0, -35.0, 0.3),
            Sprite::default(),
            Visibility::Visible,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();

        let mut query = world.query::<(&TerminalPanel, &TerminalPresentation, &Transform, &Visibility)>();
        let panels = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(panels.len(), 1);
        assert_eq!(*panels[0].3, Visibility::Hidden);
        assert_eq!(panels[0].1.current_position, Vec2::new(25.0, -35.0));
        assert_eq!(panels[0].1.target_position, Vec2::new(25.0, -35.0));
        assert_eq!(panels[0].2.translation, Vec3::new(25.0, -35.0, 0.3));
    }

    /// Verifies that projection sync creates missing panel/frame entities for terminals that exist only
    /// in authoritative terminal state.
    #[test]
    fn projection_sync_spawns_missing_terminal_entities() {
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal_without_focus(bridge);
        let mut world = World::default();
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(manager);
        world.insert_resource(TerminalPresentationStore::default());

        world
            .run_system_once(sync_terminal_projection_entities)
            .unwrap();

        let store = world.resource::<TerminalPresentationStore>();
        let presented = store
            .get(terminal_id)
            .expect("projection sync should register presentation state");
        assert_ne!(presented.panel_entity, Entity::PLACEHOLDER);
        assert_ne!(presented.frame_entity, Entity::PLACEHOLDER);
        assert_eq!(world.query::<&TerminalPanel>().iter(&world).count(), 1);
        assert_eq!(world.query::<&TerminalPanelFrame>().iter(&world).count(), 1);
    }

    /// Verifies that projection sync removes stale panel/frame entities after authoritative terminal
    /// state drops the terminal.
    #[test]
    fn projection_sync_despawns_stale_terminal_entities() {
        let (bridge, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let terminal_id = manager.create_terminal_without_focus(bridge);
        let mut world = World::default();
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(manager);
        world.insert_resource(TerminalPresentationStore::default());

        world
            .run_system_once(sync_terminal_projection_entities)
            .unwrap();
        {
            let mut manager = world.resource_mut::<TerminalManager>();
            let _ = manager.remove_terminal(terminal_id);
        }

        world
            .run_system_once(sync_terminal_projection_entities)
            .unwrap();

        assert!(world
            .resource::<TerminalPresentationStore>()
            .get(terminal_id)
            .is_none());
        assert_eq!(world.query::<&TerminalPanel>().iter(&world).count(), 0);
        assert_eq!(world.query::<&TerminalPanelFrame>().iter(&world).count(), 0);
    }

    /// Verifies the current presentation policy in `ShowAll`: even then, only the active terminal panel
    /// remains visible once focus exists.
    #[test]
    fn terminal_visibility_policy_show_all_keeps_only_active_terminal_visible() {
        // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
        let (bridge_one, _) = test_bridge();
        let (bridge_two, _) = test_bridge();
        let mut manager = TerminalManager::default();
        let id_one = manager.create_terminal(bridge_one);
        let id_two = manager.create_terminal(bridge_two);
        manager.focus_terminal(id_one);

        let window = Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        };
        let hud_state = crate::hud::HudState::default();
        let view_state = TerminalViewState::default();
        let font_state = TerminalFontState::default();
        let active_layout =
            active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

        manager.get_mut(id_one).unwrap().snapshot.surface = Some(TerminalSurface::new(
            active_layout.0.cols,
            active_layout.0.rows,
        ));
        manager.get_mut(id_one).unwrap().surface_revision = 1;
        manager.get_mut(id_two).unwrap().snapshot.surface = Some(TerminalSurface::new(2, 2));
        manager.get_mut(id_two).unwrap().surface_revision = 1;

        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            id_one,
            PresentedTerminal {
                image: Default::default(),
                texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: active_layout.1.texture_size,
                    cell_size: active_layout.1.cell_size,
                },
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
                texture_state: TerminalTextureState {
                    texture_size: UVec2::new(100, 100),
                    cell_size: UVec2::new(10, 20),
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: UVec2::new(100, 100),
                    cell_size: UVec2::new(10, 20),
                },
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
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        world.insert_resource(time);
        insert_terminal_manager_resources(&mut world, manager);
        world.insert_resource(presentation_store);
        world.insert_resource(crate::hud::TerminalVisibilityState {
            policy: crate::hud::TerminalVisibilityPolicy::Isolate(id_one),
        });
        world.insert_resource(view_state);
        insert_test_hud_state(&mut world, hud_state);
        world.spawn((window, PrimaryWindow));
        world.spawn((
            TerminalPanel { id: id_one },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));
        world.spawn((
            TerminalPanel { id: id_two },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ));

        world.run_system_once(sync_terminal_presentations).unwrap();
        {
            let manager = world.resource::<TerminalManager>();
            assert_eq!(manager.terminal_ids().len(), 2);
        }
        let mut query = world.query::<(&TerminalPanel, &Visibility)>();
        let mut vis = query
            .iter(&world)
            .map(|(panel, visibility)| (panel.id, *visibility))
            .collect::<Vec<_>>();
        vis.sort_by_key(|(id, _)| id.0);
        assert_eq!(vis[0], (id_one, Visibility::Visible));
        assert_eq!(vis[1], (id_two, Visibility::Hidden));

        world
            .resource_mut::<crate::hud::TerminalVisibilityState>()
            .policy = crate::hud::TerminalVisibilityPolicy::ShowAll;
        world.run_system_once(sync_terminal_presentations).unwrap();
        let mut query = world.query::<(&TerminalPanel, &Visibility)>();
        let mut vis = query
            .iter(&world)
            .map(|(panel, visibility)| (panel.id, *visibility))
            .collect::<Vec<_>>();
        vis.sort_by_key(|(id, _)| id.0);
        assert_eq!(vis[0], (id_one, Visibility::Visible));
        assert_eq!(vis[1], (id_two, Visibility::Hidden));
    }
}
