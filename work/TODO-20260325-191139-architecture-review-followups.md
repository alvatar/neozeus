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
- [ ] Write down the current authority split explicitly:
  - [ ] `TerminalManager` / successor owns terminal identity + registry truth
  - [ ] `TerminalPresentationStore` / successor owns presentation projection truth
  - [ ] HUD state resources own retained HUD truth
  - [ ] ECS entities remain projections, not the primary source of truth
- [ ] Record current startup invariants:
  - [ ] normal startup restores/imports daemon sessions
  - [ ] verifier startup bypasses restore/import path
  - [ ] focus and visibility are separate concerns
- [ ] Record redraw invariants:
  - [ ] terminal work, presentation animation, and HUD animation can all request redraw
- [ ] Record persistence invariants:
  - [ ] UI metadata persistence is best-effort and non-fatal
  - [ ] process/session continuity is not derived from `TerminalId`

### 0.2 Decide up front what is intentionally unchanged in Phases 1-2
- [ ] No UX change for spawn/focus/isolate/reset view/direct input/task dialogs
- [ ] No daemon protocol change yet
- [ ] No renderer/perf optimization yet
- [ ] No persistence format change yet

### 0.3 Create validation checklist to run after every phase
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`
- [ ] no debug artifacts left behind
- [ ] existing GUI verifiers remain runnable even if not executed every phase

---

## Phase 1 — split orchestration and bootstrap without behavior change

### 1.1 Break up `src/scene.rs`
- [ ] Extract startup/bootstrap concerns into dedicated modules
- [ ] Move panic/GPU startup handling out of the main scene wiring file
- [ ] Move window/env config helpers out of the giant scene module
- [ ] Move resource/plugin registration into an app/bootstrap layer
- [ ] Move startup-only terminal restore/import code into a dedicated startup module
- [ ] Move verifier-only startup path into a dedicated startup/verifier module
- [ ] Keep `build_app()` as a thin composition root

### 1.2 Make scheduling topology easier to read
- [ ] Centralize `NeoZeusSet` definitions and ordering in one scheduling-focused file
- [ ] Keep the current ordering semantics identical
- [ ] Reduce cross-file hidden schedule coupling
- [ ] Document which sets mutate domain state vs projections

### 1.3 Reduce `scene.rs` responsibilities to app composition only
- [ ] after extraction, `scene.rs` should primarily:
  - [ ] define the public app build entrypoint
  - [ ] compose plugins/resources/startup hooks
  - [ ] expose only minimal helper APIs still genuinely scene-specific
- [ ] remove startup-restore and verifier implementation detail from this file

### 1.4 Validation
- [ ] deterministic tests unchanged
- [ ] startup restore behavior unchanged
- [ ] verifier bypass behavior unchanged
- [ ] redraw behavior unchanged

### Phase 1 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 2 — split HUD command/orchestration surface by concern

### 2.1 Break up `src/hud/dispatcher.rs`
- [ ] separate intent fanout from command application
- [ ] create dedicated modules/files for:
  - [ ] focus command handling
  - [ ] visibility command handling
  - [ ] HUD module toggle/reset handling
  - [ ] terminal view command handling
  - [ ] terminal send command handling
  - [ ] terminal task command handling
  - [ ] terminal lifecycle command handling
- [ ] keep external behavior identical

### 2.2 Narrow mutation surfaces
- [ ] ensure each command handler reads/writes the minimum resources needed
- [ ] reduce giant multi-resource systems where practical
- [ ] keep central lifecycle operations explicit rather than ad hoc in input handlers

### 2.3 Make command paths legible
- [ ] one place for intent fanout
- [ ] one place per domain mutation category
- [ ] no mixed concerns like “task editing + terminal spawn + visibility” in the same file

### 2.4 Validation
- [ ] all HUD interactions still route correctly
- [ ] task dialog/message box behavior unchanged
- [ ] spawn/kill/focus/isolate semantics unchanged

### Phase 2 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 3 — split `HudState` into smaller authority resources

### 3.1 Separate retained HUD layout state from modal/editor state
- [ ] extract `HudLayoutState`
  - [ ] modules
  - [ ] z-order
  - [ ] drag state
  - [ ] dirty-layout flag
- [ ] extract `HudModalState`
  - [ ] message box state
  - [ ] task dialog state
- [ ] extract `HudInputCaptureState`
  - [ ] direct terminal input capture target

### 3.2 Update systems to borrow only what they actually mutate
- [ ] pointer/drag systems use layout state only
- [ ] modal keyboard systems use modal state + input capture state only
- [ ] rendering reads all relevant HUD resources immutably
- [ ] persistence reads only layout state

### 3.3 Remove borrow-checker workaround patterns caused by monolithic HUD state
- [ ] revisit staged `Vec<HudIntent>` emission in pointer handling
- [ ] revisit snapshotting `module_ids` just to reborrow mutable state later
- [ ] only keep staging where it reflects actual event semantics, not borrow pressure

### 3.4 Preserve current retained-HUD invariants
- [ ] module shells remain retained state
- [ ] Vello scene remains projection only
- [ ] direct input capture still mutually excludes modal dialogs

### 3.5 Validation
- [ ] drag behavior unchanged
- [ ] modal/editor behavior unchanged
- [ ] direct terminal input behavior unchanged
- [ ] HUD persistence behavior unchanged

### Phase 3 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 4 — split terminal domain state by concern

### 4.1 Reduce `TerminalManager` scope
- [ ] decide target split:
  - [ ] `TerminalRegistry` for terminal map + creation order
  - [ ] `TerminalFocusState` for active terminal + focus order
  - [ ] optional `TerminalResizeState` for requested dimensions
- [ ] keep `ManagedTerminal` focused on per-terminal runtime/domain data only

### 4.2 Remove unnecessary contention on a single resource
- [ ] input/focus systems should not need mutable access to the entire registry unless they actually mutate terminal records
- [ ] presentation systems should not need broad mutable access to registry state when only reading focus/order
- [ ] persistence should read registry/focus state without coupling to resize internals

### 4.3 Preserve current invariants
- [ ] creation order remains stable across focus changes
- [ ] focus order remains an explicit separate concern
- [ ] active terminal clearing/focusing still works identically
- [ ] terminal removal still updates all relevant indices/orders

### 4.4 Revisit APIs after the split
- [ ] replace broad `TerminalManager` helper APIs with narrower equivalents where appropriate
- [ ] keep high-value ergonomic helpers where they still reflect real authority boundaries

### 4.5 Validation
- [ ] startup restore/import still reconstructs terminal order and focus correctly
- [ ] focus/visibility logic unchanged
- [ ] removal/kill cleanup unchanged

### Phase 4 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 5 — make daemon session lifecycle explicit

### 5.1 Decide the intended daemon session model
- [ ] choose one explicit model and document it:
  - [ ] **Option A:** persistent sessions remain visible after exit/failure until explicitly reaped
  - [ ] **Option B:** runtime sessions auto-remove when the child exits/disconnects
- [ ] verify the choice against current startup restore/import semantics
- [ ] verify the choice against “terminal as agent identity” expectations

### 5.2 Add an explicit daemon-side session lifecycle state machine
- [ ] define a clear internal model for session state:
  - [ ] running
  - [ ] exited
  - [ ] failed
  - [ ] disconnected
  - [ ] killed/reaped if distinct
- [ ] ensure registry behavior is driven by this state machine, not by incidental map retention

### 5.3 Fix session registry semantics
- [ ] define what `list_sessions()` returns for dead sessions
- [ ] define when a dead session is removed from the registry
- [ ] define whether clients can attach to non-running sessions for inspection/final surface
- [ ] define how startup reconcile should treat dead sessions

### 5.4 Fix ordering semantics
- [ ] stop relying on lexical `session_id` sorting for durable ordering
- [ ] introduce creation sequence / monotonic order in the daemon registry if needed
- [ ] preserve deterministic ordering for startup restore/import

### 5.5 Validation
- [ ] tests for exited/failure/disconnect lifecycle handling
- [ ] tests for registry/list semantics after exit
- [ ] tests for restore/import semantics under the chosen lifecycle model

### Phase 5 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 6 — clarify client attach and daemon-client ownership semantics

### 6.1 Review current single-attach-per-UI-process limitation
- [ ] decide whether “already attached in this UI process” is intentional product policy or just an implementation artifact
- [ ] if intentional, document it clearly in code/tests
- [ ] if not intentional, redesign session routing to support multiple local consumers safely

### 6.2 Tighten daemon client lifecycle handling
- [ ] make shutdown/reader/writer thread ownership explicit
- [ ] ensure client drop/shutdown behavior is deterministic
- [ ] ensure pending requests and session routes are cleaned up predictably on disconnect

### 6.3 Tighten daemon server connection semantics
- [ ] review subscription ownership and unsubscribe behavior
- [ ] ensure subscriber cleanup is robust on client disconnect and session kill
- [ ] ensure session kill and client disconnect do not leave stale routing/subscription state

### 6.4 Validation
- [ ] tests for reconnect/reattach semantics
- [ ] tests for kill/disconnect cleanup
- [ ] tests for route cleanup after failed attach or closed client

### Phase 6 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 7 — delete or isolate legacy backend architecture debt

### 7.1 Audit currently retained legacy modules
- [ ] `backend`
- [ ] `pty_backend`
- [ ] `tmux`
- [ ] `tmux_viewer_backend`
- [ ] any dead legacy attach/provision target variants that only exist for old paths

### 7.2 Choose a strategy per legacy path
- [ ] delete code that is no longer part of the supported architecture
- [ ] or move it behind Cargo features if real retention is required
- [ ] avoid dead default-build architecture that silently drifts out of sync

### 7.3 Clean up public surface
- [ ] remove unused reexports in `src/terminals.rs`
- [ ] reduce `#[allow(dead_code)]` debt
- [ ] remove stale comments that describe obsolete runtime paths as if they were active

