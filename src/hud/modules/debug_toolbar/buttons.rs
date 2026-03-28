use super::super::super::state::{
    HudLayoutState, HudRect, HUD_BUTTON_GAP, HUD_BUTTON_HEIGHT, HUD_BUTTON_MIN_WIDTH,
    HUD_MODULE_PADDING,
};
use super::super::super::view_models::DebugToolbarView;
use super::super::super::widgets::HudWidgetKey;

use super::{DebugToolbarAction, DebugToolbarButton};

/// Builds the retained button list for the debug toolbar module.
pub(in crate::hud) fn debug_toolbar_buttons(
    shell_rect: HudRect,
    debug_toolbar_view: &DebugToolbarView,
    layout_state: &HudLayoutState,
) -> Vec<DebugToolbarButton> {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let toolbar_enabled = layout_state
        .get(HudWidgetKey::DebugToolbar)
        .map(|module| module.shell.enabled)
        .unwrap_or(true);
    let agent_list_enabled = layout_state
        .get(HudWidgetKey::AgentList)
        .map(|module| module.shell.enabled)
        .unwrap_or(false);
    let conversation_list_enabled = layout_state
        .get(HudWidgetKey::ConversationList)
        .map(|module| module.shell.enabled)
        .unwrap_or(false);
    let thread_pane_enabled = layout_state
        .get(HudWidgetKey::ThreadPane)
        .map(|module| module.shell.enabled)
        .unwrap_or(false);

    let buttons = vec![
        (
            "new terminal".to_owned(),
            DebugToolbarAction::SpawnTerminal,
            false,
        ),
        ("show all".to_owned(), DebugToolbarAction::ShowAll, false),
        (
            "pixel perfect".to_owned(),
            DebugToolbarAction::TogglePixelPerfect,
            debug_toolbar_view.pixel_perfect_active,
        ),
        (
            "reset view".to_owned(),
            DebugToolbarAction::ResetView,
            false,
        ),
        (
            "pwd".to_owned(),
            DebugToolbarAction::SendCommand("pwd"),
            false,
        ),
        (
            "ls".to_owned(),
            DebugToolbarAction::SendCommand("ls"),
            false,
        ),
        (
            "clear".to_owned(),
            DebugToolbarAction::SendCommand("clear"),
            false,
        ),
        (
            "btop".to_owned(),
            DebugToolbarAction::SendCommand("btop"),
            false,
        ),
        (
            "tmux".to_owned(),
            DebugToolbarAction::SendCommand("tmux"),
            false,
        ),
        (
            "0 toolbar".to_owned(),
            DebugToolbarAction::ToggleModule(HudWidgetKey::DebugToolbar),
            toolbar_enabled,
        ),
        (
            "1 agents".to_owned(),
            DebugToolbarAction::ToggleModule(HudWidgetKey::AgentList),
            agent_list_enabled,
        ),
        (
            "2 convs".to_owned(),
            DebugToolbarAction::ToggleModule(HudWidgetKey::ConversationList),
            conversation_list_enabled,
        ),
        (
            "3 thread".to_owned(),
            DebugToolbarAction::ToggleModule(HudWidgetKey::ThreadPane),
            thread_pane_enabled,
        ),
    ];

    let mut cursor_x = shell_rect.x + HUD_MODULE_PADDING;
    let y = shell_rect.y + HUD_MODULE_PADDING;
    buttons
        .into_iter()
        .map(|(label, action, active)| {
            let width = HUD_BUTTON_MIN_WIDTH.max(label.len() as f32 * 8.0 + 20.0);
            let rect = HudRect {
                x: cursor_x,
                y,
                w: width,
                h: HUD_BUTTON_HEIGHT,
            };
            cursor_x += width + HUD_BUTTON_GAP;
            DebugToolbarButton {
                label,
                rect,
                action,
                active,
            }
        })
        .collect()
}

/// Test-only wrapper that builds toolbar buttons and maps them into the simplified test view type.
#[cfg(test)]
pub(crate) fn test_debug_toolbar_buttons(
    shell_rect: HudRect,
    debug_toolbar_view: &DebugToolbarView,
    layout_state: &HudLayoutState,
) -> Vec<super::DebugToolbarButtonTestView> {
    debug_toolbar_buttons(shell_rect, debug_toolbar_view, layout_state)
        .into_iter()
        .map(|button| super::DebugToolbarButtonTestView {
            label: button.label,
            rect: button.rect,
            active: button.active,
        })
        .collect()
}
