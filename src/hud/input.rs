use crate::{
    hud::{
        modules, AgentDirectory, HudIntent, HudModuleId, HudRect, HudState, HUD_TITLEBAR_HEIGHT,
    },
    terminals::{TerminalManager, TerminalPresentationStore, TerminalViewState},
};
use bevy::{
    ecs::system::SystemParam,
    input::{
        keyboard::KeyboardInput,
        mouse::{MouseScrollUnit, MouseWheel},
        ButtonState,
    },
    prelude::*,
    window::PrimaryWindow,
};

fn cursor_hud_position(window: &Window) -> Option<Vec2> {
    window.cursor_position()
}

fn content_hit_rect(rect: HudRect) -> HudRect {
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
    hud_state: ResMut<'w, HudState>,
    terminal_manager: Res<'w, TerminalManager>,
    presentation_store: Res<'w, TerminalPresentationStore>,
    view_state: Res<'w, TerminalViewState>,
    agent_directory: Res<'w, AgentDirectory>,
    hud_commands: MessageWriter<'w, HudIntent>,
}

pub(crate) fn handle_hud_pointer_input(mut ctx: HudPointerContext) {
    if ctx.hud_state.keyboard_capture_active() {
        ctx.hud_state.drag = None;
        return;
    }
    let Some(cursor) = cursor_hud_position(&ctx.primary_window) else {
        if ctx.mouse_buttons.just_released(MouseButton::Left) {
            ctx.hud_state.drag = None;
        }
        return;
    };

    let mut emitted_commands = Vec::new();

    if ctx.mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(module_id) = ctx.hud_state.topmost_enabled_at(cursor) {
            ctx.hud_state.raise_to_front(module_id);
            let titlebar_rect = ctx
                .hud_state
                .get(module_id)
                .map(|module| module.shell.titlebar_rect())
                .unwrap_or_default();
            if titlebar_rect.contains(cursor) && module_id != HudModuleId::AgentList {
                ctx.hud_state.drag = Some(crate::hud::HudDragState {
                    module_id,
                    grab_offset: Vec2::new(cursor.x - titlebar_rect.x, cursor.y - titlebar_rect.y),
                });
            } else if let Some(module) = ctx.hud_state.get(module_id) {
                let content_rect = content_hit_rect(module.shell.current_rect);
                if content_rect.contains(cursor) {
                    modules::handle_pointer_click(
                        module_id,
                        &module.model,
                        content_rect,
                        cursor,
                        &ctx.terminal_manager,
                        &ctx.presentation_store,
                        &ctx.view_state,
                        &ctx.agent_directory,
                        &ctx.hud_state,
                        &mut emitted_commands,
                    );
                }
            }
        }
    }

    if ctx.mouse_buttons.pressed(MouseButton::Left) {
        if let Some(drag) = ctx.hud_state.drag {
            if let Some(module) = ctx.hud_state.get_mut(drag.module_id) {
                module.shell.target_rect.x = cursor.x - drag.grab_offset.x;
                module.shell.target_rect.y = cursor.y - drag.grab_offset.y;
                ctx.hud_state.dirty_layout = true;
            }
        }
    }

    if ctx.mouse_buttons.just_released(MouseButton::Left) {
        ctx.hud_state.drag = None;
    }

    let scroll_delta = ctx.mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        }
    });
    if scroll_delta != 0.0 {
        if let Some(module_id) = ctx.hud_state.topmost_enabled_at(cursor) {
            if let Some(module) = ctx.hud_state.get_mut(module_id) {
                let content_rect = content_hit_rect(module.shell.current_rect);
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
        .hud_state
        .topmost_enabled_at(cursor)
        .and_then(|module_id| {
            ctx.hud_state.get(module_id).and_then(|module| {
                let content_rect = content_hit_rect(module.shell.current_rect);
                content_rect.contains(cursor).then_some(module_id)
            })
        });
    let module_ids = ctx.hud_state.iter_z_order().collect::<Vec<_>>();
    for module_id in module_ids {
        let Some(module) = ctx.hud_state.get_mut(module_id) else {
            continue;
        };
        let content_rect = content_hit_rect(module.shell.current_rect);
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

pub(crate) fn handle_hud_module_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    hud_state: Res<HudState>,
    mut hud_commands: MessageWriter<HudIntent>,
) {
    if hud_state.keyboard_capture_active() {
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