### 7.4 Validation
- [ ] default build still supports the actual intended runtime path(s)
- [ ] tests cover any intentionally retained feature-gated path
- [ ] no accidental dependency on deleted legacy code remains

### Phase 7 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 8 — replace ad hoc persistence formats with structured serialization

### 8.1 Get approval before adding dependencies
- [ ] ask explicitly before adding `serde` / `ron` / `toml`
- [ ] decide format based on readability + migration cost

### 8.2 Replace session persistence format
- [ ] replace whitespace-splitting line protocol with structured serialization
- [ ] preserve versioning/migration capability
- [ ] support one-time read compatibility for old `terminals.v1` if needed
- [ ] ensure labels and future fields handle spaces, `=`, backslashes, and multiline safely

### 8.3 Replace HUD layout persistence format
- [ ] replace whitespace-splitting layout format with structured serialization
- [ ] preserve best-effort non-fatal load/save semantics
- [ ] preserve stable defaults for missing modules/fields

### 8.4 Validation
- [ ] roundtrip tests for both persistence formats
- [ ] malformed-file fallback tests
- [ ] migration tests if backward compatibility is kept

### Phase 8 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 9 — remove remaining borrow-checker-shaped design hacks in presentation/input

### 9.1 Revisit terminal presentation synchronization shape
- [ ] review whether `sync_terminal_presentations()` still needs broad `Local<>` snap caches after state/resource splitting
- [ ] separate explicit transition state from incidental previous-frame caches where it improves clarity
- [ ] reduce hidden behavior embedded in `Local<Option<...>>` if possible

