# TODO — architecture review follow-ups: orchestration split, state boundaries, daemon lifecycle

Source: architecture review requested on 2026-03-24/25.

Goal:
- reduce orchestration gravity
- reduce borrow-checker-shaped design pressure
- clarify daemon/session lifecycle semantics
- delete or isolate legacy architecture debt
- replace fragile persistence hacks with structured state
- only then address scaling/perf cleanups

Constraints:
- do not broaden scope into unrelated rendering/HUD redesign
- do not add new dependencies without explicit approval
- do not delete tests
- keep behavior stable unless a phase explicitly changes semantics
- commit after each meaningful completed phase

Success criteria:
- `scene.rs` no longer acts as a composition + startup + verifier + runtime god module
- HUD/terminal state ownership is narrower and better aligned with system mutation boundaries
- daemon session lifecycle is explicit and tested
- legacy backends are either deleted or feature-gated
- persistence format is no longer a hand-rolled whitespace protocol
- no regressions in existing static or GUI verification flows

---

## Phase 0 — freeze the target semantics before refactoring

### 0.1 Record current architectural invariants
- [x] Write down the current authority split explicitly:
  - [x] `TerminalManager` / successor owns terminal identity + registry truth
  - [x] `TerminalPresentationStore` / successor owns presentation projection truth
  - [x] HUD state resources own retained HUD truth
  - [x] ECS entities remain projections, not the primary source of truth
- [x] Record current startup invariants:
  - [x] normal startup restores/imports daemon sessions
  - [x] verifier startup bypasses restore/import path
  - [x] focus and visibility are separate concerns
- [x] Record redraw invariants:
  - [x] terminal work, presentation animation, and HUD animation can all request redraw
- [x] Record persistence invariants:
  - [x] UI metadata persistence is best-effort and non-fatal
  - [x] process/session continuity is not derived from `TerminalId`

### 0.2 Decide up front what is intentionally unchanged in Phases 1-2
- [x] No UX change for spawn/focus/isolate/reset view/direct input/task dialogs
- [x] No daemon protocol change yet
- [x] No renderer/perf optimization yet
- [x] No persistence format change yet

