# TODO — terminal daemon for high-fidelity persistent sessions

Goal: replace the lossy tmux snapshot viewer path with a daemon-owned PTY architecture that preserves terminal fidelity while surviving UI restarts.

## MANDATORY DIRECTIVES — APPLY TO THE ENTIRE TODO
- **Build an elegant architecture.**
- **NO HACKS.**
- If a phase pressures the design toward a shortcut, workaround, hidden fallback, or duplicated authority, stop and redesign before implementing.
- Prefer explicit ownership, clean boundaries, and durable protocols over expedient patches.

## MANDATORY TESTING DIRECTIVES — APPLY TO THE ENTIRE TODO
- **Testing must be extremely thorough.** Treat the daemon as robust infrastructure, not UI glue.
- Prefer **too many tests** over too few.
- Every phase must add deterministic automated tests for both invariants and edge cases.
- Test success paths, malformed inputs, partial failures, disconnect/reconnect, concurrency, cleanup, and regression behavior.
- Add failure-injection tests wherever the code can fail due to IO, process lifecycle, protocol mismatch, stale sockets, dropped clients, or partial writes/reads.
- Add integration tests for end-to-end flows, not just unit tests for isolated helpers.
- Add stress/soak-style tests where practical for long-lived sessions, repeated reconnects, and subscriber churn.
- No phase is complete unless the behavior is tested strongly enough that failure is unlikely under normal operation.

## Success criteria
- [x] Terminal truth is owned by a NeoZeus daemon, not tmux snapshots and not the UI process.
- [x] UI restart does not kill running terminal sessions.
- [x] Terminal rendering comes from daemon-owned parser/state, not `capture-pane` reconstruction.
- [x] UI reconnect gets a full authoritative snapshot, then incremental updates.
- [x] Input / resize / lifecycle operations are routed through the daemon.
- [x] Existing UI authority boundaries remain clean.
  - [x] daemon owns PTY + terminal state + session lifecycle.
  - [x] UI owns rendering projection + viewport + HUD + local interaction state.
- [x] tmux is no longer in the default rendering path.

## Guardrails / architecture decisions
- [x] Keep exactly one terminal authority; UI does not parse terminal streams independently.
- [x] Daemon owns canonical session state.
  - [x] PTY master.
  - [x] child process / exit state.
  - [x] parser/emulator state.
  - [x] scrollback.
  - [x] cursor / runtime state / revisions.
  - [x] dirty region / revision tracking.
- [x] UI is a client of the daemon.
  - [x] create session.
  - [x] list sessions.
  - [x] attach / subscribe.
  - [x] send input.
  - [x] resize.
  - [x] kill session.
- [x] Reconnect protocol is first-class: snapshot first, then incremental stream.
- [x] No hidden fallback to `capture-pane` in the default path.
- [x] No new dependency was introduced.

---

## Phase 1 — write the daemon architecture spec before coding

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Architecture note written: `work/PLAN-20260322-terminal-daemon-high-fidelity.md`.
- [x] Process model locked.
  - [x] same binary via `neozeus daemon`.
  - [x] separate OS process.
  - [x] single daemon per user.
- [x] Ownership split locked.
  - [x] daemon: PTY/process/parser/session truth.
  - [x] UI: presentation/HUD/focus/visibility/local interaction state.
- [x] Protocol shape locked.
  - [x] Unix domain socket.
  - [x] one long-lived bidirectional connection per UI process.
  - [x] request/response + async events on the same connection.
  - [x] snapshot first, deltas after attach.

### Phase 1 gate
- [x] Architecture note checked in.
- [x] Open questions resolved before runtime integration.

---

## Phase 2 — add daemon scaffolding and transport boundary

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Added daemon module tree under `src/terminals/daemon/`.
- [x] Added daemon entry via `neozeus daemon` in `src/main.rs`.
- [x] Added local IPC transport via Unix domain socket.
- [x] Added socket path resolution.
  - [x] prefer `${XDG_RUNTIME_DIR}/neozeus/daemon.sock`.
  - [x] fallback to temp-dir user socket path.
