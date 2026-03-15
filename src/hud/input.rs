use crate::{
    hud::{
        modules, AgentDirectory, HudCommand, HudDispatcher, HudModuleId, HudRect, HudState,
        HUD_TITLEBAR_HEIGHT,
    },
    terminals::{TerminalManager, TerminalPresentationStore, TerminalViewState},
};
use bevy::{
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

#[allow(
    clippy::too_many_arguments,
    reason = "HUD pointer routing needs window input, HUD state, terminal state, and dispatcher together"
)]
pub(crate) fn handle_hud_pointer_input(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut hud_state: ResMut<HudState>,
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    view_state: Res<TerminalViewState>,
    agent_directory: Res<AgentDirectory>,
    mut dispatcher: ResMut<HudDispatcher>,
) {
    let Some(cursor) = cursor_hud_position(&primary_window) else {
        if mouse_buttons.just_released(MouseButton::Left) {
            hud_state.drag = None;
        }
        return;
    };

    if mouse_buttons.just_pressed(MouseButton::Left) {
        if let Some(module_id) = hud_state.topmost_enabled_at(cursor) {
            hud_state.raise_to_front(module_id);
            let titlebar_rect = hud_state
                .get(module_id)
                .map(|module| module.shell.titlebar_rect())
                .unwrap_or_default();
            if titlebar_rect.contains(cursor) {
                hud_state.drag = Some(crate::hud::HudDragState {
                    module_id,
                    grab_offset: Vec2::new(cursor.x - titlebar_rect.x, cursor.y - titlebar_rect.y),
                });
            } else if let Some(module) = hud_state.get(module_id) {
                let content_rect = content_hit_rect(module.shell.current_rect);
                if content_rect.contains(cursor) {
                    modules::handle_pointer_click(
                        module_id,
                        &module.model,
                        content_rect,
                        cursor,
                        &terminal_manager,
                        &presentation_store,
                        &view_state,
                        &agent_directory,
                        &mut dispatcher,
                    );
                }
            }
        }
    }

    if mouse_buttons.pressed(MouseButton::Left) {
        if let Some(drag) = hud_state.drag {
            if let Some(module) = hud_state.get_mut(drag.module_id) {
                module.shell.target_rect.x = cursor.x - drag.grab_offset.x;
                module.shell.target_rect.y = cursor.y - drag.grab_offset.y;
                hud_state.dirty_layout = true;
            }
        }
    }

    if mouse_buttons.just_released(MouseButton::Left) {
        hud_state.drag = None;
    }

    let scroll_delta = mouse_wheel.read().fold(0.0, |acc, event| {
        acc + match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        }
    });
    if scroll_delta != 0.0 {
        if let Some(module_id) = hud_state.topmost_enabled_at(cursor) {
            if let Some(module) = hud_state.get_mut(module_id) {
                let content_rect = content_hit_rect(module.shell.current_rect);
                if content_rect.contains(cursor) {
                    modules::handle_scroll(
                        module_id,
                        &mut module.model,
                        scroll_delta,
                        &terminal_manager,
                        content_rect,
                    );
                }
            }
        }
    }
}

pub(crate) fn handle_hud_module_shortcuts(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut dispatcher: ResMut<HudDispatcher>,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if !alt {
        return;
    }

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
        dispatcher
            .commands
            .push(HudCommand::ToggleModule(module_id));
    }
}
