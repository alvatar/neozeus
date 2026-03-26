use crate::terminals::TerminalId;
use bevy::prelude::Message;

use super::state::HudModuleId;

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) enum HudIntent {
    SpawnTerminal,
    SpawnShellTerminal,
    FocusTerminal(TerminalId),
    HideAllButTerminal(TerminalId),
    ShowAllTerminals,
    ToggleModule(HudModuleId),
    ResetModule(HudModuleId),
    ToggleActiveTerminalDisplayMode,
    ResetTerminalView,
    SendActiveTerminalCommand(String),
    SendTerminalCommand(TerminalId, String),
    SetTerminalTaskText(TerminalId, String),
    ClearDoneTerminalTasks(TerminalId),
    AppendTerminalTask(TerminalId, String),
    PrependTerminalTask(TerminalId, String),
    ConsumeNextTerminalTask(TerminalId),
    KillActiveTerminal,
}

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) struct TerminalFocusRequest {
    pub(crate) terminal_id: TerminalId,
}

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) enum TerminalVisibilityRequest {
    Isolate(TerminalId),
    ShowAll,
}

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) enum HudModuleRequest {
    Toggle(HudModuleId),
    Reset(HudModuleId),
}

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) enum TerminalViewRequest {
    ToggleActiveDisplayMode,
    ResetActiveView,
}

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) enum TerminalSendRequest {
    Active(String),
    Target {
        terminal_id: TerminalId,
        command: String,
    },
}

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) enum TerminalLifecycleRequest {
    Spawn,
    SpawnShell,
    KillActive,
}

#[derive(Clone, Debug, Message, PartialEq)]
pub(crate) enum TerminalTaskRequest {
    SetText {
        terminal_id: TerminalId,
        text: String,
    },
    ClearDone {
        terminal_id: TerminalId,
    },
    Append {
        terminal_id: TerminalId,
        text: String,
    },
    Prepend {
        terminal_id: TerminalId,
        text: String,
    },
    ConsumeNext {
        terminal_id: TerminalId,
    },
}
