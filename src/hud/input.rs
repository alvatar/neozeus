use crate::{
    app::{
        AgentCommand, AppCommand, AppSessionState, CreateAgentDialogField, RenameAgentDialogField,
        WidgetCommand,
    },
    composer::{
        create_agent_dialog_target_at, message_box_action_at, rename_agent_dialog_target_at,
        task_dialog_action_at, CreateAgentDialogTarget, RenameAgentDialogTarget,
    },
    text_selection::AgentListTextSelectionState,
};

use super::{
    modules,
    state::{
        AgentListUiState, ConversationListUiState, HudDragState, HudInputCaptureState,
        HudLayoutState, HudRect, HUD_TITLEBAR_HEIGHT,
    },
    view_models::{AgentListRowKey, AgentListView, ConversationListView, InfoBarView},
    widgets::HudWidgetKey,
};
use bevy::{
    ecs::system::SystemParam,
    input::{
        keyboard::KeyboardInput,
        mouse::{MouseScrollUnit, MouseWheel},
        ButtonState,
    },
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};

/// Returns the cursor position in HUD space.
///
/// HUD space currently matches the primary window's cursor coordinate system directly, so this helper
/// is intentionally tiny and exists mainly to keep the rest of the input code phrased in HUD terms.
fn cursor_hud_position(window: &Window) -> Option<Vec2> {
    window.cursor_position()
}

/// Computes the rectangle that should respond to module-content interactions.
///
/// Most modules reserve their titlebar for dragging and only expose the area below it as interactive
/// content. The agent list is the exception: its whole shell is interactive, so it keeps the full
/// rectangle.
fn content_hit_rect(module_id: HudWidgetKey, rect: HudRect) -> HudRect {
    if matches!(module_id, HudWidgetKey::AgentList | HudWidgetKey::InfoBar) {
        return rect;
    }
    HudRect {
        x: rect.x,
        y: rect.y + HUD_TITLEBAR_HEIGHT.min(rect.h),
        w: rect.w,
        h: (rect.h - HUD_TITLEBAR_HEIGHT.min(rect.h)).max(0.0),
    }
}

#[derive(SystemParam)]
struct HudPointerContext<'w, 's> {
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    mouse_wheel: MessageReader<'w, 's, MouseWheel>,
    layout_state: ResMut<'w, HudLayoutState>,
    app_session: ResMut<'w, AppSessionState>,
    input_capture: Res<'w, HudInputCaptureState>,
    agent_list_state: ResMut<'w, AgentListUiState>,
    agent_list_text_selection: ResMut<'w, AgentListTextSelectionState>,
    conversation_list_state: ResMut<'w, ConversationListUiState>,
    info_bar_view: Res<'w, InfoBarView>,
    agent_list_view: Res<'w, AgentListView>,
    conversation_list_view: Res<'w, ConversationListView>,
    app_commands: MessageWriter<'w, AppCommand>,
    redraws: MessageWriter<'w, RequestRedraw>,
}

/// Handles all pointer-driven HUD interaction: modal buttons, module clicks, dragging, scrolling,
/// and hover state.
///
/// The function is ordered by capture priority. Visible modals get first chance at the pointer, then
/// direct terminal input suppresses HUD interaction entirely, and only then does ordinary module
/// interaction run. Within normal interaction it handles click dispatch, titlebar dragging, scroll
/// routing, and per-module hover updates.
pub(crate) fn handle_hud_pointer_input(world: &mut World) {
    let mut state: bevy::ecs::system::SystemState<HudPointerContext> =
        bevy::ecs::system::SystemState::new(world);
    {
        let mut ctx = state.get_mut(world);
        handle_hud_pointer_input_with_context(&mut ctx);
    }
    state.apply(world);
}

