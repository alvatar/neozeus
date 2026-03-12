# neozeus

Two native Bevy terminal PoCs.

## PoCs

- `poc/bevy_shadow_terminal`
  - backend: `shadow-terminal` / WezTerm core
- `poc/bevy_alacritty_terminal`
  - backend: `alacritty_terminal` + `portable-pty`

## Run

```bash
cd ~/code/neozeus
cargo run -p bevy_shadow_terminal
cargo run -p bevy_alacritty_terminal
```

## Current state

Both now render:
- per-cell foreground/background colors
- cursor
- width-aware cell painting
- UTF-8 text better than the first pass

Still PoC-level:
- not optimized
- no image protocol rendering
- no selection yet
