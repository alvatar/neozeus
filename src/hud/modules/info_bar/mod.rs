mod input;
mod render;

pub(crate) use input::handle_pointer_click;
pub(in crate::hud) use render::{render_content, INFO_BAR_BACKGROUND, INFO_BAR_BORDER};

#[cfg(test)]
mod tests;
