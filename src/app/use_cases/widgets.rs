use crate::hud::{HudLayoutState, HudWidgetKey};

/// Toggles widget.
pub(crate) fn toggle_widget(widget_id: HudWidgetKey, layout_state: &mut HudLayoutState) {
    let enabled = layout_state
        .get(widget_id)
        .is_some_and(|module| !module.shell.enabled);
    layout_state.set_module_enabled(widget_id, enabled);
}

/// Resets widget.
pub(crate) fn reset_widget(widget_id: HudWidgetKey, layout_state: &mut HudLayoutState) {
    layout_state.reset_module(widget_id);
}
