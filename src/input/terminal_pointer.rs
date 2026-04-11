use super::*;

/// Tests whether a window-space cursor position lies inside a terminal panel's current on-screen
/// rectangle.
///
/// Terminal presentations are stored around the scene center, while the cursor arrives in window
/// coordinates with an upper-left origin. The function converts the panel's centered presentation
/// rectangle into the same window coordinate system and then performs a simple bounds check.
pub(super) fn terminal_panel_screen_rect(
    window: &Window,
    presentation: &TerminalPresentation,
) -> (Vec2, Vec2) {
    let min = Vec2::new(
        window.width() * 0.5 + presentation.current_position.x - presentation.current_size.x * 0.5,
        window.height() * 0.5 - presentation.current_position.y - presentation.current_size.y * 0.5,
    );
    let max = min + presentation.current_size;
    (min, max)
}

pub(super) fn terminal_panel_contains_cursor(
    window: &Window,
    presentation: &TerminalPresentation,
    cursor: Vec2,
) -> bool {
    let (min, max) = terminal_panel_screen_rect(window, presentation);
    cursor.x >= min.x && cursor.x <= max.x && cursor.y >= min.y && cursor.y <= max.y
}

/// Finds the frontmost visible terminal panel under the cursor.
///
/// The query is filtered in three steps: hidden panels are ignored, the cursor must land inside the
/// panel rectangle, and ties are resolved by the current presentation `z` so clicking overlapping
/// panels always targets the one visually on top.
pub(super) fn topmost_terminal_panel_at_cursor(
    window: &Window,
    panels: &Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    cursor: Vec2,
) -> Option<TerminalPanel> {
    panels
        .iter()
        .filter(|(_, _, visibility)| **visibility == Visibility::Visible)
        .filter(|(_, presentation, _)| terminal_panel_contains_cursor(window, presentation, cursor))
        .max_by(|(_, left, _), (_, right, _)| left.current_z.total_cmp(&right.current_z))
        .map(|(panel, _, _)| *panel)
}

/// Turns a left-click on a visible terminal panel into focus + isolate intents.
///
/// The system deliberately refuses to act while a modal is open, while the window is unfocused, or
/// when the click lands on a HUD module. Only genuine background clicks on a terminal panel are
/// promoted into `FocusTerminal` and `HideAllButTerminal` intents.
#[allow(
    clippy::too_many_arguments,
    reason = "pointer focus routing now consults explicit input ownership before translating clicks into intents"
)]
pub(crate) fn focus_terminal_on_panel_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    runtime_index: Res<AgentRuntimeIndex>,
    mut app_commands: MessageWriter<AppCommand>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if app_session.modal_input_owner(&input_capture)
        || !mouse_buttons.just_pressed(MouseButton::Left)
        || !primary_window.focused
    {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return;
    }
    let Some(panel) = topmost_terminal_panel_at_cursor(&primary_window, &panels, cursor) else {
        return;
    };
    let Some(agent_id) = runtime_index.agent_for_terminal(panel.id) else {
        return;
    };
    app_commands.write(AppCommand::Agent(AppAgentCommand::Inspect(agent_id)));
}

/// Clears terminal focus when the user clicks on empty background space.
///
/// This is the inverse of panel focusing: if the click is not blocked by a modal, does not hit a HUD
/// module, and does not land on any visible terminal panel, the active terminal is cleared. The
/// function also resets visibility to `ShowAll`, clears per-terminal view focus, reconciles direct
/// input capture, and marks session persistence dirty so the unfocused state can be saved.
#[allow(
    clippy::too_many_arguments,
    reason = "background-click clear needs input, focus, visibility, view, and persistence resources together"
)]
pub(crate) fn hide_terminal_on_background_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    focus_state: Res<TerminalFocusState>,
    mut app_commands: MessageWriter<AppCommand>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    if app_session.modal_input_owner(&input_capture)
        || !mouse_buttons.just_pressed(MouseButton::Left)
        || !primary_window.focused
    {
        return;
    }
    let Some(_) = focus_state.active_id() else {
        return;
    };
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return;
    }
    if topmost_terminal_panel_at_cursor(&primary_window, &panels, cursor).is_some() {
        return;
    }
    app_commands.write(AppCommand::Agent(AppAgentCommand::ClearFocus));
}

