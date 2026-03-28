use crate::{
    app::AppSessionState,
    app::{AgentCommand, AppCommand, TaskCommand, WidgetCommand},
    hud::{
        message_box_action_at, modules, task_dialog_action_at, AgentListUiState, AgentListView,
        ConversationListUiState, ConversationListView, DebugToolbarView, HudInputCaptureState,
        HudLayoutState, HudMessageBoxAction, HudRect, HudTaskDialogAction, HudWidgetKey,
        HUD_TITLEBAR_HEIGHT,
    },
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
    if module_id == HudWidgetKey::AgentList {
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
pub(crate) struct HudPointerContext<'w, 's> {
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    mouse_wheel: MessageReader<'w, 's, MouseWheel>,
    layout_state: ResMut<'w, HudLayoutState>,
    app_session: ResMut<'w, AppSessionState>,
    input_capture: Res<'w, HudInputCaptureState>,
    agent_list_state: ResMut<'w, AgentListUiState>,
    conversation_list_state: ResMut<'w, ConversationListUiState>,
    debug_toolbar_view: Res<'w, DebugToolbarView>,
    agent_list_view: Res<'w, AgentListView>,
    conversation_list_view: Res<'w, ConversationListView>,
    app_commands: MessageWriter<'w, AppCommand>,
    redraws: MessageWriter<'w, RequestRedraw>,
}

/// Converts a message-box task action button click into the corresponding task-edit intent.
///
/// The payload comes from the current message-box text, trimmed and rejected if empty. Successful
/// conversion also closes the message box and discards its draft because the text has been consumed
/// into a task mutation.
fn message_box_task_command(
    composer: &mut crate::ui::ComposerState,
    action: HudMessageBoxAction,
) -> Option<AppCommand> {
    let agent_id = match composer.session.as_ref().map(|session| &session.mode) {
        Some(crate::ui::ComposerMode::Message { agent_id }) => *agent_id,
        _ => return None,
    };
    let payload = composer.message_editor.text.trim().to_owned();
    if payload.is_empty() {
        return None;
    }
    composer.discard_current_message();
    Some(AppCommand::Task(match action {
        HudMessageBoxAction::AppendTask => TaskCommand::Append {
            agent_id,
            text: payload,
        },
        HudMessageBoxAction::PrependTask => TaskCommand::Prepend {
            agent_id,
            text: payload,
        },
    }))
}

/// Converts a task-dialog action button click into the corresponding HUD intent.
///
/// Today the only task-dialog action is `ClearDone`, which requires a bound target terminal to emit
/// an intent.
fn task_dialog_command(
    composer: &mut crate::ui::ComposerState,
    action: HudTaskDialogAction,
) -> Option<AppCommand> {
    let agent_id = match composer.session.as_ref().map(|session| &session.mode) {
        Some(crate::ui::ComposerMode::TaskEdit { agent_id }) => *agent_id,
        _ => return None,
    };
    match action {
        HudTaskDialogAction::ClearDone => {
            Some(AppCommand::Task(TaskCommand::ClearDone { agent_id }))
        }
    }
}

/// Handles all pointer-driven HUD interaction: modal buttons, module clicks, dragging, scrolling,
/// and hover state.
///
/// The function is ordered by capture priority. Visible modals get first chance at the pointer, then
/// direct terminal input suppresses HUD interaction entirely, and only then does ordinary module
/// interaction run. Within normal interaction it handles click dispatch, titlebar dragging, scroll
/// routing, and per-module hover updates.
pub(crate) fn handle_hud_pointer_input(mut ctx: HudPointerContext) {
    if ctx.app_session.composer.message_editor.visible {
        ctx.layout_state.drag = None;
        let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
            return;
        };
        if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
            if let Some(action) = message_box_action_at(&ctx.primary_window, cursor) {
                if let Some(command) =
                    message_box_task_command(&mut ctx.app_session.composer, action)
                {
                    ctx.app_commands.write(command);
                }
                ctx.redraws.write(RequestRedraw);
            }
        }
        return;
    }
    if ctx.app_session.composer.task_editor.visible {
        ctx.layout_state.drag = None;
        let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
            return;
        };
        if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
            if let Some(action) = task_dialog_action_at(&ctx.primary_window, cursor) {
                if let Some(command) = task_dialog_command(&mut ctx.app_session.composer, action) {
                    ctx.app_commands.write(command);
                }
                ctx.redraws.write(RequestRedraw);
            }
        }
        return;
    }
    if ctx.input_capture.direct_input_terminal.is_some() {
        ctx.layout_state.drag = None;
        return;
    }
    let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
        if ctx.mouse_buttons.just_released(MouseButton::Left) {
            ctx.layout_state.drag = None;
        }
        return;
    };

    let mut emitted_commands = Vec::new();

    if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(module_id) = ctx.layout_state.topmost_enabled_at(cursor) {
            ctx.layout_state.raise_to_front(module_id);
            let titlebar_rect = ctx
                .layout_state
                .get(module_id)
                .map(|module| module.shell.titlebar_rect())
                .unwrap_or_default();
            if titlebar_rect.contains(cursor) && module_id != HudWidgetKey::AgentList {
                ctx.layout_state.drag = Some(crate::hud::HudDragState {
                    module_id,
                    grab_offset: Vec2::new(cursor.x - titlebar_rect.x, cursor.y - titlebar_rect.y),
                });
            } else if let Some(module) = ctx.layout_state.get(module_id) {
                let content_rect = content_hit_rect(module_id, module.shell.current_rect);
                if content_rect.contains(cursor) {
                    modules::handle_pointer_click(
                        module_id,
                        content_rect,
                        cursor,
                        &ctx.agent_list_state,
                        &ctx.conversation_list_state,
                        &ctx.agent_list_view,
                        &ctx.conversation_list_view,
                        &ctx.debug_toolbar_view,
                        &ctx.layout_state,
                        &mut emitted_commands,
                    );
                }
            }
        }
    }

    if ctx.mouse_buttons.pressed(MouseButton::Left) {
        if let Some(drag) = ctx.layout_state.drag {
            if let Some(module) = ctx.layout_state.get_mut(drag.module_id) {
                module.shell.target_rect.x = cursor.x - drag.grab_offset.x;
                module.shell.target_rect.y = cursor.y - drag.grab_offset.y;
                ctx.layout_state.dirty_layout = true;
            }
        }
    }

    if ctx.mouse_buttons.just_released(MouseButton::Left) {
        ctx.layout_state.drag = None;
    }

    let scroll_delta = ctx.mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        }
    });
    if scroll_delta != 0.0 {
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

    for command in emitted_commands {
        ctx.app_commands.write(command);
    }
}

