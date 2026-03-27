use crate::{
    app::AppSessionState,
    hud::{
        message_box_action_at, modules, task_dialog_action_at, AgentListView, ConversationListView,
        HudInputCaptureState, HudIntent, HudLayoutState, HudMessageBoxAction, HudRect,
        HudTaskDialogAction, HudWidgetKey, ThreadView, HUD_TITLEBAR_HEIGHT,
    },
    terminals::{
        TerminalFocusState, TerminalId, TerminalManager, TerminalPresentationStore,
        TerminalViewState,
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
    terminal_manager: Res<'w, TerminalManager>,
    focus_state: Res<'w, TerminalFocusState>,
    presentation_store: Res<'w, TerminalPresentationStore>,
    view_state: Res<'w, TerminalViewState>,
    agent_list_view: Res<'w, AgentListView>,
    conversation_list_view: Res<'w, ConversationListView>,
    thread_view: Res<'w, ThreadView>,
    hud_commands: MessageWriter<'w, HudIntent>,
    redraws: MessageWriter<'w, RequestRedraw>,
}

/// Converts a message-box task action button click into the corresponding task-edit intent.
///
/// The payload comes from the current message-box text, trimmed and rejected if empty. Successful
/// conversion also closes the message box and discards its draft because the text has been consumed
/// into a task mutation.
fn message_box_task_intent(
    composer: &mut crate::ui::ComposerState,
    action: HudMessageBoxAction,
) -> Option<HudIntent> {
    let target_terminal = composer.message_editor.target_terminal?;
    let payload = composer.message_editor.text.trim().to_owned();
    if payload.is_empty() {
        return None;
    }
    composer.discard_current_message();
    Some(match action {
        HudMessageBoxAction::AppendTask => HudIntent::AppendTerminalTask(target_terminal, payload),
        HudMessageBoxAction::PrependTask => {
            HudIntent::PrependTerminalTask(target_terminal, payload)
        }
    })
}

/// Converts a task-dialog action button click into the corresponding HUD intent.
///
/// Today the only task-dialog action is `ClearDone`, which requires a bound target terminal to emit
/// an intent.
fn task_dialog_intent(
    composer: &mut crate::ui::ComposerState,
    action: HudTaskDialogAction,
) -> Option<HudIntent> {
    match action {
        HudTaskDialogAction::ClearDone => composer
            .task_editor
            .target_terminal
            .map(HudIntent::ClearDoneTerminalTasks),
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
                if let Some(intent) = message_box_task_intent(&mut ctx.app_session.composer, action)
                {
                    ctx.hud_commands.write(intent);
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
                if let Some(intent) = task_dialog_intent(&mut ctx.app_session.composer, action) {
                    ctx.hud_commands.write(intent);
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
                        &module.model,
                        content_rect,
                        cursor,
                        &ctx.terminal_manager,
                        &ctx.focus_state,
                        &ctx.presentation_store,
                        &ctx.view_state,
                        &ctx.agent_list_view,
                        &ctx.conversation_list_view,
                        &ctx.thread_view,
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
                        &mut module.model,
                        scroll_delta,
                        &ctx.terminal_manager,
                        content_rect,
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
        let Some(module) = ctx.layout_state.get_mut(module_id) else {
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
                &mut module.model,
                content_rect,
                point,
                &ctx.terminal_manager,
                &ctx.focus_state,
                &ctx.agent_list_view,
                &ctx.conversation_list_view,
            )
        } else {
            modules::clear_hover(module_id, &mut module.model)
        };
    }

    for command in emitted_commands {
        ctx.hud_commands.write(command);
    }
}

/// Chooses the next or previous terminal id for keyboard navigation through the agent list.
///
/// When no terminal is active yet, the function picks the first or last terminal depending on the
/// navigation direction. Once a terminal is active, movement is clamped to the list bounds rather than
/// wrapping.
fn adjacent_agent_terminal_id(
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    step: isize,
) -> Option<TerminalId> {
    let terminal_ids = terminal_manager.terminal_ids();
    if terminal_ids.is_empty() {
        return None;
    }

    let current_index = match focus_state.active_id() {
        Some(active_id) => terminal_ids.iter().position(|id| *id == active_id)?,
        None => {
            return Some(if step < 0 {
                *terminal_ids.last()?
            } else {
                terminal_ids[0]
            });
        }
    };

    let next_index = if step < 0 {
        current_index.saturating_sub(step.unsigned_abs())
    } else {
        current_index
            .saturating_add(step as usize)
            .min(terminal_ids.len().saturating_sub(1))
    };
    (next_index != current_index).then_some(terminal_ids[next_index])
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
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    mut hud_commands: MessageWriter<HudIntent>,
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
                    adjacent_agent_terminal_id(&terminal_manager, &focus_state, 1)
                }
                KeyCode::KeyK | KeyCode::ArrowUp => {
                    adjacent_agent_terminal_id(&terminal_manager, &focus_state, -1)
                }
                _ => None,
            };
            if let Some(terminal_id) = navigation_target {
                hud_commands.write(HudIntent::FocusTerminal(terminal_id));
                hud_commands.write(HudIntent::HideAllButTerminal(terminal_id));
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
        hud_commands.write(match action {
            ShortcutAction::Toggle => HudIntent::ToggleModule(module_id),
            ShortcutAction::Reset => HudIntent::ResetModule(module_id),
        });
    }
}
