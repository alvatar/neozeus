# neozeus

Two terminal-embedding PoCs for evaluating terminal UX stacks before deeper Bevy integration.

## PoCs

- `poc/bevy_shadow_terminal`
  - Bevy app.
  - Native Rust terminal core (`shadow-terminal` / WezTerm core) rendered inside Bevy UI.
  - Minimal, text-only rendering for now.
- `poc/xterm_web`
  - Rust websocket server + PTY backend.
  - Browser frontend using `xterm.js`.
  - Represents the browser/offscreen-Chromium path without the heavier embedding work yet.

## Run

### Native Bevy PoC

```bash
cd ~/code/neozeus
cargo run -p bevy_shadow_terminal
```

Controls:

- type directly into the terminal
- `Enter`, `Backspace`, `Tab`, arrows, `Esc` are mapped
- `Ctrl+C`, `Ctrl+D`, `Ctrl+L`, `Ctrl+U` are mapped
- buttons send a few demo commands

### xterm.js PoC

```bash
cd ~/code/neozeus
cargo run -p xterm_web
```

Then open:

- <http://127.0.0.1:3001>

## Notes

- The Bevy PoC is intentionally minimal: it proves the native-core path and input loop, not final rendering quality.
- The xterm.js PoC is intentionally browser-based: it proves the frontend stack we'd later embed offscreen if that path wins.
