# neozeus

A local multi-agent terminal UI built with Bevy.

## What it does

- persistent agent terminals
- fast agent switching
- message box + task dialog UX
- offscreen visual regression checks
- small TOML config surface

## Run

```bash
cargo run
```

Default startup is borderless fullscreen.
Use `NEOZEUS_WINDOW_MODE=windowed` for a normal window.

## neozeus-msg

Install the standalone messaging CLI with:

```bash
./scripts/install-neozeus-msg
```

Usage:

```bash
neozeus-msg send --to-agent <agent-name> "message..."
neozeus-msg send --to-session <session-name> "message..."
```

Target modes:

- `--to-agent` — normal user-facing mode; resolves the current agent label through `neozeus-state.v1`
- `--to-session` — lower-level diagnostic/recovery mode; targets one daemon session directly

The command connects directly to the NeoZeus daemon and sends the payload through the existing terminal command path, so embedded newlines behave like Enter presses and a final Enter is sent automatically.

## Useful keys

- `z` — spawn agent terminal
- `Ctrl+Alt+z` — spawn plain `zsh`
- `j` / `k` or `↑` / `↓` — inspect previous/next agent
- `Enter` — open message box for active agent
- `t` — open task dialog
- `n` — consume next task
- `Ctrl+Enter` — toggle direct terminal input
- `Ctrl+k` — kill active terminal
- `F10` — quit
- `0` / `1` — toggle HUD modules
- `Alt+Shift+0/1` — reset HUD modules

## Config

Search order:

1. `NEOZEUS_CONFIG_PATH`
2. `$XDG_CONFIG_HOME/neozeus/config.toml`
3. `~/.config/neozeus/config.toml`
4. `./neozeus.toml`

Supported keys:

```toml
[terminal]
font_path = "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"
font_size_px = 16.0
baseline_offset_px = -0.5

[window]
title = "NEOZEUS CONTROL PANEL"
app_id = "neozeus"
```

Env overrides still win.

Useful env overrides:

- `NEOZEUS_WINDOW_MODE=windowed`
- `NEOZEUS_WINDOW_TITLE=...`
- `NEOZEUS_APP_ID=...`
- `NEOZEUS_TERMINAL_FONT_PATH=...`
- `NEOZEUS_TERMINAL_FONT_SIZE_PX=...`
- `NEOZEUS_TERMINAL_BASELINE_OFFSET_PX=...`
- `NEOZEUS_AGENT_BLOOM_INTENSITY=...`

## Visual verification

Offscreen mode is the supported visual-regression path.
It renders without opening a real desktop window.

```bash
./scripts/offscreen/run-suite.sh
```

Useful scripts:

- `./scripts/offscreen/run-scenario.sh <scenario> <output.ppm>`
- `./scripts/offscreen/verify-agent-list-bloom.sh`
- `./scripts/offscreen/verify-message-box-bloom.sh`
- `./scripts/offscreen/verify-task-dialog-bloom.sh`
- `./scripts/offscreen/verify-inspect-switch-latency.sh`
- `./scripts/offscreen/verify-pi-restore-interactive.sh`

Built-in scenarios:

- `agent-list-bloom`
- `message-box-bloom`
- `task-dialog-bloom`
- `inspect-switch-latency`
- `pi-restore-interactive`

## Dev checks

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