### 0.3 Create validation checklist to run after every phase
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`
- [x] no debug artifacts left behind
- [x] existing GUI verifiers remain runnable even if not executed every phase

Note:
- invariants/non-goals were recorded in `work/ARCH-20260325-refactor-invariants.md`

---

## Phase 1 — split orchestration and bootstrap without behavior change

### 1.1 Break up `src/scene.rs`
- [x] Extract startup/bootstrap concerns into dedicated modules
- [x] Move panic/GPU startup handling out of the main scene wiring file
- [x] Move window/env config helpers out of the giant scene module
- [x] Move resource/plugin registration into an app/bootstrap layer
- [x] Move startup-only terminal restore/import code into a dedicated startup module
- [x] Move verifier-only startup path into a dedicated startup/verifier module
- [x] Keep `build_app()` as a thin composition root

### 1.2 Make scheduling topology easier to read
- [x] Centralize `NeoZeusSet` definitions and ordering in one scheduling-focused file
- [x] Keep the current ordering semantics identical
- [x] Reduce cross-file hidden schedule coupling
- [x] Document which sets mutate domain state vs projections

### 1.3 Reduce `scene.rs` responsibilities to app composition only
- [x] after extraction, `scene.rs` should primarily:
  - [x] define the public app build entrypoint
  - [x] compose plugins/resources/startup hooks
  - [x] expose only minimal helper APIs still genuinely scene-specific
- [x] remove startup-restore and verifier implementation detail from this file

### 1.4 Validation
- [x] deterministic tests unchanged
- [x] startup restore behavior unchanged
- [x] verifier bypass behavior unchanged
- [x] redraw behavior unchanged

### Phase 1 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 2 — split HUD command/orchestration surface by concern

### 2.1 Break up `src/hud/dispatcher.rs`
- [x] separate intent fanout from command application
- [x] create dedicated modules/files for:
  - [x] focus command handling
  - [x] visibility command handling
  - [x] HUD module toggle/reset handling
  - [x] terminal view command handling
  - [x] terminal send command handling
  - [x] terminal task command handling
  - [x] terminal lifecycle command handling
- [x] keep external behavior identical

### 2.2 Narrow mutation surfaces
- [x] ensure each command handler reads/writes the minimum resources needed
- [x] reduce giant multi-resource systems where practical
- [x] keep central lifecycle operations explicit rather than ad hoc in input handlers

### 2.3 Make command paths legible
- [x] one place for intent fanout
- [x] one place per domain mutation category
- [x] no mixed concerns like “task editing + terminal spawn + visibility” in the same file

### 2.4 Validation
- [x] all HUD interactions still route correctly
- [x] task dialog/message box behavior unchanged
- [x] spawn/kill/focus/isolate semantics unchanged

### Phase 2 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 3 — split `HudState` into smaller authority resources

### 3.1 Separate retained HUD layout state from modal/editor state
- [x] extract `HudLayoutState`
  - [x] modules
  - [x] z-order
  - [x] drag state
  - [x] dirty-layout flag
- [x] extract `HudModalState`
  - [x] message box state
  - [x] task dialog state
- [x] extract `HudInputCaptureState`
  - [x] direct terminal input capture target

### 3.2 Update systems to borrow only what they actually mutate
- [x] pointer/drag systems use layout state only
- [x] modal keyboard systems use modal state + input capture state only
- [x] rendering reads all relevant HUD resources immutably
- [x] persistence reads only layout state

### 3.3 Remove borrow-checker workaround patterns caused by monolithic HUD state
- [x] revisit staged `Vec<HudIntent>` emission in pointer handling
- [x] revisit snapshotting `module_ids` just to reborrow mutable state later
- [x] only keep staging where it reflects actual event semantics, not borrow pressure

### 3.4 Preserve current retained-HUD invariants
- [x] module shells remain retained state
- [x] Vello scene remains projection only
- [x] direct input capture still mutually excludes modal dialogs

### 3.5 Validation
- [x] drag behavior unchanged
- [x] modal/editor behavior unchanged
- [x] direct terminal input behavior unchanged
- [x] HUD persistence behavior unchanged

### Phase 3 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 4 — split terminal domain state by concern

### 4.1 Reduce `TerminalManager` scope
- [x] decide target split:
  - [x] `TerminalRegistry` for terminal map + creation order
  - [x] `TerminalFocusState` for active terminal + focus order
  - [x] optional `TerminalResizeState` for requested dimensions
- [x] keep `ManagedTerminal` focused on per-terminal runtime/domain data only

Note:
- the registry resource kept the existing `TerminalManager` type name for call-surface stability; it now acts as the registry/creation-order authority while focus moved to `TerminalFocusState`
- `requested_dimensions` intentionally stayed with `ManagedTerminal` after review; a separate resize resource was deemed unnecessary at current complexity

### 4.2 Remove unnecessary contention on a single resource
- [x] input/focus systems should not need mutable access to the entire registry unless they actually mutate terminal records
- [x] presentation systems should not need broad mutable access to registry state when only reading focus/order
- [x] persistence should read registry/focus state without coupling to resize internals

### 4.3 Preserve current invariants
- [x] creation order remains stable across focus changes
- [x] focus order remains an explicit separate concern
- [x] active terminal clearing/focusing still works identically
- [x] terminal removal still updates all relevant indices/orders

### 4.4 Revisit APIs after the split
- [x] replace broad `TerminalManager` helper APIs with narrower equivalents where appropriate
- [x] keep high-value ergonomic helpers where they still reflect real authority boundaries

### 4.5 Validation
- [x] startup restore/import still reconstructs terminal order and focus correctly
- [x] focus/visibility logic unchanged
- [x] removal/kill cleanup unchanged

### Phase 4 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 5 — make daemon session lifecycle explicit

### 5.1 Decide the intended daemon session model
- [x] choose one explicit model and document it:
  - [x] **Option A:** persistent sessions remain visible after exit/failure until explicitly reaped
  - [ ] **Option B:** runtime sessions auto-remove when the child exits/disconnects
- [x] verify the choice against current startup restore/import semantics
- [x] verify the choice against “terminal as agent identity” expectations

### 5.2 Add an explicit daemon-side session lifecycle state machine
- [x] define a clear internal model for session state:
  - [x] running
  - [x] exited
  - [x] failed
  - [x] disconnected
  - [x] killed/reaped if distinct
- [x] ensure registry behavior is driven by this state machine, not by incidental map retention

### 5.3 Fix session registry semantics
- [x] define what `list_sessions()` returns for dead sessions
- [x] define when a dead session is removed from the registry
- [x] define whether clients can attach to non-running sessions for inspection/final surface
- [x] define how startup reconcile should treat dead sessions

### 5.4 Fix ordering semantics
- [x] stop relying on lexical `session_id` sorting for durable ordering
- [x] introduce creation sequence / monotonic order in the daemon registry if needed
- [x] preserve deterministic ordering for startup restore/import

### 5.5 Validation
- [x] tests for exited/failure/disconnect lifecycle handling
- [x] tests for registry/list semantics after exit
- [x] tests for restore/import semantics under the chosen lifecycle model

### Phase 5 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 6 — clarify client attach and daemon-client ownership semantics

### 6.1 Review current single-attach-per-UI-process limitation
- [x] decide whether “already attached in this UI process” is intentional product policy or just an implementation artifact
- [x] if intentional, document it clearly in code/tests
- [ ] if not intentional, redesign session routing to support multiple local consumers safely

### 6.2 Tighten daemon client lifecycle handling
- [x] make shutdown/reader/writer thread ownership explicit
- [x] ensure client drop/shutdown behavior is deterministic
- [x] ensure pending requests and session routes are cleaned up predictably on disconnect

### 6.3 Tighten daemon server connection semantics
- [x] review subscription ownership and unsubscribe behavior
- [x] ensure subscriber cleanup is robust on client disconnect and session kill
- [x] ensure session kill and client disconnect do not leave stale routing/subscription state

### 6.4 Validation
- [x] tests for reconnect/reattach semantics
- [x] tests for kill/disconnect cleanup
- [x] tests for route cleanup after failed attach or closed client

### Phase 6 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 7 — delete or isolate legacy backend architecture debt

### 7.1 Audit currently retained legacy modules
- [x] `backend`
- [x] `pty_backend`
- [x] `tmux`
- [x] `tmux_viewer_backend`
- [x] any dead legacy attach/provision target variants that only exist for old paths

### 7.2 Choose a strategy per legacy path
- [x] delete code that is no longer part of the supported architecture
- [x] or move it behind Cargo features if real retention is required
- [x] avoid dead default-build architecture that silently drifts out of sync

### 7.3 Clean up public surface
- [x] remove unused reexports in `src/terminals.rs`
- [x] reduce `#[allow(dead_code)]` debt
- [x] remove stale comments that describe obsolete runtime paths as if they were active

