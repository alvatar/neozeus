# PLAN — terminal daemon for high-fidelity persistent sessions

## Locked decisions

### Process model
- Daemon runs as the same binary via `neozeus daemon`.
- Daemon is a separate OS process.
- Daemon is single-instance per user.
- UI startup flow:
  1. resolve socket path
  2. try connect
  3. if absent, spawn `neozeus daemon --socket <path>`
  4. retry connect with bounded timeout
  5. perform protocol handshake/version validation

### IPC / transport
- Transport: Unix domain socket.
- Topology: one long-lived bidirectional connection per UI process.
- Protocol shape: request/response plus async events on the same connection.
- Messages are framed and versioned.
- No tmux in the default rendering path.

### Authority split
- Daemon owns:
  - PTY lifetime
  - child process lifetime
  - terminal parser/emulator state
  - terminal snapshot/delta truth
  - session registry
- UI owns:
  - Bevy presentation/projection
  - focus / visibility policy
  - HUD
  - viewport zoom/pan and local interaction state
  - persisted UI metadata (labels/order/focus) for reconnect UX
- There is exactly one terminal authority: the daemon.
- UI never reparses raw PTY output.

### Session identity
- Sessions use daemon-issued stable string ids.
- Prefixes:
  - persistent/default sessions: `neozeus-session-`
  - verifier sessions: `neozeus-verifier-`
- Existing persistence continues to store session ids + UI metadata.

### Rendering model
- Daemon parses PTY bytes incrementally with the existing terminal engine.
- On attach, daemon returns a full authoritative snapshot plus current revision.
- After attach, daemon streams incremental `TerminalUpdate` events tagged with session id and revision.
- UI consumes snapshot/deltas and updates its local presentation projection.

### Bridge/client shape
- UI keeps one daemon client resource for the single socket connection.
- Per-terminal `TerminalBridge` remains the UI-facing adapter.
- Each bridge is backed by daemon session attach/send/kill operations, not a local PTY worker.

### Compatibility policy
- Legacy tmux code may remain isolated for compatibility/tests, but daemon becomes the default primary backend.
- No hidden fallback from daemon path back to tmux snapshot rendering.

### Testing requirements
- Protocol roundtrip tests.
- Socket lifecycle tests.
- stale-socket handling tests.
- attach/snapshot/update ordering tests.
- reconnect tests.
- daemon-backed spawn/input/kill integration tests.
- failure-injection tests for socket/protocol/process errors.
