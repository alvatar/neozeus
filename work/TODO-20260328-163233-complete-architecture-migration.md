# TODO: complete the state-driven architecture migration

Source plan: `work/PLAN-20260328-complete-architecture-migration.md`

## Global execution rules
- [ ] Preserve behavior unless a task explicitly changes product semantics.
- [ ] Add or update tests for every meaningful architectural change before marking a phase done.
- [ ] Delete compatibility shims as soon as their replacement path is proven.
- [ ] Do not leave dual ownership or dual command paths temporarily “for later” once a phase starts.
- [ ] After each phase run:
  - [ ] `cargo test`
  - [ ] `cargo clippy --all-targets -- -D warnings`
  - [ ] `cargo fmt --check`
- [ ] For rendering/projection-affecting phases, also run the relevant offscreen/visual subset immediately.

---

## Phase 1 — collapse to one command surface

### 1.1 Audit runtime `HudIntent` usage
- [ ] Enumerate all non-test `HudIntent` writers.
- [ ] Enumerate all non-test `HudIntent` readers.
- [ ] Identify every runtime schedule edge that still routes through `translate_hud_intents_to_app_commands`.

### 1.2 Replace HUD/input emission with direct `AppCommand`
- [ ] Change `src/hud/input.rs` to write `AppCommand` instead of `HudIntent`.
- [ ] Replace task action emission from message/task modal interactions with direct app/task commands.
- [ ] Replace focus/isolate click handling with direct `AppCommand::Agent(...)` emission.
- [ ] Replace widget toggle/reset emission with direct `AppCommand::Widget(...)`.
- [ ] Replace debug-toolbar actions with direct app commands.

### 1.3 Remove terminal-keyed HUD vocabulary from widgets
- [ ] Update `src/hud/modules/agent_list/interaction.rs` so row clicks emit agent-keyed commands.
- [ ] Update `src/hud/modules/conversation_list.rs` so row clicks emit agent/conversation-keyed commands.
- [ ] Ensure no widget interaction emits `TerminalId`-based product actions except terminal-command-specific flows.

### 1.4 Delete runtime translation layer
- [ ] Remove `translate_hud_intents_to_app_commands` from `src/app/dispatch.rs`.
- [ ] Remove runtime `HudIntent` schedule wiring from `src/app/schedule.rs`.
- [ ] Remove `HudIntent` message registration from bootstrap/runtime setup if no longer needed.
- [ ] Delete `src/hud/messages.rs` if fully unused.

### 1.5 Update tests to new command surface
- [ ] Replace HUD/input tests that currently assert `HudIntent` emission with `AppCommand` assertions.
- [ ] Replace helper utilities that drain `HudIntent` messages.
- [ ] Update characterization tests for spawn/focus/show-all/inspect/task actions.

### 1.6 Phase 1 validation
- [ ] Verify there are no non-test `HudIntent` references left in runtime code.
- [ ] Run validation commands.

---

## Phase 2 — give tasks a real domain boundary

### 2.1 Reshape command model
- [ ] Add `TaskCommand` to `src/app/commands.rs`.
- [ ] Add `AppCommand::Task(TaskCommand)`.
- [ ] Move task variants out of `ConversationCommand`.
- [ ] Update imports/re-exports and pattern matches in dispatch/handlers/tests.

### 2.2 Separate task use cases from conversation use cases
- [ ] Create `src/app/use_cases/tasks.rs`.
- [ ] Move `set_task_text`, `append_task`, `prepend_task`, `clear_done_tasks`, `consume_next_task` into it.
- [ ] Restrict `src/app/use_cases/conversation.rs` to message/conversation behavior only.
- [ ] Update `src/app/use_cases/mod.rs` exports.

### 2.3 Remove dual task truth
- [ ] Decide exact role of `TerminalNotesState`: persistence adapter, compatibility projection, or removable legacy state.
- [ ] Remove inline task mutation code that writes both `AgentTaskStore` and `TerminalNotesState` as co-equal truth.
- [ ] Introduce a narrow task-to-notes sync helper/system if compatibility output is still required.
- [ ] Ensure UI/task rendering reads from `AgentTaskStore`-derived state only.

### 2.4 Rework persistence/sync path
- [ ] If session notes persistence is still required, add a one-way projection from `AgentTaskStore` to persisted note/session text.
- [ ] Mark persistence dirty from the projection path, not each task mutation use case.
- [ ] Ensure restore/bootstrap reconstructs `AgentTaskStore` correctly from persisted source.

### 2.5 Update command dispatch
- [ ] Route task commands through `AppCommand::Task` handling.
- [ ] Remove task-specific handling from conversation command branches.
- [ ] Update composer submit mapping for task-related modes.

### 2.6 Add/adjust tests
- [ ] Unit tests for `TaskCommand` routing.
- [ ] Domain tests for append/prepend/clear/consume/set-text behavior.
- [ ] Regression tests that prove `AgentTaskStore` is sole task truth.
- [ ] Persistence/sync tests for task -> notes/session compatibility path if retained.

