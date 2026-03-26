# neozeus

Initial comparison result: the Alacritty path and the shadow-terminal/WezTerm path looked similar in the current renderer, so the decision was made on technical merit instead; `alacritty_terminal + portable-pty` won as the simpler, more direct, more configurable, and more mature embedding path for this app, while the shadow-terminal wrapper added risk without giving a decisive practical advantage.

Run with `cargo run`.

Window manager identity defaults to `neozeus` and can be overridden with:

- `NEOZEUS_APP_ID` — native window/app id (Wayland app_id / WM-visible name)
- `NEOZEUS_WINDOW_TITLE` — human-facing window title
- `NEOZEUS_WINDOW_MODE` — `windowed` to disable the default borderless fullscreen startup mode
- `NEOZEUS_WINDOW_SCALE_FACTOR` — optional scale-factor override for deterministic sizing in offscreen verification

NeoZeus also reads a small TOML config from the first existing path in:

- `NEOZEUS_CONFIG_PATH`
- `$XDG_CONFIG_HOME/neozeus/config.toml`
- `~/.config/neozeus/config.toml`
- `./neozeus.toml`

Currently supported TOML keys:

```toml
[terminal]
font_path = "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf"

[window]
title = "neozeus"
app_id = "neozeus"
```

The repo now includes a `neozeus.toml` that pins the terminal primary font to Adwaita Mono.
Environment variables still override TOML values.

Agent-list bloom verification / tuning can also override:

- `NEOZEUS_AGENT_BLOOM_INTENSITY` — non-negative bloom intensity override

## Module map

### Core

| Module | Purpose |
| --- | --- |
| `main` | Entry point; chooses app vs daemon mode. |
| `app` | App facade; re-exports bootstrap/output/schedule pieces. |
| `app::bootstrap` | Builds the Bevy app, plugins, resources, and window setup. |
| `app::output` | Offscreen output routing and final-frame capture. |
| `app::schedule` | System-set ordering and schedule wiring. |
| `app_config` | Env/TOML config loading and parsing. |
| `startup` | Scene bootstrap, restore/import, initial terminal startup. |
| `input` | Global terminal keyboard/mouse input handling. |
| `verification` | Auto-verify dispatch and deterministic verification scenarios. |

### HUD

| Module | Purpose |
| --- | --- |
| `hud` | HUD facade; exports HUD systems, state, and helpers. |
| `hud::animation` | Shell rect/alpha animation for retained HUD modules. |
| `hud::bloom` | Agent-list bloom render pipeline and tuning. |
| `hud::capture` | HUD/window texture readback and dumps. |
| `hud::commands` | HUD intent application layer. |
| `hud::commands::focus` | Applies focus requests into terminal state. |
| `hud::commands::intent_fanout` | Fans HUD intents into typed request queues. |
| `hud::commands::lifecycle` | Applies spawn/kill lifecycle requests. |
| `hud::commands::modules` | Applies HUD module enable/reset requests. |
| `hud::commands::send` | Applies terminal send/message requests. |
| `hud::commands::tasks` | Applies task-note mutations and task sends. |
| `hud::commands::view` | Applies pan/zoom view changes. |
| `hud::commands::visibility` | Applies show-all/isolate visibility changes. |
| `hud::compositor` | Offscreen HUD composite mesh/camera setup. |
| `hud::input` | HUD pointer hit-testing and HUD shortcuts. |
| `hud::message_box` | Retained message-box/task-dialog editor state. |
| `hud::messages` | Intent/request enums shared by HUD systems. |
| `hud::modules` | Module dispatch layer for HUD widgets. |
| `hud::modules::agent_list` | Agent-list widget state/rows/interaction. |
| `hud::modules::agent_list::interaction` | Agent-list click/hover/scroll behavior. |
| `hud::modules::agent_list::render` | Agent-list drawing helpers. |
| `hud::modules::debug_toolbar` | Debug-toolbar widget wiring. |
| `hud::modules::debug_toolbar::buttons` | Debug-toolbar button models. |
| `hud::modules::debug_toolbar::input` | Debug-toolbar pointer handling. |
| `hud::modules::debug_toolbar::render` | Debug-toolbar drawing helpers. |
| `hud::persistence` | HUD layout save/load and debounce. |
| `hud::render` | Vello drawing for HUD shells and modals. |
| `hud::setup` | HUD scene startup and structural layout sync. |
| `hud::state` | Retained HUD resources and data model. |

### Terminals