fn handle_hud_pointer_input_with_context(ctx: &mut HudPointerContext<'_, '_>) {
    if handle_create_agent_dialog_pointer(ctx)
        || handle_rename_agent_dialog_pointer(ctx)
        || handle_message_dialog_pointer(ctx)
        || handle_task_dialog_pointer(ctx)
        || handle_direct_input_pointer_capture(ctx)
    {
        return;
    }
    handle_general_hud_pointer(ctx);
}

fn handle_create_agent_dialog_pointer(ctx: &mut HudPointerContext<'_, '_>) -> bool {
    if !ctx.app_session.create_agent_dialog.visible {
        return false;
    }
    ctx.layout_state.drag = None;
    let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
        return true;
    };
    if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(target) = create_agent_dialog_target_at(&ctx.primary_window, cursor) {
            match target {
                CreateAgentDialogTarget::NameField => {
                    ctx.app_session.create_agent_dialog.focus = CreateAgentDialogField::Name;
                    ctx.app_session
                        .create_agent_dialog
                        .cwd_field
                        .clear_completion();
                    ctx.app_session.create_agent_dialog.error = None;
                }
                CreateAgentDialogTarget::Kind(kind) => {
                    ctx.app_session.create_agent_dialog.focus = CreateAgentDialogField::Kind;
                    ctx.app_session
                        .create_agent_dialog
                        .cwd_field
                        .clear_completion();
                    ctx.app_session.create_agent_dialog.set_kind(kind);
                }
                CreateAgentDialogTarget::StartingFolderField => {
                    ctx.app_session.create_agent_dialog.focus =
                        CreateAgentDialogField::StartingFolder;
                    ctx.app_session.create_agent_dialog.error = None;
                }
                CreateAgentDialogTarget::CreateButton => {
                    ctx.app_session.create_agent_dialog.focus =
                        CreateAgentDialogField::CreateButton;
                    ctx.app_session
                        .create_agent_dialog
                        .cwd_field
                        .clear_completion();
                    if let Some(command) =
                        ctx.app_session.create_agent_dialog.build_create_command()
                    {
                        ctx.app_commands.write(command);
                    }
                }
            }
            ctx.redraws.write(RequestRedraw);
        }
    }
    true
}

fn handle_rename_agent_dialog_pointer(ctx: &mut HudPointerContext<'_, '_>) -> bool {
    if !ctx.app_session.rename_agent_dialog.visible {
        return false;
    }
    ctx.layout_state.drag = None;
    let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
        return true;
    };
    if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(target) = rename_agent_dialog_target_at(&ctx.primary_window, cursor) {
            match target {
                RenameAgentDialogTarget::NameField => {
                    ctx.app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
                    ctx.app_session.rename_agent_dialog.error = None;
                }
                RenameAgentDialogTarget::RenameButton => {
                    ctx.app_session.rename_agent_dialog.focus =
                        RenameAgentDialogField::RenameButton;
                    if let Some(command) =
                        ctx.app_session.rename_agent_dialog.build_rename_command()
                    {
                        ctx.app_commands.write(command);
                    }
                }
            }
            ctx.redraws.write(RequestRedraw);
        }
    }
    true
}

fn handle_message_dialog_pointer(ctx: &mut HudPointerContext<'_, '_>) -> bool {
    if !ctx.app_session.composer.message_editor.visible {
        return false;
    }
    ctx.layout_state.drag = None;
    let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
        return true;
    };
    if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(action) = message_box_action_at(&ctx.primary_window, cursor) {
            if let Some(command) = ctx.app_session.composer.message_box_action_command(action) {
                ctx.app_commands.write(command);
            }
            ctx.redraws.write(RequestRedraw);
        }
    }
    true
}

fn handle_task_dialog_pointer(ctx: &mut HudPointerContext<'_, '_>) -> bool {
    if !ctx.app_session.composer.task_editor.visible {
        return false;
    }
    ctx.layout_state.drag = None;
    let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
        return true;
    };
    if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(action) = task_dialog_action_at(&ctx.primary_window, cursor) {
            if let Some(command) = ctx.app_session.composer.task_dialog_action_command(action) {
                ctx.app_commands.write(command);
            }
            ctx.redraws.write(RequestRedraw);
        }
    }
    true
}

