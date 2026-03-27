use crate::{agents::AgentId, hud::HudWidgetKey, terminals::TerminalId, ui::ComposerState};
use bevy::prelude::Resource;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VisibilityMode {
    #[default]
    ShowAll,
    FocusedOnly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HudWidgetPlacement {
    pub(crate) enabled: bool,
    pub(crate) rect: crate::hud::HudRect,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AppSessionState {
    pub(crate) active_agent: Option<AgentId>,
    pub(crate) visibility_mode: VisibilityMode,
    pub(crate) widget_layout: BTreeMap<HudWidgetKey, HudWidgetPlacement>,
    pub(crate) widget_order: Vec<HudWidgetKey>,
    pub(crate) composer: ComposerState,
    pub(crate) direct_input_terminal: Option<TerminalId>,
}

#[cfg(test)]
mod tests;
