use crate::hud::{HudRect, HUD_AGENT_LIST_WIDTH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct HudWidgetKey(&'static str);

#[allow(non_upper_case_globals)]
impl HudWidgetKey {
    pub(crate) const DebugToolbar: Self = Self("debug-toolbar");
    pub(crate) const AgentList: Self = Self("agent-list");
    pub(crate) const ConversationList: Self = Self("conversation-list");
    pub(crate) const ThreadPane: Self = Self("thread-pane");

    /// Returns the numeric shortcut displayed for this HUD widget.
    pub(crate) fn number(self) -> u8 {
        widget_definition(self)
            .map(|definition| definition.shortcut_number)
            .unwrap_or_default()
    }

    /// Returns the user-facing title for the current startup-connect phase.
    pub(crate) fn title(self) -> &'static str {
        widget_definition(self)
            .map(|definition| definition.title)
            .unwrap_or("Widget")
    }

    /// Returns the persistence/title key for this HUD widget.
    pub(crate) fn title_key(self) -> &'static str {
        widget_definition(self)
            .map(|definition| definition.persistence_key)
            .unwrap_or("Widget")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudWidgetDefinition {
    pub(crate) key: HudWidgetKey,
    pub(crate) shortcut_number: u8,
    pub(crate) title: &'static str,
    pub(crate) persistence_key: &'static str,
    pub(crate) default_enabled: bool,
    pub(crate) default_rect: HudRect,
}

pub(crate) const HUD_WIDGET_DEFINITIONS: [HudWidgetDefinition; 4] = [
    HudWidgetDefinition {
        key: HudWidgetKey::DebugToolbar,
        shortcut_number: 0,
        title: "Debug Toolbar",
        persistence_key: "DebugToolbar",
        default_enabled: true,
        default_rect: HudRect {
            x: 24.0,
            y: 24.0,
            w: 920.0,
            h: 64.0,
        },
    },
    HudWidgetDefinition {
        key: HudWidgetKey::AgentList,
        shortcut_number: 1,
        title: "Agent List",
        persistence_key: "AgentList",
        default_enabled: true,
        default_rect: HudRect {
            x: 0.0,
            y: 0.0,
            w: HUD_AGENT_LIST_WIDTH,
            h: 720.0,
        },
    },
    HudWidgetDefinition {
        key: HudWidgetKey::ConversationList,
        shortcut_number: 2,
        title: "Conversations",
        persistence_key: "ConversationList",
        default_enabled: false,
        default_rect: HudRect {
            x: 332.0,
            y: 112.0,
            w: 320.0,
            h: 320.0,
        },
    },
    HudWidgetDefinition {
        key: HudWidgetKey::ThreadPane,
        shortcut_number: 3,
        title: "Thread",
        persistence_key: "ThreadPane",
        default_enabled: false,
        default_rect: HudRect {
            x: 668.0,
            y: 112.0,
            w: 520.0,
            h: 320.0,
        },
    },
];

/// Returns the static definition for one HUD widget key.
pub(crate) fn widget_definition(key: HudWidgetKey) -> Option<&'static HudWidgetDefinition> {
    HUD_WIDGET_DEFINITIONS
        .iter()
        .find(|definition| definition.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that widget registry includes conversation and thread widgets.
    #[test]
    fn widget_registry_includes_conversation_and_thread_widgets() {
        assert!(HUD_WIDGET_DEFINITIONS
            .iter()
            .any(|definition| definition.key == HudWidgetKey::ConversationList));
        assert!(HUD_WIDGET_DEFINITIONS
            .iter()
            .any(|definition| definition.key == HudWidgetKey::ThreadPane));
    }
}
