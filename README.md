# neozeus

Two native Bevy terminal PoCs.

## PoCs

- `poc/bevy_shadow_terminal`
  - Bevy app
  - terminal core: `shadow-terminal` / WezTerm core
- `poc/bevy_alacritty_terminal`
  - Bevy app
  - terminal core: `alacritty_terminal` + `portable-pty`

Both are intentionally minimal:
- embedded in a Bevy window
- shell-backed PTY
- basic keyboard forwarding
- demo buttons for `pwd`, `ls`, `clear`, `top`, `vi`

## Run

```bash
cd ~/code/neozeus
cargo run -p bevy_shadow_terminal
cargo run -p bevy_alacritty_terminal
```

## Current limitation

Rendering is plain monospace text for both PoCs right now. The goal here is backend/input validation first, not final terminal-quality rendering.
