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

NEOZEUS HELPERS
- Use NeoZeus helpers when they exist
- Prefer canonical CLI, not custom flow

TMUX
- Use tmux for:
  - all tests
  - builds
  - benchmarks
  - long commands
  - noisy commands
- Use:
  - `neozeus-tmux run --name <NAME> [--cwd <DIR>] -- <COMMAND...>`
- If command writes log: tee stdout too
  - example: `... 2>&1 | tee /tmp/<log>.log`
- After start:
  - report tmux name
  - user can `tmux attach -t <tmux-name>`
  - poll log for result

MESSAGING
- Prefer:
  - `neozeus-msg send --to-agent <label> "msg"`
- Exact-session fallback only when needed:
  - `neozeus-msg send --to-session <session> "msg"`
- Payload v1 = one quoted arg
- Embedded newlines map to Enter

WORKTREE
- Only use lifecycle commands from git worktree branch `neozeus/*`
- Commit work before merge commands
- If user says:
  - `merge and continue` -> `neozeus-worktree merge-continue`
  - `merge and finalize` -> `neozeus-worktree merge-finalize`
  - `discard worktree` -> `neozeus-worktree discard`
- If merge conflicts happen:
  - inspect conflicted files
  - resolve
  - `git add <files>`
  - `git commit`
  - run the lifecycle command again

NEOZEUS
- Never let tests/tools touch the user's live NeoZeus daemon/state; isolate with temp dirs/socket overrides
- App-state path precedence: `$XDG_STATE_HOME/neozeus/neozeus-state.v1` > `~/.local/state/neozeus/neozeus-state.v1` > `$XDG_CONFIG_HOME/neozeus/neozeus-state.v1`
- Daemon socket precedence: `$NEOZEUS_DAEMON_SOCKET_PATH` > `$XDG_RUNTIME_DIR/neozeus/daemon.v2.sock` > `/tmp/neozeus-$USER/daemon.v2.sock`
