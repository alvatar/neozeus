use crate::{
    app_config::{DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX, TERMINAL_MARGIN},
    hud::{TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        create_terminal_image, TerminalDisplayMode, TerminalHudSurfaceMarker, TerminalId,
        TerminalManager, TerminalPanel, TerminalPanelFrame, TerminalPanelSprite,
        TerminalPresentation, TerminalPresentationStore, TerminalTextureState, TerminalViewState,
    },
};
use bevy::{prelude::*, window::PrimaryWindow};

pub(crate) const HUD_SIDE_RESERVED: f32 = 72.0;
pub(crate) const HUD_TOP_RESERVED: f32 = 140.0;
pub(crate) const HUD_BOTTOM_RESERVED: f32 = 64.0;
pub(crate) const HUD_FRAME_PADDING: Vec2 = Vec2::new(18.0, 18.0);

fn terminal_home_position(slot: usize) -> Vec2 {
    const COLUMNS: usize = 3;
    const STEP_X: f32 = 360.0;
    const STEP_Y: f32 = 220.0;
    let column = slot % COLUMNS;
    let row = slot / COLUMNS;
    Vec2::new(-360.0 + column as f32 * STEP_X, 120.0 - row as f32 * STEP_Y)
}

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
    commands.spawn((
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
    ));
    commands.spawn((
        Sprite::from_image(image_handle.clone()),
        Transform::from_xyz(home_position.x, home_position.y, presentation.current_z),
        TerminalPanelSprite,
        TerminalPanel { id },
        presentation,
    ));

    presentation_store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: image_handle,
            texture_state: TerminalTextureState {
                texture_size: UVec2::ONE,
                cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
        },
    );
}

fn window_scale_factor(window: &Window) -> f32 {
    window.scale_factor().max(f32::EPSILON)
}

fn logical_to_physical_size(size: Vec2, window: &Window) -> Vec2 {
    size * window_scale_factor(window)
}

fn physical_to_logical_size(size: Vec2, window: &Window) -> Vec2 {
    size / window_scale_factor(window)
}

