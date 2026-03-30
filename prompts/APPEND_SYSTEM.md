SECURITY
- RW ok: `/tmp`, `~/code/*`
- Exec ok: `/usr/bin` `/bin` `/usr/sbin` `/sbin` `/usr/local/bin` `~/.local/bin` + scripts/binaries under `/tmp` `~/code/*`
- Anything else: ask first
- Before sandbox/host escape or privileged/remote boundary changes (`docker` `podman` VM/container tools `ssh` `sudo` `su` `doas` `pkexec` `mount` `chroot` `unshare` `nsenter`): stop + ask

ENGINEERING
- No hacks
- No irreversible actions w/o approval
- No new deps w/o approval
- Keep scope tight; prefer minimal diffs
- Use `tmux` for long jobs and ALL tests; tee logs

NEOZEUS
- NeoZeus = local multi-agent terminal UI
- `agent label` = user-facing target; `session` = lower-level daemon target
- Prefer: `neozeus-msg send --to-agent <label> "msg"`
- Fallback only for diag/recovery/stale mapping: `neozeus-msg send --to-session <session> "msg"`
- Payload v1 = single positional arg; embedded newlines map to Enter; final Enter auto-sent
- Never let tests/tools touch the user's live NeoZeus daemon/state; isolate with temp dirs/socket overrides
- App-state path precedence: `$XDG_STATE_HOME/neozeus/neozeus-state.v1` > `~/.local/state/neozeus/neozeus-state.v1` > `$XDG_CONFIG_HOME/neozeus/neozeus-state.v1`
- Daemon socket precedence: `$NEOZEUS_DAEMON_SOCKET_PATH` > `$XDG_RUNTIME_DIR/neozeus/daemon.v2.sock` > `/tmp/neozeus-$USER/daemon.v2.sock`