fn handle_direct_input_pointer_capture(ctx: &mut HudPointerContext<'_, '_>) -> bool {
    if ctx.input_capture.direct_input_terminal.is_none() {
        return false;
    }
    ctx.layout_state.drag = None;
    true
}

fn handle_general_hud_pointer(ctx: &mut HudPointerContext<'_, '_>) {
    let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
        clear_pointer_state_when_cursor_missing(ctx);
        return;
    };

    let mut emitted_commands = Vec::new();
    handle_general_left_click(ctx, cursor, &mut emitted_commands);
    handle_general_drag_update(ctx, cursor, &mut emitted_commands);
    handle_general_release(ctx, cursor, &mut emitted_commands);
    handle_general_scroll(ctx, cursor);
    handle_general_hover(ctx, cursor);
    emit_app_commands(ctx, emitted_commands);
}

fn clear_pointer_state_when_cursor_missing(ctx: &mut HudPointerContext<'_, '_>) {
    if ctx.mouse_buttons.just_released(MouseButton::Left) {
        ctx.layout_state.drag = None;
        ctx.agent_list_state.drag.clear();
        ctx.agent_list_text_selection.clear_drag();
    }
}

fn handle_general_left_click(
    ctx: &mut HudPointerContext<'_, '_>,
    cursor: Vec2,
    emitted_commands: &mut Vec<AppCommand>,
) {
    if !ctx.mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(module_id) = ctx.layout_state.topmost_enabled_at(cursor) else {
        return;
    };
    ctx.layout_state.raise_to_front(module_id);
    let titlebar_rect = ctx
        .layout_state
        .get(module_id)
        .map(|module| module.shell.titlebar_rect())
        .unwrap_or_default();
    if titlebar_rect.contains(cursor)
        && !matches!(module_id, HudWidgetKey::AgentList | HudWidgetKey::InfoBar)
    {
        ctx.layout_state.drag = Some(HudDragState {
            module_id,
            grab_offset: Vec2::new(cursor.x - titlebar_rect.x, cursor.y - titlebar_rect.y),
        });
        return;
    }
    let Some(module) = ctx.layout_state.get(module_id) else {
        return;
    };
    let content_rect = content_hit_rect(module_id, module.shell.current_rect);
    if !content_rect.contains(cursor) {
        return;
    }
    if module_id == HudWidgetKey::AgentList {
        if let Some(text_row) = modules::text_row_at_point(
            &ctx.agent_list_state,
            content_rect,
            cursor,
            &ctx.agent_list_view,
        ) {
            ctx.agent_list_text_selection.clear_selection();
            ctx.agent_list_text_selection.begin_drag(text_row, cursor);
            ctx.agent_list_state.drag.clear();
            return;
        }
        let pressed_row = modules::row_at_point(
            &ctx.agent_list_state,
            content_rect,
            cursor,
            &ctx.agent_list_view,
        );
        ctx.agent_list_state.drag.pressed_row = pressed_row.clone();
        ctx.agent_list_state.drag.pressed_agent = match pressed_row {
            Some(AgentListRowKey::Agent(agent_id)) => Some(agent_id),
            _ => None,
        };
        ctx.agent_list_state.drag.press_origin = Some(cursor);
        ctx.agent_list_state.drag.dragging_agent = None;
        ctx.agent_list_state.drag.drag_cursor = None;
        ctx.agent_list_state.drag.drag_grab_offset_y = ctx
            .agent_list_state
            .drag
            .pressed_agent
            .and_then(|agent_id| {
                modules::agent_rows(
                    content_rect,
                    ctx.agent_list_state.scroll_offset,
                    ctx.agent_list_state.hovered_row.as_ref(),
                    &ctx.agent_list_view,
                )
                .into_iter()
                .find(|row| !row.is_tmux_child() && row.owner_agent_id() == Some(agent_id))
                .map(|row| cursor.y - row.rect.y)
            })
            .unwrap_or(0.0);
        ctx.agent_list_state.drag.last_reorder_index = ctx
            .agent_list_state
            .drag
            .pressed_agent
            .and_then(|agent_id| {
                ctx.agent_list_view
                    .rows
                    .iter()
                    .position(|row| row.key == AgentListRowKey::Agent(agent_id))
            });
        return;
    }
    modules::handle_pointer_click(
        module_id,
        content_rect,
        cursor,
        &ctx.agent_list_state,
        &ctx.conversation_list_state,
        &ctx.agent_list_view,
        &ctx.conversation_list_view,
        &ctx.info_bar_view,
        &ctx.layout_state,
        emitted_commands,
    );
}