### 7.4 Validation
- [x] default build still supports the actual intended runtime path(s)
- [x] tests cover any intentionally retained feature-gated path
- [x] no accidental dependency on deleted legacy code remains

### Phase 7 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 8 — replace ad hoc persistence formats with structured serialization

### 8.1 Get approval before adding dependencies
- [x] ask explicitly before adding `serde` / `ron` / `toml`
- [x] decide format based on readability + migration cost

Note:
- no new dependency was added; instead, both persistence paths moved to structured `version 2` block formats with backward-compatible `version 1` readers

### 8.2 Replace session persistence format
- [x] replace whitespace-splitting line protocol with structured serialization
- [x] preserve versioning/migration capability
- [x] support one-time read compatibility for old `terminals.v1` if needed
- [x] ensure labels and future fields handle spaces, `=`, backslashes, and multiline safely

### 8.3 Replace HUD layout persistence format
- [x] replace whitespace-splitting layout format with structured serialization
- [x] preserve best-effort non-fatal load/save semantics
- [x] preserve stable defaults for missing modules/fields

### 8.4 Validation
- [x] roundtrip tests for both persistence formats
- [x] malformed-file fallback tests
- [x] migration tests if backward compatibility is kept

### Phase 8 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 9 — remove remaining borrow-checker-shaped design hacks in presentation/input

### 9.1 Revisit terminal presentation synchronization shape
- [x] review whether `sync_terminal_presentations()` still needs broad `Local<>` snap caches after state/resource splitting
- [x] separate explicit transition state from incidental previous-frame caches where it improves clarity
- [x] reduce hidden behavior embedded in `Local<Option<...>>` if possible

### 9.2 Revisit background ordering implementation
- [x] replace O(n²) `ordered.contains()` path with explicit set/index tracking
- [x] keep output ordering identical

### 9.3 Revisit panel/frame entity synchronization
- [x] determine whether panel/frame split can be expressed more cleanly after state refactor
- [x] keep explicit disjoint borrowing only where ECS truly requires it
- [x] avoid entity indirection that exists only because larger resources are over-coupled

### 9.4 Revisit input/pointer handling after HUD state split
- [x] reduce mutable-state dance in pointer input if no longer necessary
- [x] preserve central intent emission and drag semantics

### 9.5 Validation
- [x] no behavior changes in focus/isolate/direct-input/panel-frame sync
- [x] tests still cover current animation and visibility semantics

### Phase 9 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 10 — then address hot-path scaling and unnecessary allocation churn