/// Chooses the next or previous agent id for keyboard navigation through the agent list.
fn adjacent_agent_id(
    app_session: &AppSessionState,
    agent_list_view: &AgentListView,
    step: isize,
) -> Option<crate::agents::AgentId> {
    if agent_list_view.rows.is_empty() {
        return None;
    }

    let current_index = match app_session.active_agent {
        Some(active_agent) => agent_list_view
            .rows
            .iter()
            .position(|row| row.agent_id == active_agent)?,
        None => {
            return Some(if step < 0 {
                agent_list_view.rows.last()?.agent_id
            } else {
                agent_list_view.rows[0].agent_id
            });
        }
    };

    let next_index = if step < 0 {
        current_index.saturating_sub(step.unsigned_abs())
    } else {
        current_index
            .saturating_add(step as usize)
            .min(agent_list_view.rows.len().saturating_sub(1))
    };
    (next_index != current_index).then_some(agent_list_view.rows[next_index].agent_id)
}

/// Handles keyboard shortcuts that toggle/reset HUD modules and navigate the agent list.
///
/// Plain digit keys toggle modules, `Alt+Shift+digit` resets them, and plain `j`/`k` or up/down
/// arrows navigate between terminals by emitting focus+isolate intents. All of it is suppressed while
/// any modal/editor state owns keyboard capture.
pub(crate) fn handle_hud_module_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    app_session: Res<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    agent_list_view: Res<AgentListView>,
    mut app_commands: MessageWriter<AppCommand>,
) {
    if app_session.composer.keyboard_capture_active(&input_capture) {
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

    for event in messages.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        if matches!(action, ShortcutAction::Toggle) {
            let navigation_target = match event.key_code {
                KeyCode::KeyJ | KeyCode::ArrowDown => {
                    adjacent_agent_id(&app_session, &agent_list_view, 1)
                }
                KeyCode::KeyK | KeyCode::ArrowUp => {
                    adjacent_agent_id(&app_session, &agent_list_view, -1)
                }
                _ => None,
            };
            if let Some(agent_id) = navigation_target {
                app_commands.write(AppCommand::Agent(AgentCommand::Focus(agent_id)));
                app_commands.write(AppCommand::Agent(AgentCommand::Inspect(agent_id)));
                continue;
            }
        }

        let module_id = match event.key_code {
            KeyCode::Digit0 => Some(HudWidgetKey::DebugToolbar),
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