fn handle_general_drag_update(
    ctx: &mut HudPointerContext<'_, '_>,
    cursor: Vec2,
    emitted_commands: &mut Vec<AppCommand>,
) {
    if !ctx.mouse_buttons.pressed(MouseButton::Left) {
        return;
    }
    if let Some(drag) = ctx.agent_list_text_selection.drag.clone() {
        if let Some(module) = ctx.layout_state.get(HudWidgetKey::AgentList) {
            let content_rect = content_hit_rect(HudWidgetKey::AgentList, module.shell.current_rect);
            let focus_row = modules::text_row_at_point(
                &ctx.agent_list_state,
                content_rect,
                cursor,
                &ctx.agent_list_view,
            )
            .unwrap_or_else(|| drag.anchor_row.clone());
            let moved_far_enough = drag.press_origin.distance(cursor) >= 4.0;
            if focus_row != drag.anchor_row || moved_far_enough {
                if let Some(text) = modules::selected_text_for_rows(
                    &ctx.agent_list_view,
                    &drag.anchor_row,
                    &focus_row,
                ) {
                    ctx.agent_list_text_selection
                        .set_selection(drag.anchor_row, focus_row, text);
                    ctx.redraws.write(RequestRedraw);
                }
            }
        }
        return;
    }
    if let Some(pressed_agent) = ctx.agent_list_state.drag.pressed_agent {
        let moved_far_enough = ctx
            .agent_list_state
            .drag
            .press_origin
            .is_some_and(|origin| origin.distance(cursor) >= 4.0);
        if moved_far_enough && ctx.agent_list_state.drag.dragging_agent.is_none() {
            ctx.agent_list_state.drag.dragging_agent = Some(pressed_agent);
            ctx.agent_list_state.drag.drag_cursor = Some(cursor);
            ctx.redraws.write(RequestRedraw);
        }
        if let Some(dragging_agent) = ctx.agent_list_state.drag.dragging_agent {
            ctx.agent_list_state.drag.drag_cursor = Some(cursor);
            if let Some(module) = ctx.layout_state.get(HudWidgetKey::AgentList) {
                let content_rect =
                    content_hit_rect(HudWidgetKey::AgentList, module.shell.current_rect);
                if let Some(target_index) = modules::reorder_target_index(
                    &ctx.agent_list_state,
                    content_rect,
                    cursor,
                    &ctx.agent_list_view,
                ) {
                    if ctx.agent_list_state.drag.last_reorder_index != Some(target_index) {
                        emitted_commands.push(AppCommand::Agent(AgentCommand::Reorder {
                            agent_id: dragging_agent,
                            target_index,
                        }));
                        ctx.agent_list_state.drag.last_reorder_index = Some(target_index);
                        ctx.redraws.write(RequestRedraw);
                    }
                }
            }
        }
        return;
    }
    if let Some(drag) = ctx.layout_state.drag {
        if let Some(module) = ctx.layout_state.get_mut(drag.module_id) {
            module.shell.target_rect.x = cursor.x - drag.grab_offset.x;
            module.shell.target_rect.y = cursor.y - drag.grab_offset.y;
            ctx.layout_state.dirty_layout = true;
        }
    }
}

