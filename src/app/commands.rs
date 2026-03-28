use crate::{agents::AgentId, conversations::ConversationId, hud::HudWidgetKey, ui::ComposerMode};
use bevy::prelude::Message;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerRequest {
    pub(crate) mode: ComposerMode,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Message, PartialEq, Eq)]
pub(crate) enum AppCommand {
    Agent(AgentCommand),
    Terminal(TerminalCommand),
    Conversation(ConversationCommand),
    Task(TaskCommand),
    Composer(ComposerCommand),
    Widget(WidgetCommand),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentCommand {
    SpawnTerminal,
    SpawnShellTerminal,
    Focus(AgentId),
    Inspect(AgentId),
    ShowAll,
    ClearFocus,
    KillActive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCommand {
    SendCommandToActive { command: String },
    ResetActiveView,
    ToggleActiveDisplayMode,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConversationCommand {
    SendMessage {
        conversation_id: ConversationId,
        sender: AgentId,
        body: String,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TaskCommand {
    SetText { agent_id: AgentId, text: String },
    Append { agent_id: AgentId, text: String },
    Prepend { agent_id: AgentId, text: String },
    ClearDone { agent_id: AgentId },
    ConsumeNext { agent_id: AgentId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ComposerCommand {
    Open(ComposerRequest),
    Submit,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WidgetCommand {
    Toggle(HudWidgetKey),
    Reset(HudWidgetKey),
}

#[cfg(test)]
mod tests;
