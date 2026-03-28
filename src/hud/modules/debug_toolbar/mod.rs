mod buttons;
mod input;
mod render;

use crate::hud::state::HudRect;
use crate::hud::widgets::HudWidgetKey;

pub(crate) use buttons::debug_toolbar_buttons;
pub(crate) use input::handle_pointer_click;
pub(crate) use render::render_content;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DebugToolbarAction {
    SpawnTerminal,
    ShowAll,
    TogglePixelPerfect,
    ResetView,
    SendCommand(&'static str),
    ToggleModule(HudWidgetKey),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DebugToolbarButton {
    pub(crate) label: String,
    pub(crate) rect: HudRect,
    pub(crate) action: DebugToolbarAction,
    pub(crate) active: bool,
}
