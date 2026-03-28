use crate::{agents::AgentId, terminals::TerminalId, ui::ComposerState};
use bevy::prelude::Resource;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VisibilityMode {
    #[default]
    ShowAll,
    FocusedOnly,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AppSessionState {
    pub(crate) active_agent: Option<AgentId>,
    pub(crate) visibility_mode: VisibilityMode,
    pub(crate) composer: ComposerState,
    pub(crate) direct_input_terminal: Option<TerminalId>,
}

#[cfg(test)]
mod tests;