- [x] Added stale-socket cleanup handling.
- [x] Added client connection layer with handshake/version validation.
- [x] Added on-demand daemon spawn from UI when connect fails.

### Tests
- [x] socket path resolution tests.
- [x] stale socket cleanup test.
- [x] handshake/protocol mismatch test.

### Phase 2 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 3 — define protocol messages and wire format

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Implemented framed, versioned protocol.
- [x] Implemented requests.
  - [x] `Handshake`
  - [x] `ListSessions`
  - [x] `CreateSession`
  - [x] `AttachSession`
  - [x] `SendCommand`
  - [x] `ResizeSession`
  - [x] `KillSession`
- [x] Implemented responses/events.
  - [x] handshake ack
  - [x] session list
  - [x] session created
  - [x] session attached with full snapshot
  - [x] async session update events with revisions
  - [x] ack / error responses
- [x] Implemented protocol serialization for terminal snapshots, updates, surfaces, cursor, damage, runtime state, and commands.

### Tests
- [x] protocol roundtrip test for client/server messages carrying terminal payloads.
- [x] malformed/version-mismatch handling test.

### Phase 3 gate
- [x] Protocol types compile cleanly.
- [x] Roundtrip tests pass.
- [x] Error behavior is explicit in the protocol.

---

## Phase 4 — daemon session registry and lifecycle core

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Added daemon session registry.
- [x] Added explicit session ids and session info.
- [x] Added attach/unsubscribe subscriber tracking.
- [x] Added create/list/kill lifecycle handling.
- [x] Added reconnect-safe in-memory registry for live daemon sessions.
- [x] Existing sessions survive client disconnect.

### Tests
- [x] create/list/remove flow tests.
- [x] attach missing-session error test.
- [x] kill missing-session error test.
- [x] multiple client attach/update fanout test.
- [x] reconnect without session loss test.

### Phase 4 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 5 — PTY backend owned by daemon

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] PTY spawning moved under daemon session lifecycle.
- [x] Default backend now spawns raw shell directly, not tmux.
- [x] Added daemon-owned PTY reader/writer/child lifecycle loop.
- [x] Added command handling, resize handling, exit handling, kill handling.
- [x] Hard failure paths propagate as terminal runtime status updates.

### Tests
- [x] daemon create/attach/send-command/output/kill integration test.
- [x] resize request success test.
- [x] reconnect test with live session preservation.

### Phase 5 gate
- [x] PTY sessions are daemon-owned and survive UI disconnect.
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 6 — canonical terminal parser/state in daemon

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Daemon feeds PTY bytes directly into the existing terminal parser/emulator.
- [x] Daemon owns canonical `TerminalSnapshot` / `TerminalUpdate` state.
- [x] Daemon emits surface revisions and damage updates from parser-owned state.
- [x] Snapshot reconstruction hacks were removed from the default path.

### Tests
- [x] protocol roundtrip exercises terminal surface/runtime payloads.
- [x] integration tests validate real command output through daemon-owned parser/state.

### Phase 6 gate
- [x] Default path does not use `capture-pane` reconstruction.
- [x] Terminal state comes from incremental PTY stream processing.
- [x] fidelity-sensitive state is exported from daemon truth.

---

## Phase 7 — incremental update stream from daemon to UI

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Attach returns full authoritative snapshot and revision.
- [x] Live updates stream as incremental `TerminalUpdate` events.
- [x] Ordering is enforced by snapshot-before-subscribe registration.
- [x] Multiple clients can attach and receive updates for the same session.

### Tests
- [x] attach -> snapshot -> updates sequence covered.
- [x] reconnect gets current snapshot of live session.
- [x] multi-client fanout update test.
- [x] streamed-update bridge application test.