pub(crate) fn pixel_perfect_cell_size(cols: usize, rows: usize, window: &Window) -> UVec2 {
    let base_texture_width = (cols as u32).max(1) as f32 * DEFAULT_CELL_WIDTH_PX as f32;
    let base_texture_height = (rows as u32).max(1) as f32 * DEFAULT_CELL_HEIGHT_PX as f32;
    let fit_size_physical = logical_to_physical_size(
        Vec2::new(
            (window.width() - HUD_SIDE_RESERVED * 2.0 - HUD_FRAME_PADDING.x * 2.0).max(64.0),
            (window.height() - HUD_TOP_RESERVED - HUD_BOTTOM_RESERVED - HUD_FRAME_PADDING.y * 2.0)
                .max(64.0),
        ),
        window,
    );
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

pub(crate) fn snap_to_pixel_grid(position: Vec2, window: &Window) -> Vec2 {
    let scale_factor = window_scale_factor(window);
    (position * scale_factor).round() / scale_factor
}

fn smooth_terminal_screen_size(
    texture_state: &TerminalTextureState,
    view_state: &TerminalViewState,
    window: &Window,
) -> Vec2 {
    let texture_width = texture_state.texture_size.x.max(1) as f32;
    let texture_height = texture_state.texture_size.y.max(1) as f32;
    let fit_width = (window.width() - TERMINAL_MARGIN * 2.0).max(64.0);
    let fit_height = (window.height() - TERMINAL_MARGIN * 2.0).max(64.0);
    let fit_scale = (fit_width / texture_width).min(fit_height / texture_height);
    let zoom_scale = 10.0 / view_state.distance.max(0.1);
    Vec2::new(texture_width, texture_height) * fit_scale * zoom_scale
}

fn hud_terminal_target_position(window: &Window) -> Vec2 {
    let top = window.height() * 0.5 - HUD_TOP_RESERVED;
    let bottom = -window.height() * 0.5 + HUD_BOTTOM_RESERVED;
    snap_to_pixel_grid(Vec2::new(0.0, (top + bottom) * 0.5 - 8.0), window)
}

fn hud_surface_size(terminal_size: Vec2) -> Vec2 {
    terminal_size + HUD_FRAME_PADDING * 2.0
}

pub(crate) fn pixel_perfect_terminal_logical_size(
    texture_state: &TerminalTextureState,
    window: &Window,
) -> Vec2 {
    physical_to_logical_size(
        Vec2::new(
            texture_state.texture_size.x.max(1) as f32,
            texture_state.texture_size.y.max(1) as f32,
        ),
        window,
    )
}

pub(crate) fn terminal_texture_screen_size(
    texture_state: &TerminalTextureState,
    view_state: &TerminalViewState,
    window: &Window,
    pixel_perfect: bool,
) -> Vec2 {
    if pixel_perfect {
        return pixel_perfect_terminal_logical_size(texture_state, window);
    }

    smooth_terminal_screen_size(texture_state, view_state, window)
}

#[allow(
    clippy::too_many_arguments,
    reason = "presentation sync needs terminal/presentation/view state together"
)]
pub(crate) fn sync_terminal_presentations(
    time: Res<Time>,
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    visibility_state: Res<TerminalVisibilityState>,
    view_state: Res<TerminalViewState>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut panels: Query<(
        &TerminalPanel,
        &mut TerminalPresentation,
        &mut Transform,
        &mut Sprite,
        &mut Visibility,
    )>,
) {
    let active_id = terminal_manager.active_id();
    let background_ids = terminal_manager
        .focus_order()
        .iter()
        .copied()
        .filter(|id| Some(*id) != active_id)
        .collect::<Vec<_>>();
    let blend = 1.0 - (-time.delta_secs() * 10.0).exp();

    for (panel, mut presentation, mut transform, mut sprite, mut visibility) in &mut panels {
        if active_id.is_none() {
            *visibility = Visibility::Hidden;
            continue;
        }
        let Some(terminal) = terminal_manager.get(panel.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let Some(presented_terminal) = presentation_store.get(panel.id) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if terminal.snapshot.surface.is_none() {
            *visibility = Visibility::Hidden;
            continue;
        }
        if matches!(visibility_state.policy, TerminalVisibilityPolicy::Isolate(id) if id != panel.id)
        {
            *visibility = Visibility::Hidden;
            continue;
        }

        let smooth_size = smooth_terminal_screen_size(
            &presented_terminal.texture_state,
            &view_state,
            &primary_window,
        );
        let hud_size =
            pixel_perfect_terminal_logical_size(&presented_terminal.texture_state, &primary_window);
        let pixel_perfect = Some(panel.id) == active_id
            && presented_terminal.display_mode == TerminalDisplayMode::PixelPerfect;
        let background_rank = background_ids
            .iter()
            .position(|id| *id == panel.id)
            .unwrap_or_default() as f32;

        if Some(panel.id) == active_id {
            presentation.target_alpha = 1.0;
            if pixel_perfect {
                presentation.target_position = hud_terminal_target_position(&primary_window);
                presentation.target_size = hud_size;
                presentation.target_z = 3.0;
            } else {
                presentation.target_position = view_state.offset;
                presentation.target_size = smooth_size;
                presentation.target_z = 0.3;
            }
        } else {
            presentation.target_position = view_state.offset + presentation.home_position;
            presentation.target_size = smooth_size * 0.62;
            presentation.target_alpha = 0.84;
            presentation.target_z = -0.05 - background_rank * 0.02;
        }

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

        *visibility = Visibility::Visible;
        sprite.custom_size = Some(presentation.current_size.max(Vec2::ONE));
        sprite.color = Color::WHITE;
        transform.translation = presentation.current_position.extend(presentation.current_z);
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;
    }
}

pub(crate) fn sync_terminal_panel_frames(
    hud_state: Res<crate::hud::HudState>,
    panels: Query<
        (&TerminalPanel, &TerminalPresentation, &Visibility),
        Without<TerminalPanelFrame>,
    >,
    mut frames: Query<
        (
            &TerminalPanelFrame,
            &mut Transform,
            &mut Sprite,
            &mut Visibility,
        ),
        Without<TerminalPanel>,
    >,
) {
    for (frame, mut transform, mut sprite, mut visibility) in &mut frames {
        let Some(target_terminal) = hud_state.direct_input_terminal else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let Some((_, presentation, panel_visibility)) =
            panels.iter().find(|(panel, _, _)| panel.id == frame.id)
        else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if target_terminal != frame.id || *panel_visibility != Visibility::Visible {
            *visibility = Visibility::Hidden;
            continue;
        }

        *visibility = Visibility::Visible;
        sprite.custom_size = Some((presentation.current_size + Vec2::splat(18.0)).max(Vec2::ONE));
        sprite.color = Color::srgba(1.0, 0.48, 0.08, 0.96);
        transform.translation = presentation
            .current_position
            .extend(presentation.current_z - 0.02);
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;
    }
}

pub(crate) fn sync_terminal_hud_surface(
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    visibility_state: Res<TerminalVisibilityState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation)>,
    mut hud_surface: Single<
        (&mut Transform, &mut Sprite, &mut Visibility),
        With<TerminalHudSurfaceMarker>,
    >,
) {
    let (transform, sprite, visibility) = &mut *hud_surface;
    let Some(active_id) = terminal_manager.active_id() else {
        **visibility = Visibility::Hidden;
        return;
    };
    let Some(terminal) = terminal_manager.get(active_id) else {
        **visibility = Visibility::Hidden;
        return;
    };
    if matches!(visibility_state.policy, TerminalVisibilityPolicy::Isolate(id) if id != active_id) {
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
    let Some((_, presentation)) = panels.iter().find(|(panel, _)| panel.id == active_id) else {
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
