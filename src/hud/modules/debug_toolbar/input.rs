use crate::app::AppCommand;

use super::super::super::{state::HudLayoutState, state::HudRect, view_models::DebugToolbarView};
use bevy::prelude::Vec2;

/// Handles pointer clicks for the info bar.
///
/// The bar is intentionally non-interactive for now, so clicks are ignored and no commands are
/// emitted.
pub(crate) fn handle_pointer_click(
    _shell_rect: HudRect,
    _point: Vec2,
    _debug_toolbar_view: &DebugToolbarView,
    _layout_state: &HudLayoutState,
    _emitted_commands: &mut Vec<AppCommand>,
) {
}
