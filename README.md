# neozeus

Initial comparison result: the Alacritty path and the shadow-terminal/WezTerm path looked similar in the current renderer, so the decision was made on technical merit instead; `alacritty_terminal + portable-pty` won as the simpler, more direct, more configurable, and more mature embedding path for this app, while the shadow-terminal wrapper added risk without giving a decisive practical advantage.

Run with `cargo run`.

## Verification

Default development checks stay headless and cheap:

- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt --check`

Window-opening GUI verifiers are opt-in and grouped under `scripts/gui/`:

- full GUI suite: `./scripts/gui/run-suite.sh`
- visible-output verifier only: `./scripts/gui/verify-visible-terminal.sh`
- color verifier only: `./scripts/gui/verify-terminal-colors.sh`

Compatibility wrappers remain at the old paths:

- `./scripts/verify-visible-terminal.sh`
- `./scripts/verify-terminal-colors.sh`