/// Handles middle-mouse dragging for either viewport panning or terminal scrollback.
///
/// The mode split is deliberate:
/// - `Shift + middle-drag` pans the presented terminal by mutating the view offset directly.
/// - plain `middle-drag` is translated into line-based scrollback commands sent to the active
///   terminal bridge.
///
/// For scrollback, the function converts pixel motion into logical terminal lines using the current
/// presented cell height and carries sub-line remainder in [`TerminalPointerState`] so slow drags do
/// not lose precision.
#[allow(
    clippy::too_many_arguments,
    reason = "mouse drag needs input, geometry, pointer state, and terminal bridge"
)]
pub(crate) fn drag_terminal_view(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    presentation_store: Res<TerminalPresentationStore>,
    layout_state: Res<HudLayoutState>,
    mut view_state: ResMut<TerminalViewState>,
    mut pointer_state: ResMut<TerminalPointerState>,
) {
    // Aggregate all motion events for the frame so drag behavior is framerate-independent.
    let delta = mouse_motion
        .read()
        .fold(Vec2::ZERO, |acc, event| acc + event.delta);

    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let middle_pressed = mouse_buttons.pressed(MouseButton::Middle);
    if !primary_window.focused || !middle_pressed || delta == Vec2::ZERO {
        pointer_state.scroll_drag_remainder_px = 0.0;
        return;
    }

    if shift {
        pointer_state.scroll_drag_remainder_px = 0.0;
        view_state.apply_offset_delta(focus_state.active_id(), Vec2::new(delta.x, -delta.y));
        return;
    }

    let Some(texture_state) = presentation_store.active_texture_state(focus_state.active_id())
    else {
        pointer_state.scroll_drag_remainder_px = 0.0;
        return;
    };
    let pixel_perfect = presentation_store.active_display_mode(focus_state.active_id())
        == Some(TerminalDisplayMode::PixelPerfect);
    let screen_size = terminal_texture_screen_size(
        texture_state,
        &view_state,
        &primary_window,
        &layout_state,
        pixel_perfect,
    );
    let screen_cell_height = if texture_state.cell_size.y == 0 || texture_state.texture_size.y == 0
    {
        1.0
    } else {
        screen_size.y * (texture_state.cell_size.y as f32 / texture_state.texture_size.y as f32)
    }
    .max(1.0);

    // Keep fractional drag distance between frames so one slow drag across multiple frames still
    // eventually produces the correct number of scroll lines.
    pointer_state.scroll_drag_remainder_px += delta.y;
    let lines = (-pointer_state.scroll_drag_remainder_px / screen_cell_height).trunc() as i32;
    if lines != 0 {
        pointer_state.scroll_drag_remainder_px += lines as f32 * screen_cell_height;
        if let Some(bridge) = focus_state.active_bridge(&terminal_manager) {
            bridge.send(TerminalCommand::ScrollDisplay(lines));
        }
    }
}

/// Routes ordinary mouse-wheel input into terminal scrollback for either the focused terminal or the
/// direct-input target terminal.
///
/// `Shift + wheel` is reserved for zoom and is ignored here. HUD-owned regions are also ignored so
/// module scrolling keeps priority when the cursor is over HUD content.
const TERMINAL_PAGE_SCROLL_FALLBACK_ROWS: i32 = 20;

pub(super) fn terminal_page_scroll_rows(
    terminal_manager: &TerminalManager,
    terminal_id: TerminalId,
) -> i32 {
    terminal_manager
        .get(terminal_id)
        .and_then(|terminal| terminal.snapshot.surface.as_ref())
        .map(|surface| surface.rows.saturating_sub(1))
        .filter(|rows| *rows > 0)
        .and_then(|rows| i32::try_from(rows).ok())
        .unwrap_or(TERMINAL_PAGE_SCROLL_FALLBACK_ROWS)
}

#[allow(
    clippy::too_many_arguments,
    reason = "wheel scrolling needs keyboard, window focus, HUD hit-testing, terminal routing, and pointer remainder state together"
)]
pub(crate) fn scroll_terminal_with_mouse_wheel(
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    input_capture: Res<HudInputCaptureState>,
    mut pointer_state: ResMut<TerminalPointerState>,
    mut mouse_wheel: MessageReader<MouseWheel>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !primary_window.focused || shift {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return;
    }

    let target_terminal = input_capture
        .direct_input_terminal
        .or_else(|| focus_state.active_id());
    let Some(bridge) = target_terminal
        .and_then(|terminal_id| terminal_manager.get(terminal_id))
        .map(|terminal| &terminal.bridge)
    else {
        return;
    };

    let wheel_delta_lines = mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 24.0,
        }
    });
    if wheel_delta_lines == 0.0 {
        return;
    }

    pointer_state.wheel_scroll_remainder_lines += wheel_delta_lines;
    let lines = pointer_state.wheel_scroll_remainder_lines.trunc() as i32;
    if lines == 0 {
        return;
    }
    pointer_state.wheel_scroll_remainder_lines -= lines as f32;
    bridge.send(TerminalCommand::ScrollDisplay(lines));
}

/// Applies shift-wheel zoom to the shared terminal view distance.
///
/// Only focused-window `Shift + wheel` input is treated as zoom. Mouse-wheel units are normalized to
/// a common scale and then applied to `view_state.distance`, which is clamped so the camera cannot be
/// zoomed into nonsense or pushed arbitrarily far away.
pub(crate) fn zoom_terminal_view(
    keys: Res<ButtonInput<KeyCode>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut view_state: ResMut<TerminalViewState>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !primary_window.focused || !shift {
        return;
    }

    let zoom_delta = mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 24.0,
        }
    });

    if zoom_delta == 0.0 {
        return;
    }

    view_state.distance = (view_state.distance - zoom_delta * 0.8).clamp(2.0, 40.0);
}