### 2.7 Phase 2 validation
- [ ] Verify no task variant remains under `ConversationCommand`.
- [ ] Verify no task use case mutates `TerminalNotesState` as peer truth.
- [ ] Run validation commands.

---

## Phase 3 — move product policy out of terminal lifecycle helpers

### 3.1 Narrow terminal lifecycle helper responsibility
- [ ] Inspect `src/terminals/lifecycle.rs` for kill/remove helpers that mutate focus/visibility/view policy.
- [ ] Design a narrower helper API that performs runtime kill + terminal removal only.
- [ ] Return enough outcome data for caller policy decisions.

### 3.2 Refactor lifecycle code
- [ ] Replace `kill_active_terminal_session_and_remove` or split it into narrower helper(s).
- [ ] Remove replacement-terminal selection from lifecycle helper.
- [ ] Remove visibility policy mutation from lifecycle helper.
- [ ] Remove view-focus/reset mutation from lifecycle helper.

### 3.3 Consolidate kill policy in app use case
- [ ] Update `src/app/use_cases/kill_active_agent.rs` to own all replacement-agent selection logic.
- [ ] Ensure focus state, visibility mode, and terminal view updates are decided only there.
- [ ] Reconcile direct-input capture after the new policy flow.

### 3.4 Add/adjust tests
- [ ] Characterization tests for active-agent replacement after kill.
- [ ] Tests for visibility policy after kill in show-all vs focused-only modes.
- [ ] Tests for no-replacement / last-agent removal behavior.
- [ ] Tests that lifecycle helper alone does not impose app policy.

### 3.5 Phase 3 validation
- [ ] Verify terminal lifecycle helpers no longer mutate app-level replacement policy.
- [ ] Run validation commands.

---

## Phase 4 — make HUD read view-models, not raw domain/runtime state

### 4.1 Audit remaining HUD leakage
- [ ] Inventory raw domain/runtime reads in `src/hud/input.rs`.
- [ ] Inventory raw domain/runtime reads in `src/hud/render.rs`.
- [ ] Classify each dependency as: required UI data, accidental policy leak, or removable legacy coupling.

### 4.2 Expand view-model set where needed
- [ ] Define or extend `AgentListView` for all row interaction/render needs.
- [ ] Define or extend `ConversationListView` for selection/focus/status needs.
- [ ] Define or extend `ThreadView` for thread pane rendering.
- [ ] Add `ComposerView` if modal UI still reaches into raw catalog/runtime state.
- [ ] Add `ActiveTerminalView` / `TerminalChromeView` if HUD still needs active terminal presentation metadata.
- [ ] Add task-related derived view if current task modal/render still depends on notes/runtime state.

### 4.3 Move derivation into dedicated sync systems
- [ ] Create or update view-model sync systems in app/UI layer.
- [ ] Ensure widget-facing resources are derived from authoritative state only.
- [ ] Ensure widgets stop doing ad-hoc label/status lookup from raw stores.

### 4.4 Simplify HUD input
- [ ] Remove direct `TerminalManager` lookups from ordinary widget interaction code.
- [ ] Remove direct `TerminalFocusState` dependence for ordinary command selection.
- [ ] Remove direct `TerminalPresentationStore`/`TerminalViewState` dependence for ordinary HUD logic.
- [ ] Keep only narrowly justified raw dependencies, if any, behind explicit adapter/view types.

### 4.5 Simplify HUD render
- [ ] Refactor widget content rendering to use view-model resources only.
- [ ] Refactor modal label/target rendering to use derived composer/task/agent display data.
- [ ] Reduce `HudRenderInputs` to view-model/session/UI concerns.

### 4.6 Add/adjust tests
- [ ] View-model derivation tests for agent list, conversation list, thread pane, composer/task views.
- [ ] Regression tests proving row identity and clicks are agent-keyed.
- [ ] Tests that conversation widget behavior no longer depends on `TerminalId`.

### 4.7 Phase 4 validation
- [ ] Verify HUD widget interaction/render no longer depends on raw runtime/domain stores except justified exceptions.
- [ ] Run validation commands.

---

## Phase 5 — remove the closed-world widget state model

### 5.1 Design per-widget retained state split
- [ ] Identify state currently stored in `HudModuleModel` variants.
- [ ] For each widget, define a dedicated retained UI state type/resource.
- [ ] Keep generic shell/layout/z-order state in layout/session resources only.

### 5.2 Refactor layout state
- [ ] Remove `HudModuleModel` from `src/hud/state.rs`.
- [ ] Update `HudModuleInstance` or equivalent so it stores shell data only.
- [ ] Keep widget registry metadata-only.

### 5.3 Refactor widget dispatch/render/input plumbing
- [ ] Replace enum-pattern routing in `src/hud/modules/mod.rs`.
- [ ] Wire each widget to its own state resource/system.
- [ ] Update hover/scroll/click handling to use per-widget state.
- [ ] Update bloom/effects code that currently pattern-matches `HudModuleModel`.

### 5.4 Update persistence/layout flows
- [ ] Ensure widget shell persistence remains intact after state split.
- [ ] Ensure reset/default behavior still works via registry defaults + widget-specific state reset.

