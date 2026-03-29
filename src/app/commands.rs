use crate::{agents::AgentId, composer::ComposerMode, hud::HudWidgetKey};
use bevy::prelude::Message;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerRequest {
    pub(crate) mode: ComposerMode,
}

#[derive(Clone, Debug, Message, PartialEq, Eq)]
pub(crate) enum AppCommand {
    Agent(AgentCommand),
    Task(TaskCommand),
    Composer(ComposerCommand),
    Widget(WidgetCommand),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentCommand {
    Create {
        label: Option<String>,
        spawn_shell_only: bool,
        working_directory: String,
    },
    Focus(AgentId),
    Inspect(AgentId),
    Reorder {
        agent_id: AgentId,
        target_index: usize,
    },
    ClearFocus,
    KillActive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TaskCommand {
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
