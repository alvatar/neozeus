# neozeus

Initial comparison result: the Alacritty path and the shadow-terminal/WezTerm path looked similar in the current renderer, so the decision was made on technical merit instead; `alacritty_terminal + portable-pty` won as the simpler, more direct, more configurable, and more mature embedding path for this app, while the shadow-terminal wrapper added risk without giving a decisive practical advantage.

Run with `cargo run`.

Window manager identity defaults to `neozeus` and can be overridden with:

- `NEOZEUS_APP_ID` — native window/app id (Wayland app_id / WM-visible name)
- `NEOZEUS_WINDOW_TITLE` — human-facing window title
- `NEOZEUS_WINDOW_MODE` — `windowed` to disable the default borderless fullscreen startup mode
- `NEOZEUS_WINDOW_SCALE_FACTOR` — optional scale-factor override for deterministic window sizing in GUI verification

Agent-list bloom verification / tuning can also override:

- `NEOZEUS_AGENT_BLOOM_INTENSITY` — non-negative bloom intensity override

## Verification

Default development checks stay headless and cheap:

- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt --check`

Offscreen visual verification is now the default path for layering/composition work. In
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

Window-opening GUI verifiers remain opt-in for cases the offscreen path does not yet cover and are
kept under `scripts/gui/`:

- full GUI suite: `./scripts/gui/run-suite.sh`
- visible-output verifier only: `./scripts/gui/verify-visible-terminal.sh`
- color verifier only: `./scripts/gui/verify-terminal-colors.sh`
- agent-list bloom verifier only: `./scripts/gui/verify-agent-list-bloom.sh`
- agent-list bloom stage inspector: `./scripts/gui/inspect-agent-list-bloom-stages.sh`

Compatibility wrappers remain at the old paths:

- `./scripts/verify-visible-terminal.sh`
- `./scripts/verify-terminal-colors.sh`
- `./scripts/verify-agent-list-bloom.sh`
- `./scripts/inspect-agent-list-bloom-stages.sh`
