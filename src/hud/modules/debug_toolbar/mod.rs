mod buttons;
mod input;
mod render;

use super::super::state::HudRect;
use super::super::widgets::HudWidgetKey;

pub(in crate::hud) use buttons::debug_toolbar_buttons;
pub(crate) use input::handle_pointer_click;
pub(crate) use render::render_content;

#[cfg(test)]
pub(crate) use buttons::test_debug_toolbar_buttons;

#[derive(Clone, Debug, PartialEq)]
enum DebugToolbarAction {
    SpawnTerminal,
    ShowAll,
    TogglePixelPerfect,
    ResetView,
    SendCommand(&'static str),
    ToggleModule(HudWidgetKey),
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::hud) struct DebugToolbarButton {
    pub(in crate::hud) label: String,
    pub(in crate::hud) rect: HudRect,
    action: DebugToolbarAction,
    pub(in crate::hud) active: bool,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DebugToolbarButtonTestView {
    pub(crate) label: String,
    pub(crate) rect: HudRect,
    pub(crate) active: bool,
}
