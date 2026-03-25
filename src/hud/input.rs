use crate::{
    hud::{
        message_box_action_at, modules, task_dialog_action_at, AgentDirectory,
        HudInputCaptureState, HudIntent, HudLayoutState, HudMessageBoxAction, HudModalState,
        HudModuleId, HudRect, HudTaskDialogAction, HUD_TITLEBAR_HEIGHT,
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

fn cursor_hud_position(window: &Window) -> Option<Vec2> {
    window.cursor_position()
}

fn content_hit_rect(module_id: HudModuleId, rect: HudRect) -> HudRect {
    if module_id == HudModuleId::AgentList {
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
    modal_state: ResMut<'w, HudModalState>,
    input_capture: Res<'w, HudInputCaptureState>,
    terminal_manager: Res<'w, TerminalManager>,
    focus_state: Res<'w, TerminalFocusState>,
    presentation_store: Res<'w, TerminalPresentationStore>,
    view_state: Res<'w, TerminalViewState>,
    agent_directory: Res<'w, AgentDirectory>,
    hud_commands: MessageWriter<'w, HudIntent>,
    redraws: MessageWriter<'w, RequestRedraw>,
}

fn message_box_task_intent(
    modal_state: &mut HudModalState,
    action: HudMessageBoxAction,
) -> Option<HudIntent> {
    let target_terminal = modal_state.message_box.target_terminal?;
    let payload = modal_state.message_box.text.trim().to_owned();
    if payload.is_empty() {
        return None;
    }
    modal_state.close_message_box_and_discard_draft();
    Some(match action {
        HudMessageBoxAction::AppendTask => HudIntent::AppendTerminalTask(target_terminal, payload),
        HudMessageBoxAction::PrependTask => {
            HudIntent::PrependTerminalTask(target_terminal, payload)
        }
    })
}

fn task_dialog_intent(
    modal_state: &mut HudModalState,
    action: HudTaskDialogAction,
) -> Option<HudIntent> {
    match action {
        HudTaskDialogAction::ClearDone => modal_state
            .task_dialog
            .target_terminal
            .map(HudIntent::ClearDoneTerminalTasks),
    }
}

pub(crate) fn handle_hud_pointer_input(mut ctx: HudPointerContext) {
    if ctx.modal_state.message_box.visible {
        ctx.layout_state.drag = None;
        let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
            return;
        };
        if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
            if let Some(action) = message_box_action_at(&ctx.primary_window, cursor) {
                if let Some(intent) = message_box_task_intent(&mut ctx.modal_state, action) {
                    ctx.hud_commands.write(intent);
                }
                ctx.redraws.write(RequestRedraw);
            }
        }
        return;
    }
    if ctx.modal_state.task_dialog.visible {
        ctx.layout_state.drag = None;
        let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
            return;
        };
        if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
            if let Some(action) = task_dialog_action_at(&ctx.primary_window, cursor) {
                if let Some(intent) = task_dialog_intent(&mut ctx.modal_state, action) {
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
            if titlebar_rect.contains(cursor) && module_id != HudModuleId::AgentList {
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
                        &ctx.agent_directory,
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
                &ctx.agent_directory,
            )
        } else {
            modules::clear_hover(module_id, &mut module.model)
        };
    }

    for command in emitted_commands {
        ctx.hud_commands.write(command);
    }
}

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

pub(crate) fn handle_hud_module_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    modal_state: Res<HudModalState>,
    input_capture: Res<HudInputCaptureState>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    mut hud_commands: MessageWriter<HudIntent>,
) {
    if modal_state.keyboard_capture_active(&input_capture) {
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
            KeyCode::Digit0 => Some(HudModuleId::DebugToolbar),
            KeyCode::Digit1 => Some(HudModuleId::AgentList),
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