fn handle_general_release(
    ctx: &mut HudPointerContext<'_, '_>,
    cursor: Vec2,
    emitted_commands: &mut Vec<AppCommand>,
) {
    if !ctx.mouse_buttons.just_released(MouseButton::Left) {
        return;
    }
    if let Some(drag) = ctx.agent_list_text_selection.drag.clone() {
        ctx.agent_list_text_selection.clear_drag();
        if let Some(module) = ctx.layout_state.get(HudWidgetKey::AgentList) {
            let content_rect = content_hit_rect(HudWidgetKey::AgentList, module.shell.current_rect);
            let focus_row = modules::text_row_at_point(
                &ctx.agent_list_state,
                content_rect,
                cursor,
                &ctx.agent_list_view,
            )
            .unwrap_or_else(|| drag.anchor_row.clone());
            let moved_far_enough = drag.press_origin.distance(cursor) >= 4.0;
            if focus_row != drag.anchor_row || moved_far_enough {
                if let Some(text) = modules::selected_text_for_rows(
                    &ctx.agent_list_view,
                    &drag.anchor_row,
                    &focus_row,
                ) {
                    ctx.agent_list_text_selection
                        .set_selection(drag.anchor_row, focus_row, text);
                    ctx.redraws.write(RequestRedraw);
                }
            } else {
                ctx.agent_list_text_selection.clear_selection();
                match drag.anchor_row {
                    AgentListRowKey::Agent(agent_id)
                        if modules::row_at_point(
                            &ctx.agent_list_state,
                            content_rect,
                            cursor,
                            &ctx.agent_list_view,
                        ) == Some(AgentListRowKey::Agent(agent_id)) =>
                    {
                        emitted_commands.push(AppCommand::OwnedTmux(
                            crate::app::OwnedTmuxCommand::ClearSelection,
                        ));
                        emitted_commands.push(AppCommand::Agent(AgentCommand::Focus(agent_id)));
                        emitted_commands.push(AppCommand::Agent(AgentCommand::Inspect(agent_id)));
                    }
                    AgentListRowKey::OwnedTmux(session_uid)
                        if modules::row_at_point(
                            &ctx.agent_list_state,
                            content_rect,
                            cursor,
                            &ctx.agent_list_view,
                        ) == Some(AgentListRowKey::OwnedTmux(session_uid.clone())) =>
                    {
                        emitted_commands.push(AppCommand::OwnedTmux(
                            crate::app::OwnedTmuxCommand::Select { session_uid },
                        ));
                    }
                    _ => {}
                }
            }
        }
        ctx.layout_state.drag = None;
        return;
    }
    if let Some(pressed_row) = ctx.agent_list_state.drag.pressed_row.clone() {
        let was_dragging = ctx.agent_list_state.drag.dragging_agent.is_some();
        ctx.agent_list_state.drag.clear();
        if !was_dragging {
            if let Some(module) = ctx.layout_state.get(HudWidgetKey::AgentList) {
                let content_rect =
                    content_hit_rect(HudWidgetKey::AgentList, module.shell.current_rect);
                if modules::row_at_point(
                    &ctx.agent_list_state,
                    content_rect,
                    cursor,
                    &ctx.agent_list_view,
                ) == Some(pressed_row.clone())
                {
                    match pressed_row {
                        AgentListRowKey::Agent(agent_id) => {
                            emitted_commands.push(AppCommand::OwnedTmux(
                                crate::app::OwnedTmuxCommand::ClearSelection,
                            ));
                            emitted_commands.push(AppCommand::Agent(AgentCommand::Focus(agent_id)));
                            emitted_commands
                                .push(AppCommand::Agent(AgentCommand::Inspect(agent_id)));
                        }
                        AgentListRowKey::OwnedTmux(session_uid) => {
                            emitted_commands.push(AppCommand::OwnedTmux(
                                crate::app::OwnedTmuxCommand::Select { session_uid },
                            ));
                        }
                    }
                }
            }
        }
    }
    ctx.layout_state.drag = None;
}

