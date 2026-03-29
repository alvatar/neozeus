use super::super::super::render::{HudPainter, HudRenderInputs};
use super::super::super::state::HudRect;

/// Renders the info bar contents.
///
/// The bar is intentionally empty for now; the shared shell drawing provides the static header
/// chrome and this hook remains ready for future status items.
pub(crate) fn render_content(
    _content_rect: HudRect,
    _painter: &mut HudPainter,
    _inputs: &HudRenderInputs,
) {
}