### 9.2 Revisit background ordering implementation
- [ ] replace O(n²) `ordered.contains()` path with explicit set/index tracking
- [ ] keep output ordering identical

### 9.3 Revisit panel/frame entity synchronization
- [ ] determine whether panel/frame split can be expressed more cleanly after state refactor
- [ ] keep explicit disjoint borrowing only where ECS truly requires it
- [ ] avoid entity indirection that exists only because larger resources are over-coupled

### 9.4 Revisit input/pointer handling after HUD state split
- [ ] reduce mutable-state dance in pointer input if no longer necessary
- [ ] preserve central intent emission and drag semantics

### 9.5 Validation
- [ ] no behavior changes in focus/isolate/direct-input/panel-frame sync
- [ ] tests still cover current animation and visibility semantics

### Phase 9 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 10 — then address hot-path scaling and unnecessary allocation churn

### 10.1 HUD rendering hot path
- [ ] audit full-scene Vello rebuild cost
- [ ] avoid recomputing obviously stable derived data where cheap caching is justified
- [ ] stop allocating/render-transforming labels unnecessarily on every frame when unchanged
- [ ] specifically review agent-list row label uppercasing in render path

### 10.2 Terminal presentation hot path
- [ ] avoid repeated derived-order allocations where persistent scratch/state is more appropriate
- [ ] review active texture-state comparisons and cloned state in snap logic

