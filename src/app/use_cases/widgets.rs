use crate::hud::{HudLayoutState, HudWidgetKey};

/// Toggles widget.
pub(crate) fn toggle_widget(widget_id: HudWidgetKey, layout_state: &mut HudLayoutState) {
    let enabled = !layout_state.module_enabled(widget_id);
    layout_state.set_module_enabled(widget_id, enabled);
}

/// Resets widget.
pub(crate) fn reset_widget(widget_id: HudWidgetKey, layout_state: &mut HudLayoutState) {
    layout_state.reset_module(widget_id);
}
