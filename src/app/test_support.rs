//! Test-only helpers scoped to the `app` module.
//!
//! This module is `#[cfg(test)]`-gated and holds fixtures used by tests that exercise the
//! `apply_app_commands` system from outside the module. Production code never sees this module.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::World;

use super::dispatch;

/// Test harness that runs the `apply_app_commands` system exactly once against the given world.
///
/// Inserts the baseline resources the dispatch system requires so that callers can focus on the
/// state they actually want to exercise. Missing resources are the most common cause of test
/// failures through this harness, so each required resource is seeded with its `default()` value
/// only when absent.
pub(crate) fn run_apply_app_commands(world: &mut World) {
    if !world.contains_resource::<crate::hud::AgentListSelection>() {
        world.insert_resource(crate::hud::AgentListSelection::default());
    }
    if !world.contains_resource::<crate::terminals::OwnedTmuxSessionStore>() {
        world.insert_resource(crate::terminals::OwnedTmuxSessionStore::default());
    }
    if !world.contains_resource::<crate::terminals::ActiveTerminalContentState>() {
        world.insert_resource(crate::terminals::ActiveTerminalContentState::default());
    }
    if !world.contains_resource::<crate::terminals::ActiveTerminalContentSyncState>() {
        world.insert_resource(crate::terminals::ActiveTerminalContentSyncState::default());
    }
    if !world.contains_resource::<crate::aegis::AegisPolicyStore>() {
        world.insert_resource(crate::aegis::AegisPolicyStore::default());
    }
    if !world.contains_resource::<crate::aegis::AegisRuntimeStore>() {
        world.insert_resource(crate::aegis::AegisRuntimeStore::default());
    }

    world.run_system_once(dispatch::apply_app_commands).unwrap();
}

/// Structural assertion: `src/app/mod.rs` must not contain `fn`/`impl`/`struct`/`enum`/`const`/
/// `static`/`type` definitions.
///
/// `PROJECT_RULES.md` and `STYLE_GUIDE.md` require `mod.rs` files to contain only module docs,
/// submodule declarations, and curated re-exports. This test pins that invariant for the `app`
/// module root so regressions are caught at build time rather than during code review.
#[test]
fn app_mod_rs_has_no_substantive_logic() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let contents = std::fs::read_to_string(format!("{manifest_dir}/src/app/mod.rs"))
        .expect("src/app/mod.rs must exist and be readable");

    for (line_number, line) in contents.lines().enumerate() {
        let stripped = match line.split_once("//") {
            Some((before, _)) => before,
            None => line,
        };
        let trimmed = stripped.trim_start();
        for keyword in [
            "fn ",
            "pub fn ",
            "pub(crate) fn ",
            "pub(super) fn ",
            "impl ",
            "impl<",
            "struct ",
            "pub struct ",
            "pub(crate) struct ",
            "enum ",
            "pub enum ",
            "pub(crate) enum ",
            "const ",
            "pub const ",
            "pub(crate) const ",
            "static ",
            "pub static ",
            "pub(crate) static ",
            "type ",
            "pub type ",
            "pub(crate) type ",
        ] {
            assert!(
                !trimmed.starts_with(keyword),
                "src/app/mod.rs:{} contains substantive item (`{}`). Move it into a named \
                 submodule; `mod.rs` is a curated surface only.",
                line_number + 1,
                keyword.trim()
            );
        }
    }
}
