mod app_config;
mod hud;
mod input;
mod scene;
mod terminals;
mod verification;

#[cfg(test)]
mod tests;

use crate::{app_config::DEBUG_LOG_PATH, scene::build_app, terminals::append_debug_log};
use std::fs;

fn main() {
    let _ = fs::write(DEBUG_LOG_PATH, "");
    append_debug_log("app start");
    match build_app() {
        Ok(mut app) => {
            let _ = app.run();
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
