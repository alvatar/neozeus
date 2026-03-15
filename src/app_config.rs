use bevy_egui::egui;

pub(crate) const DEFAULT_COLS: u16 = 120;
pub(crate) const DEFAULT_ROWS: u16 = 38;
pub(crate) const DEFAULT_BG: egui::Color32 = egui::Color32::from_rgb(10, 10, 10);
pub(crate) const TERMINAL_MARGIN: f32 = 48.0;
pub(crate) const DEFAULT_CELL_HEIGHT_PX: u32 = 24;
pub(crate) const DEFAULT_CELL_WIDTH_PX: u32 = 14;
pub(crate) const GPU_NOT_FOUND_PANIC_FRAGMENT: &str = "Unable to find a GPU!";

pub(crate) const DEBUG_LOG_PATH: &str = "/tmp/neozeus-debug.log";
pub(crate) const DEBUG_TEXTURE_DUMP_PATH: &str = "/tmp/neozeus-texture.ppm";

pub(crate) const EVA_DEMO_Z: f32 = 20.0;
