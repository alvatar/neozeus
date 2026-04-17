use crate::{
    agents::{AgentId, AgentKind},
    composer::ComposerMode,
    hud::HudWidgetKey,
};
use bevy::prelude::Message;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerRequest {
    pub(crate) mode: ComposerMode,
}

#[derive(Clone, Debug, Message, PartialEq, Eq)]
pub(crate) enum AppCommand {
    Agent(AgentCommand),
    OwnedTmux(OwnedTmuxCommand),
    Task(TaskCommand),
    Composer(ComposerCommand),
    Aegis(AegisCommand),
    Recovery(RecoveryCommand),
    Widget(WidgetCommand),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentCommand {
    Create {
        label: Option<String>,
        kind: AgentKind,
        working_directory: String,
    },
    Rename {
        agent_id: AgentId,
        label: String,
    },
    Clone {
        source_agent_id: AgentId,
        label: String,
        workdir: bool,
    },
    Focus(AgentId),
    Inspect(AgentId),
    Reorder {
        agent_id: AgentId,
        target_index: usize,
    },
    TogglePaused(AgentId),
    ClearFocus,
    KillSelected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OwnedTmuxCommand {
    Select { session_uid: String },
    ClearSelection,
    KillSelected,
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
pub(crate) enum AegisCommand {
    Enable {
        agent_id: AgentId,
        prompt_text: String,
    },
    Disable {
        agent_id: AgentId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecoveryCommand {
    ResetAll,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WidgetCommand {
    Toggle(HudWidgetKey),
    Reset(HudWidgetKey),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agents::AgentId, composer::ComposerMode};

    /// Verifies that app command can wrap composer request.
    #[test]
    fn app_command_can_wrap_composer_request() {
        let command = AppCommand::Composer(ComposerCommand::Open(ComposerRequest {
            mode: ComposerMode::Message {
                agent_id: AgentId(1),
            },
        }));
        assert!(matches!(command, AppCommand::Composer(_)));
    }

    #[test]
    fn app_command_can_wrap_aegis_request() {
        let command = AppCommand::Aegis(AegisCommand::Enable {
            agent_id: AgentId(1),
            prompt_text: "keep going".into(),
        });
        assert!(matches!(command, AppCommand::Aegis(_)));
    }

    #[test]
    fn app_command_can_wrap_recovery_request() {
        let command = AppCommand::Recovery(RecoveryCommand::ResetAll);
        assert!(matches!(command, AppCommand::Recovery(_)));
    }
}
