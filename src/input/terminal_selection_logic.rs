use super::*;

fn terminal_selection_point_from_cursor(
    window: &Window,
    presentation: &TerminalPresentation,
    surface: &crate::terminals::TerminalSurface,
    cursor: Vec2,
) -> TerminalSelectionPoint {
    let (min, max) = terminal_panel_screen_rect(window, presentation);
    let clamped_x = cursor.x.clamp(min.x, max.x.max(min.x + 1.0) - 1.0);
    let clamped_y = cursor.y.clamp(min.y, max.y.max(min.y + 1.0) - 1.0);
    let local_x = (clamped_x - min.x).max(0.0);
    let local_y = (clamped_y - min.y).max(0.0);
    let cell_w = (presentation.current_size.x / surface.cols.max(1) as f32).max(1.0);
    let cell_h = (presentation.current_size.y / surface.rows.max(1) as f32).max(1.0);
    TerminalSelectionPoint {
        col: ((local_x / cell_w).floor() as usize).min(surface.cols.saturating_sub(1)),
        row: ((local_y / cell_h).floor() as usize).min(surface.rows.saturating_sub(1)),
    }
}

fn viewport_point(selection_point: TerminalSelectionPoint) -> TerminalViewportPoint {
    TerminalViewportPoint {
        col: selection_point.col,
        row: selection_point.row,
    }
}

fn clear_live_terminal_selection(
    terminal_manager: &TerminalManager,
    terminal_text_selection: &mut TerminalTextSelectionState,
) {
    if let Some(terminal_id) = terminal_text_selection.live_terminal_id() {
        if let Some(bridge) = terminal_manager
            .get(terminal_id)
            .map(|terminal| &terminal.bridge)
        {
            bridge.send(TerminalCommand::ClearSelection);
        }
    }
    terminal_text_selection.clear_selection();
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal text selection needs cursor hit-testing, panel geometry, surface state, and redraws together"
)]
pub(crate) fn handle_terminal_text_selection(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    active_terminal_content: Res<ActiveTerminalContentState>,
    mut terminal_text_selection: ResMut<TerminalTextSelectionState>,
    mut agent_list_text_selection: ResMut<crate::text_selection::AgentListTextSelectionState>,
    mut redraws: MessageWriter<RequestRedraw>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
) {
    if !primary_window.focused {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        if mouse_buttons.just_released(MouseButton::Left) {
            terminal_text_selection.clear_drag();
        }
        return;
    };
    if layout_state.topmost_enabled_at(cursor).is_some() {
        if mouse_buttons.just_released(MouseButton::Left) {
            terminal_text_selection.clear_drag();
        }
        return;
    }

    let panel_hit = panels
        .iter()
        .filter(|(_, _, visibility)| **visibility == Visibility::Visible)
        .filter(|(_, presentation, _)| {
            terminal_panel_contains_cursor(&primary_window, presentation, cursor)
        })
        .max_by(|(_, left, _), (_, right, _)| left.current_z.total_cmp(&right.current_z))
        .map(|(panel, presentation, _)| (*panel, *presentation));

    if mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some((panel, presentation)) = panel_hit.as_ref() {
            if let Some(resolved_surface) = resolved_terminal_selection_surface(
                &terminal_manager,
                &active_terminal_content,
                panel.id,
            ) {
                clear_live_terminal_selection(&terminal_manager, &mut terminal_text_selection);
                agent_list_text_selection.clear_selection();
                let anchor = terminal_selection_point_from_cursor(
                    &primary_window,
                    presentation,
                    resolved_surface.surface,
                    cursor,
                );
                match resolved_surface.token {
                    crate::text_selection::TerminalSelectionSurfaceToken::Snapshot(_) => {
                        terminal_text_selection.begin_live_drag(panel.id, anchor);
                    }
                    crate::text_selection::TerminalSelectionSurfaceToken::ActiveOverride(_) => {
                        terminal_text_selection.begin_override_drag(panel.id, anchor);
                    }
                }
                redraws.write(RequestRedraw);
            }
        }
        return;
    }

    if let Some(drag) = terminal_text_selection.drag {
        if mouse_buttons.pressed(MouseButton::Left) {
            if let Some((panel, presentation)) = panel_hit.as_ref() {
                if panel.id == drag.terminal_id {
                    if let Some(resolved_surface) = resolved_terminal_selection_surface(
                        &terminal_manager,
                        &active_terminal_content,
                        panel.id,
                    ) {
                        let focus = terminal_selection_point_from_cursor(
                            &primary_window,
                            presentation,
                            resolved_surface.surface,
                            cursor,
                        );
                        match (drag.source, resolved_surface.token) {
                            (
                                TerminalTextSelectionDragSource::LiveTerminal,
                                crate::text_selection::TerminalSelectionSurfaceToken::Snapshot(_),
                            ) => {
                                if extract_terminal_selection_text(
                                    resolved_surface.surface,
                                    drag.anchor,
                                    focus,
                                )
                                .is_some()
                                {
                                    terminal_text_selection.adopt_live_selection_owner(panel.id);
                                    if let Some(bridge) = terminal_manager
                                        .get(panel.id)
                                        .map(|terminal| &terminal.bridge)
                                    {
                                        bridge.send(TerminalCommand::SetSelection {
                                            anchor: viewport_point(drag.anchor),
                                            focus: viewport_point(focus),
                                        });
                                    }
                                }
                            }
                            (
                                TerminalTextSelectionDragSource::OverrideSurface,
                                crate::text_selection::TerminalSelectionSurfaceToken::ActiveOverride(_),
                            ) => {
                                if let Some(text) = extract_terminal_selection_text(
                                    resolved_surface.surface,
                                    drag.anchor,
                                    focus,
                                ) {
                                    terminal_text_selection.set_override_selection(
                                        panel.id,
                                        drag.anchor,
                                        focus,
                                        text,
                                        resolved_surface.token,
                                    );
                                    redraws.write(RequestRedraw);
                                }
                            }
                            _ => {}
                        }
                        return;
                    }
                }
            }
        }
        if mouse_buttons.just_released(MouseButton::Left) {
            terminal_text_selection.clear_drag();
        }
    }
}