| Module | Purpose |
| --- | --- |
| `terminals` | Terminal facade; exports runtime/render/state pieces. |
| `terminals::ansi_surface` | Converts Alacritty grid state into `TerminalSurface`. |
| `terminals::backend` | Terminal backend helpers, colors, and command encoding. |
| `terminals::bridge` | App-side command bridge to terminal runtimes. |
| `terminals::daemon` | Daemon facade for client/server/protocol/session. |
| `terminals::daemon::client` | Unix-socket daemon client and subprocess bootstrap. |
| `terminals::daemon::protocol` | Wire-format encode/decode for daemon IPC. |
| `terminals::daemon::server` | Daemon accept loop and request dispatch. |
| `terminals::daemon::session` | Per-session PTY worker and subscriber fanout. |
| `terminals::damage` | Row-damage computation for partial reraster. |
| `terminals::debug` | Debug logging and runtime stats helpers. |
| `terminals::fonts` | Font discovery, matching, and raster config. |
| `terminals::lifecycle` | Spawn/kill terminal sessions and ECS cleanup. |
| `terminals::mailbox` | Coalesced terminal update mailbox. |
| `terminals::notes` | Per-session notes/tasks parsing and persistence. |
| `terminals::presentation` | Terminal layout, visibility, and panel placement. |
| `terminals::presentation_state` | Presentation/view ECS state and resources. |
| `terminals::pty_spawn` | Shell command creation and test-shell isolation. |
| `terminals::raster` | `TerminalSurface` → Bevy image rasterization. |
| `terminals::registry` | Authoritative terminal manager and focus state. |
| `terminals::runtime` | Runtime spawner and runtime-status publication. |
| `terminals::session_persistence` | Session save/load/reconcile logic. |
| `terminals::types` | Shared terminal data types. |

### Tests

| Module | Purpose |
| --- | --- |
| `tests` | Shared test helpers and fake daemon/runtime plumbing. |
| `tests::hud` | HUD behavior regression tests. |
| `tests::input` | Keyboard/mouse input regression tests. |
| `tests::scene` | Startup/config/output scheduling regression tests. |
| `tests::terminals` | Terminal rendering, daemon, and lifecycle regression tests. |
| `app::output::tests` | Offscreen output/capture unit tests. |
| `hud::bloom::tests` | Bloom pipeline unit tests. |
| `hud::capture::tests` | HUD capture utility unit tests. |
| `hud::persistence::tests` | HUD persistence unit tests. |
| `terminals::damage::tests` | Damage-tracking unit tests. |
| `terminals::daemon::protocol::tests` | Daemon protocol compatibility tests. |
| `terminals::notes::tests` | Notes/task parsing unit tests. |
| `terminals::session_persistence::tests` | Session persistence unit tests. |
| `verification::tests` | Verification scenario parsing tests. |

## Verification

Default development checks stay headless and cheap:

- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt --check`

Offscreen visual verification is now the only supported visual-regression path. In
`NEOZEUS_OUTPUT_MODE=offscreen`, NeoZeus runs with a synthetic `PrimaryWindow`, disables Winit,
and renders directly into image targets without creating a real OS window.

Offscreen scripts live under `scripts/offscreen/`:

- full offscreen suite: `./scripts/offscreen/run-suite.sh`
- single scenario capture: `./scripts/offscreen/run-scenario.sh <scenario> <output-path> [bloom-intensity] [width] [height]`
- message-box bloom verifier: `./scripts/offscreen/verify-message-box-bloom.sh`
- task-dialog bloom verifier: `./scripts/offscreen/verify-task-dialog-bloom.sh`
- agent-list bloom verifier: `./scripts/offscreen/verify-agent-list-bloom.sh`
- inspect-switch verifier: `./scripts/offscreen/verify-inspect-switch-latency.sh`

Supported built-in offscreen scenarios:

- `agent-list-bloom`
- `message-box-bloom`
- `task-dialog-bloom`
- `inspect-switch-latency`

To add a new offscreen scenario:

1. extend `VerificationScenario` in `src/verification.rs`
2. implement the deterministic setup in `run_verification_scenario`
3. add/update scenario tests in `src/verification.rs`
4. add a dedicated verifier script under `scripts/offscreen/`
5. wire it into `scripts/offscreen/run-suite.sh` if it should be part of the default regression set

Legacy GUI verifier entrypoints remain only as compatibility shims:

- `./scripts/gui/run-suite.sh` delegates to the offscreen suite where possible
- GUI-only verifiers that still require a real window now fail fast with an explicit error
- `./scripts/verify-agent-list-bloom.sh` now points directly at the offscreen verifier

Compatibility wrappers remain at the old paths, but they no longer launch a visible window.