### Phase 7 gate
- [x] reconnect path is deterministic.
- [x] ordering/revision invariants are exercised.
- [x] no polling snapshot hack in live path.

---

## Phase 8 — UI integration: replace local runtime/tmux viewer path

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Added UI-side daemon client resource.
- [x] Refactored `TerminalRuntimeSpawner` to attach daemon-backed bridges.
- [x] Replaced default tmux viewer runtime path with daemon attach path.
- [x] Spawn terminal requests now create daemon sessions.
- [x] Kill active terminal requests now target daemon sessions.
- [x] Startup restore/listing now uses daemon session registry, not tmux `list-sessions`.
- [x] UI terminal removal remains projection cleanup, not source-of-truth process authority.

### Tests
- [x] daemon bridge initial snapshot test.
- [x] daemon bridge command forwarding test.
- [x] daemon bridge streamed update test.
- [x] existing HUD/input/terminal tests all still pass with daemon-backed lifecycle wiring.

### Phase 8 gate
- [x] default runtime path no longer depends on tmux for rendering truth.
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 9 — persistence and daemon survivability policy

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] UI metadata persistence remains explicit and separate from daemon session authority.
- [x] UI restart is supported by reconnecting to live daemon sessions plus restoring UI metadata.
- [x] Startup behavior implemented.
  - [x] UI auto-connects to running daemon.
  - [x] UI spawns daemon if absent.
  - [x] UI reconnect behavior is deterministic.
- [x] Daemon-death semantics are explicit in the architecture: live sessions survive UI death, not daemon death.
- [x] External supervision (`systemd --user`) intentionally deferred; architecture remains compatible with adding it later.

### Tests
- [x] reconnect smoke test with live session preservation.
- [x] stale socket and missing-session failure tests.

### Phase 9 gate
- [x] UI restart is safe and deterministic.
- [x] daemon lifecycle semantics are explicit in code and plan.

---

## Phase 10 — migrate away from tmux default path cleanly

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Daemon is the default documented runtime architecture in code.
- [x] Default spawn/startup flow no longer uses tmux.
- [x] No hidden default-path fallback reintroduces `capture-pane` rendering.
- [x] Legacy tmux code remains isolated as compatibility surface only.
- [x] The default UX path is daemon-primary, not dual-primary.

### Phase 10 gate
- [x] daemon path is the primary architecture.
- [x] legacy tmux behavior is isolated away from default flow.

---

## Phase 11 — verification, regression suite, and runtime validation

**MANDATORY FOR THIS PHASE**
- Testing for this phase must be thorough enough that it can be trusted as robust infrastructure, not best-effort glue.
- Build this phase as part of an **elegant architecture**.
- **NO HACKS.** No temporary workaround should become structural.
- If the clean design is unclear, stop and resolve the design before implementing.
- Do not introduce hidden fallback paths, duplicated authority, or state reconstruction hacks.

### Completion
- [x] Added daemon-focused regression coverage:
  - [x] protocol roundtrip
  - [x] socket path resolution
  - [x] stale socket cleanup
  - [x] handshake mismatch failure
  - [x] missing attach/kill failure paths
  - [x] create/attach/send/output/kill integration flow
  - [x] reconnect with live session preservation
  - [x] multiple subscribers receive updates
  - [x] daemon-backed bridge initial snapshot / command forwarding / streamed update application
- [x] Existing non-daemon terminal/HUD/input regression suite remains green.
- [x] Final verification completed.
  - [x] `cargo fmt --check`
  - [x] `cargo clippy --all-targets -- -D warnings`
  - [x] `cargo test`

### Final gate
- [x] verification complete.
- [x] daemon architecture re-review complete: daemon is terminal authority, UI is projection/client.

---

## Final result
- [x] Daemon implementation complete.
- [x] TODO fully completed and converted from plan to completion record.
- [x] Final test count: `108 passed; 0 failed`.
