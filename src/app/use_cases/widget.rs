use crate::hud::{HudLayoutState, HudWidgetKey};

pub(crate) fn reset_widget(widget_id: HudWidgetKey, layout_state: &mut HudLayoutState) {
    layout_state.reset_module(widget_id);
}
