use crate::terminals::TerminalId;

use super::state::HudModuleId;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HudCommand {
    SpawnTerminal,
    FocusTerminal(TerminalId),
    HideAllButTerminal(TerminalId),
    ShowAllTerminals,
    ToggleModule(HudModuleId),
    ResetModule(HudModuleId),
    ToggleActiveTerminalDisplayMode,
    ResetTerminalView,
    SendActiveTerminalCommand(String),
    SendTerminalCommand(TerminalId, String),
    KillActiveTerminal,
}
