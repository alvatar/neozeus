#[path = "terminals/backend.rs"]
mod backend;
#[path = "terminals/fonts.rs"]
mod fonts;
#[path = "terminals/manager.rs"]
mod manager;
#[path = "terminals/presentation.rs"]
mod presentation;
#[path = "terminals/render.rs"]
mod render;
#[path = "terminals/state.rs"]
mod state;

pub(crate) use backend::*;
pub(crate) use fonts::*;
pub(crate) use manager::*;
pub(crate) use presentation::*;
pub(crate) use render::*;
pub(crate) use state::*;