### 10.3 Terminal raster hot path
- [ ] review all-terminals scan in `sync_terminal_texture()`
- [ ] consider explicit dirty-terminal tracking only after authority/resource split is stable
- [ ] preserve current correctness for dropped-frame/full-redraw semantics

### 10.4 Validation
- [ ] keep perf improvements explicit and measured
- [ ] do not change correctness semantics under optimization
- [ ] add targeted tests for dirty-state behavior if control flow changes materially

### Phase 10 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`
- [ ] run an explicit profiling/measurement pass before declaring perf work done

---

## Phase 11 — clean up startup/debug hacks in core path

### 11.1 Revisit panic-hook based GPU startup handling
- [ ] decide whether the current panic-hook/catch-unwind approach is still the least-bad option
- [ ] isolate it behind a startup/bootstrap boundary even if retained
- [ ] document why it exists and what exact panic signature it is compensating for

### 11.2 Revisit forced fallback adapter policy
- [ ] confirm whether `force_fallback_adapter: true` is intentional for production behavior or just dev/test convenience
- [ ] if intentional, document the tradeoff clearly
- [ ] if not, gate it by environment/config rather than hard-coding it in core startup

### 11.3 Revisit debug log path handling
- [ ] stop unconditional core-path truncation of `/tmp/neozeus-debug.log` unless explicitly intended
- [ ] document debug logging lifecycle/policy
- [ ] keep any debug convenience isolated from domain/runtime semantics

### 11.4 Validation
- [ ] startup failure reporting still works
- [ ] debug tooling remains usable
- [ ] no regression in headless/GUI error handling expectations

### Phase 11 gate
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

---

## Phase 12 — final architecture verification and documentation

### 12.1 Re-run full deterministic validation
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`

### 12.2 Re-run GUI verification suite
- [ ] `./scripts/gui/run-suite.sh`
- [ ] verify startup restore path still coexists with GUI verification mode
- [ ] verify verifier startup bypass still behaves deterministically
- [ ] verify agent-list/HUD/presentation behavior remains correct

### 12.3 Manual architecture spot-checks
- [ ] startup restore/import with live daemon sessions
- [ ] active terminal focus/clear/isolate/show-all paths
- [ ] task dialog/message box/direct input capture interactions
- [ ] terminal kill/remove path
- [ ] daemon disconnect/error path

### 12.4 Write architecture docs/update memory-worthy notes
- [ ] update any in-repo architecture docs if they exist
- [ ] record the final authority split and daemon lifecycle semantics clearly in prose
- [ ] ensure future contributors do not have to reverse-engineer state ownership from system order

---

## Suggested implementation order

1. [ ] Phase 0 — semantic freeze / invariants
2. [ ] Phase 1 — split `scene.rs` and bootstrap/orchestration
3. [ ] Phase 2 — split `hud/dispatcher.rs`
4. [ ] Phase 3 — split `HudState`
5. [ ] Phase 4 — split terminal domain state
6. [ ] Phase 5 — daemon lifecycle semantics
7. [ ] Phase 6 — daemon client/server attach ownership cleanup
8. [ ] Phase 7 — remove or feature-gate legacy backends
9. [ ] Phase 8 — structured persistence format
10. [ ] Phase 9 — remove remaining borrow-checker-shaped hacks
11. [ ] Phase 10 — perf/scaling work
12. [ ] Phase 11 — startup/debug hacks cleanup
13. [ ] Phase 12 — final verification + docs

---

## Cross-cutting invariants to preserve throughout

- [ ] ECS entities remain projections, not domain truth
- [ ] focus remains separate from visibility policy
- [ ] terminal identity remains separate from session/process lifecycle representation
- [ ] persistence remains best-effort and non-fatal
- [ ] no irreversible semantic changes are smuggled into “pure refactor” phases
- [ ] no new dependency is added without approval
- [ ] no tests are removed or weakened
