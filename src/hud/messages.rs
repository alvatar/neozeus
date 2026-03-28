#![allow(dead_code)]

use crate::terminals::TerminalId;
use bevy::prelude::Message;

use crate::hud::HudWidgetKey;

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) enum HudIntent {
    SpawnTerminal,
    #[cfg(test)]
    SpawnShellTerminal,
    FocusTerminal(TerminalId),
    HideAllButTerminal(TerminalId),
    ShowAllTerminals,
    ToggleModule(HudWidgetKey),
    ResetModule(HudWidgetKey),
    ToggleActiveTerminalDisplayMode,
    ResetTerminalView,
    SendActiveTerminalCommand(String),
    #[cfg(test)]
    SendTerminalCommand(TerminalId, String),
    #[cfg(test)]
    SetTerminalTaskText(TerminalId, String),
    ClearDoneTerminalTasks(TerminalId),
    AppendTerminalTask(TerminalId, String),
    PrependTerminalTask(TerminalId, String),
    #[cfg(test)]
    ConsumeNextTerminalTask(TerminalId),
    #[cfg(test)]
    KillActiveTerminal,
}