fn handle_general_scroll(ctx: &mut HudPointerContext<'_, '_>, cursor: Vec2) {
    let scroll_delta = ctx.mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        }
    });
    if scroll_delta == 0.0 {
        return;
    }
    if let Some(module_id) = ctx.layout_state.topmost_enabled_at(cursor) {
        if let Some(module) = ctx.layout_state.get_mut(module_id) {
            let content_rect = content_hit_rect(module_id, module.shell.current_rect);
            if content_rect.contains(cursor) {
                modules::handle_scroll(
                    module_id,
                    scroll_delta,
                    content_rect,
                    &mut ctx.agent_list_state,
                    &mut ctx.conversation_list_state,
                    &ctx.agent_list_view,
                    &ctx.conversation_list_view,
                );
            }
        }
    }
}

fn handle_general_hover(ctx: &mut HudPointerContext<'_, '_>, cursor: Vec2) {
    let hovered_module_id = ctx
        .layout_state
        .topmost_enabled_at(cursor)
        .and_then(|module_id| {
            ctx.layout_state.get(module_id).and_then(|module| {
                let content_rect = content_hit_rect(module_id, module.shell.current_rect);
                content_rect.contains(cursor).then_some(module_id)
            })
        });
    let module_ids = ctx.layout_state.iter_z_order().collect::<Vec<_>>();
    for module_id in module_ids {
        let Some(module) = ctx.layout_state.get(module_id) else {
            continue;
        };
        let content_rect = content_hit_rect(module_id, module.shell.current_rect);
        let point = if hovered_module_id == Some(module_id) {
            Some(cursor)
        } else {
            None
        };
        let _ = if point.is_some() {
            modules::handle_hover(
                module_id,
                content_rect,
                point,
                &mut ctx.agent_list_state,
                &mut ctx.conversation_list_state,
                &ctx.agent_list_view,
                &ctx.conversation_list_view,
            )
        } else {
            modules::clear_hover(
                module_id,
                &mut ctx.agent_list_state,
                &mut ctx.conversation_list_state,
            )
        };
    }
}

fn emit_app_commands(ctx: &mut HudPointerContext<'_, '_>, emitted_commands: Vec<AppCommand>) {
    for command in emitted_commands {
        ctx.app_commands.write(command);
    }
}

enum AgentListNavigationTarget {
    Agent(crate::agents::AgentId),
    OwnedTmux(String),
}

/// Chooses the next or previous visible row target for keyboard navigation through the agent list.
fn adjacent_agent_list_target(
    app_session: &AppSessionState,
    agent_list_view: &AgentListView,
    active_terminal_content: &crate::terminals::ActiveTerminalContentState,
    step: isize,
) -> Option<AgentListNavigationTarget> {
    if agent_list_view.rows.is_empty() {
        return None;
    }

    let current_index = if let Some(session_uid) =
        active_terminal_content.selected_owned_tmux_session_uid()
    {
        agent_list_view.rows.iter().position(
            |row| matches!(&row.key, AgentListRowKey::OwnedTmux(row_uid) if row_uid == session_uid),
        )?
    } else if let Some(active_agent) = app_session.active_agent {
        agent_list_view.rows.iter().position(
            |row| matches!(row.key, AgentListRowKey::Agent(agent_id) if agent_id == active_agent),
        )?
    } else {
        return match agent_list_view.rows[if step < 0 {
            agent_list_view.rows.len() - 1
        } else {
            0
        }]
        .key
        .clone()
        {
            AgentListRowKey::Agent(agent_id) => Some(AgentListNavigationTarget::Agent(agent_id)),
            AgentListRowKey::OwnedTmux(session_uid) => {
                Some(AgentListNavigationTarget::OwnedTmux(session_uid))
            }
        };
    };

    let next_index = if step < 0 {
        current_index.saturating_sub(step.unsigned_abs())
    } else {
        current_index
            .saturating_add(step as usize)
            .min(agent_list_view.rows.len().saturating_sub(1))
    };
    if next_index == current_index {
        return None;
    }

    match agent_list_view.rows[next_index].key.clone() {
        AgentListRowKey::Agent(agent_id) => Some(AgentListNavigationTarget::Agent(agent_id)),
        AgentListRowKey::OwnedTmux(session_uid) => {
            Some(AgentListNavigationTarget::OwnedTmux(session_uid))
        }
    }
}

