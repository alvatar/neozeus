use crate::{
    hud::{
        HudLayoutState, HudModuleId, HudRect, HUD_BUTTON_GAP, HUD_BUTTON_HEIGHT,
        HUD_BUTTON_MIN_WIDTH, HUD_MODULE_PADDING,
    },
    terminals::{
        TerminalDisplayMode, TerminalManager, TerminalPresentationStore, TerminalViewState,
    },
};

use super::{DebugToolbarAction, DebugToolbarButton};

pub(crate) fn debug_toolbar_buttons(
    shell_rect: HudRect,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
    _view_state: &TerminalViewState,
    layout_state: &HudLayoutState,
) -> Vec<DebugToolbarButton> {
    let active_display_mode = presentation_store
        .active_display_mode(terminal_manager.active_id())
        .unwrap_or(TerminalDisplayMode::Smooth);
    let toolbar_enabled = layout_state
        .get(HudModuleId::DebugToolbar)
        .map(|module| module.shell.enabled)
        .unwrap_or(true);
    let agent_list_enabled = layout_state
        .get(HudModuleId::AgentList)
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
            active_display_mode == TerminalDisplayMode::PixelPerfect,
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
            DebugToolbarAction::ToggleModule(HudModuleId::DebugToolbar),
            toolbar_enabled,
        ),
        (
            "1 agents".to_owned(),
            DebugToolbarAction::ToggleModule(HudModuleId::AgentList),
            agent_list_enabled,
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
