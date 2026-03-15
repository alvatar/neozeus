use crate::terminals::TerminalId;

use super::state::{HudModuleId, TerminalVisibilityPolicy};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HudCommand {
    SpawnTerminal,
    FocusTerminal(TerminalId),
    HideAllButTerminal(TerminalId),
    ShowAllTerminals,
    #[allow(
        dead_code,
        reason = "typed rename path stays in the HUD protocol even before a concrete rename UI exists"
    )]
    RenameAgent {
        terminal_id: TerminalId,
        label: String,
    },
    ToggleModule(HudModuleId),
    ToggleActiveTerminalDisplayMode,
    ResetTerminalView,
    SendActiveTerminalCommand(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HudEvent {
    TerminalFocused(TerminalId),
    TerminalPresentationPolicyChanged(TerminalVisibilityPolicy),
    TerminalSpawned(TerminalId),
    AgentRenamed {
        terminal_id: TerminalId,
        label: String,
    },
    ActiveTerminalDisplayModeToggled(TerminalId),
    ModuleEnabledChanged {
        id: HudModuleId,
        enabled: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HudRecipients {
    One(HudModuleId),
    Some(Vec<HudModuleId>),
    All,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HudEnvelope<T> {
    pub(crate) recipients: HudRecipients,
    pub(crate) payload: T,
}
