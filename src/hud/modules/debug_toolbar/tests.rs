use super::*;
use crate::{
    app::{AgentCommand as AppAgentCommand, AppCommand},
    hud::{DebugToolbarView, HudRect, HudState, HudWidgetKey},
};
use bevy::prelude::*;

/// Verifies that clicking the debug-toolbar `new terminal` button emits the spawn-terminal intent.
#[test]
fn clicking_debug_toolbar_button_emits_spawn_terminal_command() {
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut emitted_commands = Vec::new();
    let buttons = test_debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        &DebugToolbarView::default(),
        &hud_state.layout_state(),
    );
    let new_terminal = buttons
        .iter()
        .find(|button| button.label == "new terminal")
        .expect("new terminal button missing");
    let click_point = Vec2::new(
        new_terminal.rect.x + new_terminal.rect.w * 0.5,
        new_terminal.rect.y + new_terminal.rect.h * 0.5,
    );

    handle_pointer_click(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        click_point,
        &DebugToolbarView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(
        emitted_commands,
        vec![AppCommand::Agent(AppAgentCommand::SpawnTerminal)]
    );
}

/// Verifies that clicking a debug-toolbar command button emits the corresponding active-terminal
/// command intent.
#[test]
fn clicking_debug_toolbar_command_button_emits_terminal_command() {
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut emitted_commands = Vec::new();
    let buttons = test_debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        &DebugToolbarView::default(),
        &hud_state.layout_state(),
    );
    let pwd = buttons
        .iter()
        .find(|button| button.label == "pwd")
        .expect("pwd button missing");
    let click_point = Vec2::new(pwd.rect.x + pwd.rect.w * 0.5, pwd.rect.y + pwd.rect.h * 0.5);

    handle_pointer_click(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        click_point,
        &DebugToolbarView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(
        emitted_commands,
        vec![AppCommand::Terminal(
            crate::app::TerminalCommand::SendCommandToActive {
                command: "pwd".into(),
            }
        )]
    );
}

/// Verifies that the debug toolbar exposes explicit toggle buttons for the known HUD modules.
#[test]
fn debug_toolbar_buttons_include_module_toggle_entries() {
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let buttons = test_debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 24.0,
            w: 920.0,
            h: 64.0,
        },
        &DebugToolbarView::default(),
        &hud_state.layout_state(),
    );
    assert!(buttons.iter().any(|button| button.label == "0 toolbar"));
    assert!(buttons.iter().any(|button| button.label == "1 agents"));
    assert!(buttons.iter().any(|button| button.label == "2 convs"));
    assert!(buttons.iter().any(|button| button.label == "3 thread"));
}

/// Verifies that debug-toolbar module toggle buttons mirror each module's current enabled state.
#[test]
fn debug_toolbar_module_toggle_buttons_reflect_enabled_state() {
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    hud_state.set_module_enabled(HudWidgetKey::AgentList, false);

    let buttons = test_debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 24.0,
            w: 920.0,
            h: 64.0,
        },
        &DebugToolbarView::default(),
        &hud_state.layout_state(),
    );

    let toolbar = buttons
        .iter()
        .find(|button| button.label == "0 toolbar")
        .expect("toolbar toggle button missing");
    let agents = buttons
        .iter()
        .find(|button| button.label == "1 agents")
        .expect("agent toggle button missing");
    let conversations = buttons
        .iter()
        .find(|button| button.label == "2 convs")
        .expect("conversation toggle button missing");
    let thread = buttons
        .iter()
        .find(|button| button.label == "3 thread")
        .expect("thread toggle button missing");
    assert!(toolbar.active);
    assert!(!agents.active);
    assert!(!conversations.active);
    assert!(!thread.active);
}