### 10.1 HUD rendering hot path
- [x] audit full-scene Vello rebuild cost
- [x] avoid recomputing obviously stable derived data where cheap caching is justified
- [x] stop allocating/render-transforming labels unnecessarily on every frame when unchanged
- [x] specifically review agent-list row label uppercasing in render path

### 10.2 Terminal presentation hot path
- [x] avoid repeated derived-order allocations where persistent scratch/state is more appropriate
- [x] review active texture-state comparisons and cloned state in snap logic

### 10.3 Terminal raster hot path
- [x] review all-terminals scan in `sync_terminal_texture()`
- [x] consider explicit dirty-terminal tracking only after authority/resource split is stable
- [x] preserve current correctness for dropped-frame/full-redraw semantics

### 10.4 Validation
- [x] keep perf improvements explicit and measured
- [x] do not change correctness semantics under optimization
- [x] add targeted tests for dirty-state behavior if control flow changes materially

### Phase 10 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`
- [x] run an explicit profiling/measurement pass before declaring perf work done

Note:
- representative timing smoke checks were run for a terminal presentation path and the agent-list bloom path using shell `time` around focused unit tests

---

## Phase 11 — clean up startup/debug hacks in core path

### 11.1 Revisit panic-hook based GPU startup handling
- [x] decide whether the current panic-hook/catch-unwind approach is still the least-bad option
- [x] isolate it behind a startup/bootstrap boundary even if retained
- [x] document why it exists and what exact panic signature it is compensating for

### 11.2 Revisit forced fallback adapter policy
- [x] confirm whether `force_fallback_adapter: true` is intentional for production behavior or just dev/test convenience
- [x] if intentional, document the tradeoff clearly
- [x] if not, gate it by environment/config rather than hard-coding it in core startup

### 11.3 Revisit debug log path handling
- [x] stop unconditional core-path truncation of `/tmp/neozeus-debug.log` unless explicitly intended
- [x] document debug logging lifecycle/policy
- [x] keep any debug convenience isolated from domain/runtime semantics

### 11.4 Validation
- [x] startup failure reporting still works
- [x] debug tooling remains usable
- [x] no regression in headless/GUI error handling expectations

### Phase 11 gate
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

---

## Phase 12 — final architecture verification and documentation

### 12.1 Re-run full deterministic validation
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`

### 12.2 Re-run GUI verification suite
- [ ] `./scripts/gui/run-suite.sh`
- [ ] verify startup restore path still coexists with GUI verification mode
- [ ] verify verifier startup bypass still behaves deterministically
- [ ] verify agent-list/HUD/presentation behavior remains correct

### 12.3 Manual architecture spot-checks
- [x] startup restore/import with live daemon sessions
- [x] active terminal focus/clear/isolate/show-all paths
- [x] task dialog/message box/direct input capture interactions
- [x] terminal kill/remove path
- [x] daemon disconnect/error path

### 12.4 Write architecture docs/update memory-worthy notes
- [x] update any in-repo architecture docs if they exist
- [x] record the final authority split and daemon lifecycle semantics clearly in prose
- [x] ensure future contributors do not have to reverse-engineer state ownership from system order

Note:
- GUI verifier launch was attempted, but the visible-terminal verifier did not complete in this environment despite the app starting and dispatching auto-verify input; deterministic validation remains green

---

## Suggested implementation order

1. [x] Phase 0 — semantic freeze / invariants
2. [x] Phase 1 — split `scene.rs` and bootstrap/orchestration
3. [x] Phase 2 — split `hud/dispatcher.rs`
4. [x] Phase 3 — split `HudState`
5. [x] Phase 4 — split terminal domain state
6. [x] Phase 5 — daemon lifecycle semantics
7. [x] Phase 6 — daemon client/server attach ownership cleanup
8. [x] Phase 7 — remove or feature-gate legacy backends
9. [x] Phase 8 — structured persistence format
10. [x] Phase 9 — remove remaining borrow-checker-shaped hacks
11. [x] Phase 10 — perf/scaling work
12. [x] Phase 11 — startup/debug hacks cleanup
13. [ ] Phase 12 — final verification + docs

---

## Cross-cutting invariants to preserve throughout

- [x] ECS entities remain projections, not domain truth
- [x] focus remains separate from visibility policy
- [x] terminal identity remains separate from session/process lifecycle representation
- [x] persistence remains best-effort and non-fatal
- [x] no irreversible semantic changes are smuggled into “pure refactor” phases
- [x] no new dependency is added without approval
- [x] no tests are removed or weakened