### 5.5 Add/adjust tests
- [ ] Widget registry tests after removal of `HudModuleModel`.
- [ ] Widget reset/enable/drag/hover regression tests.
- [ ] Extensibility test proving a new widget state can be added without editing a shared enum.

### 5.6 Phase 5 validation
- [ ] Verify no `HudModuleModel` references remain.
- [ ] Run validation commands.

---

## Phase 6 — finish composer migration

### 6.1 Replace wrapper ownership
- [ ] Inspect `src/ui/composer/mod.rs` for remaining `HudMessageBoxState` / `HudTaskDialogState` embedding.
- [ ] Define direct composer-owned structures for draft, selection, target, mode, and visibility.
- [ ] Move editor behavior into composer-owned types.

### 6.2 Remove old modal ownership semantics
- [ ] Refactor HUD modal render code to consume composer-owned state.
- [ ] Refactor HUD modal input code to consume composer-owned state.
- [ ] Rename stale message-box/task-dialog terminology where it still implies old ownership.

### 6.3 Update submit/cancel flow
- [ ] Ensure composer submit maps to the new command taxonomy (`ConversationCommand`, `TaskCommand`, `AgentCommand`, `TerminalCommand`).
- [ ] Ensure cancel/discard/unbind behaviors operate purely on composer state.
- [ ] Verify keyboard precedence still holds: composer > direct input > normal shortcuts.

### 6.4 Delete or shrink legacy modal types
- [ ] Remove `HudMessageBoxState` / `HudTaskDialogState` if obsolete.
- [ ] If a small render helper remains, ensure it no longer owns behavior/state.

### 6.5 Add/adjust tests
- [ ] Draft identity tests against composer-owned types.
- [ ] Submit/cancel tests across all composer modes.
- [ ] Precedence tests.
- [ ] Regression tests for task/message draft restoration and target rebinding behavior.

### 6.6 Phase 6 validation
- [ ] Verify `ComposerState` no longer wraps historical modal state as ownership model.
- [ ] Run validation commands.

---

## Phase 7 — clean use-case layout and naming

### 7.1 Restructure use-case files
- [ ] Keep `conversation.rs` focused on message/conversation behavior.
- [ ] Create/keep `tasks.rs` for task operations.
- [ ] Create/keep `terminals.rs` for terminal command/reset/display-mode operations.
- [ ] Create/keep `widgets.rs` for widget toggle/reset behavior.
- [ ] Ensure restore/spawn/focus/kill/composer use cases remain in clearly named modules.

### 7.2 Update exports and references
- [ ] Adjust `src/app/use_cases/mod.rs`.
- [ ] Update imports across dispatch/tests.
- [ ] Remove stale comments/docs referring to old file responsibility.

### 7.3 Add/adjust tests
- [ ] Add lightweight smoke tests or compile-level coverage for moved functions if needed.
- [ ] Re-run targeted regressions after module movement.

### 7.4 Phase 7 validation
- [ ] Verify file/module names match actual responsibility.
- [ ] Run validation commands.

---

## Phase 8 — fix render regressions and close CI

### 8.1 Investigate failing tests
- [ ] Reproduce the 2 failing render/reference tests in isolation.
- [ ] Determine whether failures come from grid sizing, projection geometry, resize timing, or cache invalidation.
- [ ] Document expected vs actual behavior before changing references.

### 8.2 Fix underlying rendering issue
- [ ] Repair raster/projection/grid synchronization as needed.
- [ ] Repair cached switch-frame retention semantics if broken.
- [ ] Update related code comments/invariants to describe the intended behavior.

### 8.3 Re-run visual/regression suite
- [ ] Re-run the 2 targeted tests.
- [ ] Re-run broader rendering/offscreen suite.
- [ ] Update reference assets only if behavior change is intentional and verified.

### 8.4 Final full validation
- [ ] `cargo test`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo fmt --check`
- [ ] relevant offscreen/visual suite

### 8.5 Final cleanup pass
- [ ] Remove any last compatibility code left from the migration.
- [ ] Confirm there are no stale comments mentioning deleted shims.
- [ ] Confirm no debug artifacts were introduced.

---

## Cross-phase deletion checklist
- [ ] Delete runtime `HudIntent` flow.
- [ ] Delete `translate_hud_intents_to_app_commands`.
- [ ] Delete task variants from `ConversationCommand`.
- [ ] Delete direct dual writes between `AgentTaskStore` and `TerminalNotesState`.
- [ ] Delete app-policy behavior from terminal lifecycle helpers.
- [ ] Delete `HudModuleModel`.
- [ ] Delete composer ownership wrapping around `HudMessageBoxState` / `HudTaskDialogState`.
- [ ] Delete obsolete legacy tests/helpers built around old command vocabulary.

---

## Completion gate
- [ ] Single runtime command surface only.
- [ ] Single-owner task domain only.
- [ ] App-level kill/replacement policy owned only by app use cases.
- [ ] HUD/widget layer driven by view-models and session state.
- [ ] No closed-world widget model remains.
- [ ] Composer is first-class and direct.
- [ ] Full validation suite is green.
