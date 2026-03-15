use crate::{
    hud::{
        render::{HudColors, HudPainter, HudRenderInputs},
        HudCommand, HudDispatcher, HudModuleId, HudModuleModel, HudRect, HudState, HUD_BUTTON_GAP,
        HUD_BUTTON_HEIGHT, HUD_BUTTON_MIN_WIDTH, HUD_MODULE_PADDING,
    },
    terminals::{
        TerminalDisplayMode, TerminalManager, TerminalPresentationStore, TerminalViewState,
    },
};
use bevy::prelude::Vec2;
use bevy_vello::prelude::VelloTextAnchor;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DebugToolbarAction {
    SpawnTerminal,
    ShowAll,
    TogglePixelPerfect,
    ResetView,
    SendCommand(&'static str),
    ToggleModule(HudModuleId),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DebugToolbarButton {
    pub(crate) label: String,
    pub(crate) rect: HudRect,
    pub(crate) action: DebugToolbarAction,
    pub(crate) active: bool,
}

pub(crate) fn debug_toolbar_buttons(
    shell_rect: HudRect,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
    _view_state: &TerminalViewState,
    hud_state: &HudState,
) -> Vec<DebugToolbarButton> {
    let active_display_mode = presentation_store
        .active_display_mode(terminal_manager.active_id())
        .unwrap_or(TerminalDisplayMode::Smooth);
    let toolbar_enabled = hud_state
        .get(HudModuleId::DebugToolbar)
        .map(|module| module.shell.enabled)
        .unwrap_or(true);
    let agent_list_enabled = hud_state
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

#[allow(
    clippy::too_many_arguments,
    reason = "toolbar hit routing needs geometry, terminal state, HUD state, and dispatcher together"
)]
pub(crate) fn render_content(
    model: &HudModuleModel,
    content_rect: HudRect,
    painter: &mut HudPainter,
    inputs: &HudRenderInputs,
) {
    let HudModuleModel::DebugToolbar(_) = model else {
        return;
    };
    let buttons = debug_toolbar_buttons(
        content_rect,
        inputs.terminal_manager,
        inputs.presentation_store,
        inputs.view_state,
        inputs.hud_state,
    );
    let active_status = inputs
        .terminal_manager
        .active_snapshot()
        .map(|snapshot| snapshot.runtime.status.as_str())
        .unwrap_or("no active terminal");
    let active_id = inputs
        .terminal_manager
        .active_id()
        .map(|id| id.0)
        .unwrap_or_default();
    let debug_stats = inputs.terminal_manager.active_debug_stats();
    let font_summary = match inputs.font_state.report.as_ref() {
        Some(Ok(report)) => format!("font {}", report.primary.family),
        Some(Err(error)) => format!("font error {error}"),
        None => "font loading".to_owned(),
    };

    painter.label(
        Vec2::new(content_rect.x, content_rect.y + HUD_BUTTON_HEIGHT + 8.0),
        &format!(
            "terms {} · active {} · {} · zoom {:.2}",
            inputs.terminal_manager.terminal_ids().len(),
            active_id,
            active_status,
            inputs.view_state.distance,
        ),
        14.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(
            content_rect.x + 430.0,
            content_rect.y + HUD_BUTTON_HEIGHT + 8.0,
        ),
        &font_summary,
        14.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(
            content_rect.x + 620.0,
            content_rect.y + HUD_BUTTON_HEIGHT + 8.0,
        ),
        &format!(
            "keys {} drop {} rows {}",
            debug_stats.key_events_seen,
            debug_stats.updates_dropped,
            debug_stats.dirty_rows_uploaded,
        ),
        14.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    for button in buttons {
        painter.fill_rect(
            button.rect,
            if button.active {
                HudColors::BUTTON_ACTIVE
            } else {
                HudColors::BUTTON
            },
            6.0,
        );
        painter.stroke_rect(button.rect, HudColors::BUTTON_BORDER, 6.0);
        painter.label(
            Vec2::new(button.rect.x + 10.0, button.rect.y + 6.0),
            &button.label,
            14.0,
            HudColors::TEXT,
            VelloTextAnchor::TopLeft,
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "toolbar hit routing needs geometry, terminal state, HUD state, and dispatcher together"
)]
pub(crate) fn handle_pointer_click(
    model: &HudModuleModel,
    shell_rect: HudRect,
    point: Vec2,
    terminal_manager: &TerminalManager,
    presentation_store: &TerminalPresentationStore,
    view_state: &TerminalViewState,
    hud_state: &HudState,
    dispatcher: &mut HudDispatcher,
) {
    if !matches!(model, HudModuleModel::DebugToolbar(_)) {
        return;
    }
    for button in debug_toolbar_buttons(
        shell_rect,
        terminal_manager,
        presentation_store,
        view_state,
        hud_state,
    ) {
        if !button.rect.contains(point) {
            continue;
        }
        match button.action {
            DebugToolbarAction::SpawnTerminal => {
                dispatcher.commands.push(HudCommand::SpawnTerminal)
            }
            DebugToolbarAction::ShowAll => dispatcher.commands.push(HudCommand::ShowAllTerminals),
            DebugToolbarAction::TogglePixelPerfect => dispatcher
                .commands
                .push(HudCommand::ToggleActiveTerminalDisplayMode),
            DebugToolbarAction::ResetView => {
                dispatcher.commands.push(HudCommand::ResetTerminalView)
            }
            DebugToolbarAction::SendCommand(command) => dispatcher
                .commands
                .push(HudCommand::SendActiveTerminalCommand(command.to_owned())),
            DebugToolbarAction::ToggleModule(id) => {
                dispatcher.commands.push(HudCommand::ToggleModule(id));
            }
        }
        break;
    }
}