pub(crate) fn sync_primary_selection_from_ui_text_selection(
    terminal_manager: Res<TerminalManager>,
    terminal_text_selection: Res<TerminalTextSelectionState>,
    agent_list_text_selection: Res<crate::text_selection::AgentListTextSelectionState>,
    mut primary_selection: ResMut<PrimarySelectionState>,
    mut owner: ResMut<PrimarySelectionOwnerState>,
) {
    if !terminal_manager.is_changed()
        && !terminal_text_selection.is_changed()
        && !agent_list_text_selection.is_changed()
    {
        return;
    }

    let live_terminal_selection = match terminal_text_selection.owner() {
        Some(TerminalTextSelectionOwner::LiveTerminal(terminal_id)) => terminal_manager
            .get(terminal_id)
            .and_then(|terminal| terminal.snapshot.surface.as_ref())
            .and_then(|surface| surface.selected_text.as_deref())
            .map(|text| (terminal_id, text)),
        Some(TerminalTextSelectionOwner::OverrideSurface(terminal_id)) => terminal_text_selection
            .override_selection_for(terminal_id)
            .map(|selection| (terminal_id, selection.text.as_str())),
        None => None,
    };

    let changed = if let Some((terminal_id, text)) = live_terminal_selection {
        primary_selection.set_terminal_selection(terminal_id, text)
    } else if let Some(selection) = agent_list_text_selection.selection() {
        primary_selection.set_agent_list_selection(&selection.text)
    } else {
        primary_selection.clear()
    };
    if !changed {
        return;
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(text) = primary_selection.text() {
            let _ = write_linux_primary_selection_text(&mut owner, text);
        } else {
            stop_primary_selection_owner(&mut owner);
        }
    }
}
