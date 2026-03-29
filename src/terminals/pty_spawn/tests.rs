use super::*;
use std::{ffi::OsString, fs, path::PathBuf};

/// Rewrites the shell environment so tests run against an isolated temporary home/config tree.
///
/// This function is intentionally heavy-handed: it points HOME/XDG/ZDOTDIR/history-related variables
/// into a per-process temp directory, empties `.zshenv`, forces a minimal PATH, and disables common
/// shell startup hooks. The goal is deterministic test behavior regardless of the developer's real
/// shell configuration.
pub(super) fn apply_test_shell_isolation(command: &mut CommandBuilder) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    let root = std::env::temp_dir().join(format!("neozeus-test-shell-{}", std::process::id()));
    let home = root.join("home");
    let xdg_config = root.join("xdg-config");
    let xdg_state = root.join("xdg-state");
    let xdg_cache = root.join("xdg-cache");
    let zdotdir = root.join("zdotdir");
    let kitty = root.join("kitty");
    let history = root.join("history");
    let zshenv = zdotdir.join(".zshenv");

    for dir in [&home, &xdg_config, &xdg_state, &xdg_cache, &zdotdir, &kitty] {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&zshenv, "");

    command.env("HOME", home.as_os_str());
    command.env("XDG_CONFIG_HOME", xdg_config.as_os_str());
    command.env("XDG_STATE_HOME", xdg_state.as_os_str());
    command.env("XDG_CACHE_HOME", xdg_cache.as_os_str());
    command.env("KITTY_CONFIG_DIRECTORY", kitty.as_os_str());
    command.env("ZDOTDIR", zdotdir.as_os_str());
    command.env("ZSHENV", zshenv.as_os_str());
    command.env("HISTFILE", history.as_os_str());
    command.env("BASH_ENV", "/dev/null");
    command.env("ENV", "/dev/null");
    command.env("SHELL", "/bin/zsh");
    command.env("PATH", "/usr/bin:/bin");
}

/// Locks down the current default shell choice used by [`spawn_pty`].
///
/// This is intentionally tiny, but it protects against accidentally changing the hard-coded shell
/// executable without noticing the behavioral impact on spawned sessions.
#[test]
fn raw_shell_program_is_zsh() {
    assert_eq!(raw_shell_program(), OsString::from("zsh"));
}

/// Verifies that configured shell working directories expand `~` and ignore empty input.
#[test]
fn resolve_shell_cwd_expands_home_and_ignores_empty_values() {
    let home = std::env::temp_dir().join("neozeus-pty-home-test");
    std::env::set_var("HOME", &home);

    assert_eq!(resolve_shell_cwd(None).unwrap(), None);
    assert_eq!(resolve_shell_cwd(Some("  ")).unwrap(), None);
    assert_eq!(resolve_shell_cwd(Some("~")).unwrap(), Some(home.clone()));
    assert_eq!(
        resolve_shell_cwd(Some("~/code")).unwrap(),
        Some(home.join("code"))
    );
    assert_eq!(
        resolve_shell_cwd(Some("/tmp/work")).unwrap(),
        Some(PathBuf::from("/tmp/work"))
    );
}