/// Handles keyboard shortcuts that toggle/reset HUD modules and navigate the agent list.
///
/// Plain digit keys toggle modules, `Alt+Shift+digit` resets them, and plain `j`/`k` or up/down
/// arrows walk the visible mixed agent/tmux rows. Agent rows emit focus/inspect commands; tmux rows
/// emit tmux-selection commands. All of it is suppressed while any modal/editor state owns keyboard
/// capture.
pub(crate) fn handle_hud_module_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    agent_list_view: Res<AgentListView>,
    active_terminal_content: Option<Res<crate::terminals::ActiveTerminalContentState>>,
    mut app_commands: MessageWriter<AppCommand>,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    if app_session.keyboard_capture_active(&input_capture) {
        return;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let super_key = keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    enum ShortcutAction {
        Toggle,
        Reset,
    }

    let action = if !ctrl && !alt && !super_key && !shift {
        Some(ShortcutAction::Toggle)
    } else if !ctrl && alt && !super_key && shift {
        Some(ShortcutAction::Reset)
    } else {
        None
    };
    let Some(action) = action else {
        return;
    };
    let default_active_terminal_content = crate::terminals::ActiveTerminalContentState::default();
    let active_terminal_content = active_terminal_content
        .as_deref()
        .unwrap_or(&default_active_terminal_content);

    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        if matches!(action, ShortcutAction::Toggle) {
            let navigation_target = match event.key_code {
                KeyCode::KeyJ | KeyCode::ArrowDown => Some(adjacent_agent_list_target(
                    &app_session,
                    &agent_list_view,
                    active_terminal_content,
                    1,
                )),
                KeyCode::KeyK | KeyCode::ArrowUp => Some(adjacent_agent_list_target(
                    &app_session,
                    &agent_list_view,
                    active_terminal_content,
                    -1,
                )),
                _ => None,
            }
            .flatten();
            if let Some(target) = navigation_target {
                match target {
                    AgentListNavigationTarget::Agent(agent_id) => {
                        app_commands.write(AppCommand::Agent(AgentCommand::Focus(agent_id)));
                        app_commands.write(AppCommand::Agent(AgentCommand::Inspect(agent_id)));
                    }
                    AgentListNavigationTarget::OwnedTmux(session_uid) => {
                        app_commands.write(AppCommand::OwnedTmux(
                            crate::app::OwnedTmuxCommand::Select { session_uid },
                        ));
                    }
                }
                continue;
            }
        }

        let module_id = match event.key_code {
            KeyCode::Digit0 => Some(HudWidgetKey::InfoBar),
            KeyCode::Digit1 => Some(HudWidgetKey::AgentList),
            KeyCode::Digit2 => Some(HudWidgetKey::ConversationList),
            KeyCode::Digit3 => Some(HudWidgetKey::ThreadPane),
            _ => None,
        };
        let Some(module_id) = module_id else {
            continue;
        };
        app_commands.write(match action {
            ShortcutAction::Toggle => AppCommand::Widget(WidgetCommand::Toggle(module_id)),
            ShortcutAction::Reset => AppCommand::Widget(WidgetCommand::Reset(module_id)),
        });
    }
}
