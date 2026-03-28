# TODO: complete the state-driven architecture migration

Source plan: `work/PLAN-20260328-complete-architecture-migration.md`

## Global execution rules
- [x] Preserve behavior unless a task explicitly changes product semantics.
- [x] Add or update tests for every meaningful architectural change before marking a phase done.
- [x] Delete compatibility shims as soon as their replacement path is proven.
- [x] Do not leave dual ownership or dual command paths temporarily “for later” once a phase starts.
- [x] After each phase run:
  - [x] `cargo test`
  - [x] `cargo clippy --all-targets -- -D warnings`
  - [x] `cargo fmt --check`
- [x] For rendering/projection-affecting phases, also run the relevant offscreen/visual subset immediately.

---

## Phase 1 — collapse to one command surface

### 1.1 Audit runtime `HudIntent` usage
- [x] Enumerate all non-test `HudIntent` writers.
- [x] Enumerate all non-test `HudIntent` readers.
- [x] Identify every runtime schedule edge that still routes through `translate_hud_intents_to_app_commands`.

### 1.2 Replace HUD/input emission with direct `AppCommand`
- [x] Change `src/hud/input.rs` to write `AppCommand` instead of `HudIntent`.
- [x] Replace task action emission from message/task modal interactions with direct app/task commands.
- [x] Replace focus/isolate click handling with direct `AppCommand::Agent(...)` emission.
- [x] Replace widget toggle/reset emission with direct `AppCommand::Widget(...)`.
- [x] Replace debug-toolbar actions with direct app commands.

### 1.3 Remove terminal-keyed HUD vocabulary from widgets
- [x] Update `src/hud/modules/agent_list/interaction.rs` so row clicks emit agent-keyed commands.
- [x] Update `src/hud/modules/conversation_list.rs` so row clicks emit agent/conversation-keyed commands.
- [x] Ensure no widget interaction emits `TerminalId`-based product actions except terminal-command-specific flows.

### 1.4 Delete runtime translation layer
- [x] Remove `translate_hud_intents_to_app_commands` from `src/app/dispatch.rs`.
- [x] Remove runtime `HudIntent` schedule wiring from `src/app/schedule.rs`.
- [x] Remove `HudIntent` message registration from bootstrap/runtime setup if no longer needed.
- [x] Delete `src/hud/messages.rs` if fully unused.

### 1.5 Update tests to new command surface
- [x] Replace HUD/input tests that currently assert `HudIntent` emission with `AppCommand` assertions.
- [x] Replace helper utilities that drain `HudIntent` messages.
- [x] Update characterization tests for spawn/focus/show-all/inspect/task actions.

### 1.6 Phase 1 validation
- [x] Verify there are no non-test `HudIntent` references left in runtime code.
- [x] Run validation commands.

---

## Phase 2 — give tasks a real domain boundary

### 2.1 Reshape command model
- [x] Add `TaskCommand` to `src/app/commands.rs`.
- [x] Add `AppCommand::Task(TaskCommand)`.
- [x] Move task variants out of `ConversationCommand`.
- [x] Update imports/re-exports and pattern matches in dispatch/handlers/tests.

### 2.2 Separate task use cases from conversation use cases
- [x] Create `src/app/use_cases/tasks.rs`.
- [x] Move `set_task_text`, `append_task`, `prepend_task`, `clear_done_tasks`, `consume_next_task` into it.
- [x] Restrict `src/app/use_cases/conversation.rs` to message/conversation behavior only.
- [x] Update `src/app/use_cases/mod.rs` exports.

### 2.3 Remove dual task truth
- [x] Decide exact role of `TerminalNotesState`: persistence adapter, compatibility projection, or removable legacy state.
- [x] Remove inline task mutation code that writes both `AgentTaskStore` and `TerminalNotesState` as co-equal truth.
- [x] Introduce a narrow task-to-notes sync helper/system if compatibility output is still required.
- [x] Ensure UI/task rendering reads from `AgentTaskStore`-derived state only.

### 2.4 Rework persistence/sync path
- [x] If session notes persistence is still required, add a one-way projection from `AgentTaskStore` to persisted note/session text.
- [x] Mark persistence dirty from the projection path, not each task mutation use case.
- [x] Ensure restore/bootstrap reconstructs `AgentTaskStore` correctly from persisted source.

### 2.5 Update command dispatch
- [x] Route task commands through `AppCommand::Task` handling.
- [x] Remove task-specific handling from conversation command branches.
- [x] Update composer submit mapping for task-related modes.

### 2.6 Add/adjust tests
- [x] Unit tests for `TaskCommand` routing.
- [x] Domain tests for append/prepend/clear/consume/set-text behavior.
- [x] Regression tests that prove `AgentTaskStore` is sole task truth.
- [x] Persistence/sync tests for task -> notes/session compatibility path if retained.

### 2.7 Phase 2 validation
- [x] Verify no task variant remains under `ConversationCommand`.
- [x] Verify no task use case mutates `TerminalNotesState` as peer truth.
- [x] Run validation commands.

---

## Phase 3 — move product policy out of terminal lifecycle helpers

### 3.1 Narrow terminal lifecycle helper responsibility
- [x] Inspect `src/terminals/lifecycle.rs` for kill/remove helpers that mutate focus/visibility/view policy.
- [x] Design a narrower helper API that performs runtime kill + terminal removal only.
- [x] Return enough outcome data for caller policy decisions.

### 3.2 Refactor lifecycle code
- [x] Replace `kill_active_terminal_session_and_remove` or split it into narrower helper(s).
- [x] Remove replacement-terminal selection from lifecycle helper.
- [x] Remove visibility policy mutation from lifecycle helper.
- [x] Remove view-focus/reset mutation from lifecycle helper.

### 3.3 Consolidate kill policy in app use case
- [x] Update `src/app/use_cases/kill_active_agent.rs` to own all replacement-agent selection logic.
- [x] Ensure focus state, visibility mode, and terminal view updates are decided only there.
- [x] Reconcile direct-input capture after the new policy flow.

### 3.4 Add/adjust tests
- [x] Characterization tests for active-agent replacement after kill.
- [x] Tests for visibility policy after kill in show-all vs focused-only modes.
- [x] Tests for no-replacement / last-agent removal behavior.
- [x] Tests that lifecycle helper alone does not impose app policy.

### 3.5 Phase 3 validation
- [x] Verify terminal lifecycle helpers no longer mutate app-level replacement policy.
- [x] Run validation commands.

---

## Phase 4 — make HUD read view-models, not raw domain/runtime state

### 4.1 Audit remaining HUD leakage
- [x] Inventory raw domain/runtime reads in `src/hud/input.rs`.
- [x] Inventory raw domain/runtime reads in `src/hud/render.rs`.
- [x] Classify each dependency as: required UI data, accidental policy leak, or removable legacy coupling.

### 4.2 Expand view-model set where needed
- [x] Define or extend `AgentListView` for all row interaction/render needs.
- [x] Define or extend `ConversationListView` for selection/focus/status needs.
- [x] Define or extend `ThreadView` for thread pane rendering.
- [x] Add `ComposerView` if modal UI still reaches into raw catalog/runtime state.
- [x] Add `ActiveTerminalView` / `TerminalChromeView` if HUD still needs active terminal presentation metadata.
- [x] Add task-related derived view if current task modal/render still depends on notes/runtime state.

### 4.3 Move derivation into dedicated sync systems
- [x] Create or update view-model sync systems in app/UI layer.
- [x] Ensure widget-facing resources are derived from authoritative state only.
- [x] Ensure widgets stop doing ad-hoc label/status lookup from raw stores.

### 4.4 Simplify HUD input
- [x] Remove direct `TerminalManager` lookups from ordinary widget interaction code.
- [x] Remove direct `TerminalFocusState` dependence for ordinary command selection.
- [x] Remove direct `TerminalPresentationStore`/`TerminalViewState` dependence for ordinary HUD logic.
- [x] Keep only narrowly justified raw dependencies, if any, behind explicit adapter/view types.

### 4.5 Simplify HUD render
- [x] Refactor widget content rendering to use view-model resources only.
- [x] Refactor modal label/target rendering to use derived composer/task/agent display data.
- [x] Reduce `HudRenderInputs` to view-model/session/UI concerns.

### 4.6 Add/adjust tests
- [x] View-model derivation tests for agent list, conversation list, thread pane, composer/task views.
- [x] Regression tests proving row identity and clicks are agent-keyed.
- [x] Tests that conversation widget behavior no longer depends on `TerminalId`.

### 4.7 Phase 4 validation
- [x] Verify HUD widget interaction/render no longer depends on raw runtime/domain stores except justified exceptions.
- [x] Run validation commands.

---

## Phase 5 — remove the closed-world widget state model

### 5.1 Design per-widget retained state split
- [x] Identify state currently stored in `HudModuleModel` variants.
- [x] For each widget, define a dedicated retained UI state type/resource.
- [x] Keep generic shell/layout/z-order state in layout/session resources only.

### 5.2 Refactor layout state
- [x] Remove `HudModuleModel` from `src/hud/state.rs`.
- [x] Update `HudModuleInstance` or equivalent so it stores shell data only.
- [x] Keep widget registry metadata-only.

### 5.3 Refactor widget dispatch/render/input plumbing
- [x] Replace enum-pattern routing in `src/hud/modules/mod.rs`.
- [x] Wire each widget to its own state resource/system.
- [x] Update hover/scroll/click handling to use per-widget state.
- [x] Update bloom/effects code that currently pattern-matches `HudModuleModel`.

### 5.4 Update persistence/layout flows
- [x] Ensure widget shell persistence remains intact after state split.
- [x] Ensure reset/default behavior still works via registry defaults + widget-specific state reset.

### 5.5 Add/adjust tests
- [x] Widget registry tests after removal of `HudModuleModel`.
- [x] Widget reset/enable/drag/hover regression tests.
- [x] Extensibility test proving a new widget state can be added without editing a shared enum.

### 5.6 Phase 5 validation
- [x] Verify no `HudModuleModel` references remain.
- [x] Run validation commands.

---

## Phase 6 — finish composer migration

### 6.1 Replace wrapper ownership
- [x] Inspect `src/ui/composer/mod.rs` for remaining `HudMessageBoxState` / `HudTaskDialogState` embedding.
- [x] Define direct composer-owned structures for draft, selection, target, mode, and visibility.
- [x] Move editor behavior into composer-owned types.

### 6.2 Remove old modal ownership semantics
- [x] Refactor HUD modal render code to consume composer-owned state.
- [x] Refactor HUD modal input code to consume composer-owned state.
- [x] Rename stale message-box/task-dialog terminology where it still implies old ownership.

### 6.3 Update submit/cancel flow
- [x] Ensure composer submit maps to the new command taxonomy (`ConversationCommand`, `TaskCommand`, `AgentCommand`, `TerminalCommand`).
- [x] Ensure cancel/discard/unbind behaviors operate purely on composer state.
- [x] Verify keyboard precedence still holds: composer > direct input > normal shortcuts.

### 6.4 Delete or shrink legacy modal types
- [x] Remove `HudMessageBoxState` / `HudTaskDialogState` if obsolete.
- [x] If a small render helper remains, ensure it no longer owns behavior/state.

### 6.5 Add/adjust tests
- [x] Draft identity tests against composer-owned types.
- [x] Submit/cancel tests across all composer modes.
- [x] Precedence tests.
- [x] Regression tests for task/message draft restoration and target rebinding behavior.

### 6.6 Phase 6 validation
- [x] Verify `ComposerState` no longer wraps historical modal state as ownership model.
- [x] Run validation commands.

---

## Phase 7 — clean use-case layout and naming

### 7.1 Restructure use-case files
- [x] Keep `conversation.rs` focused on message/conversation behavior.
- [x] Create/keep `tasks.rs` for task operations.
- [x] Create/keep `terminals.rs` for terminal command/reset/display-mode operations.
- [x] Create/keep `widgets.rs` for widget toggle/reset behavior.
- [x] Ensure restore/spawn/focus/kill/composer use cases remain in clearly named modules.

### 7.2 Update exports and references
- [x] Adjust `src/app/use_cases/mod.rs`.
- [x] Update imports across dispatch/tests.
- [x] Remove stale comments/docs referring to old file responsibility.

### 7.3 Add/adjust tests
- [x] Add lightweight smoke tests or compile-level coverage for moved functions if needed.
- [x] Re-run targeted regressions after module movement.

### 7.4 Phase 7 validation
- [x] Verify file/module names match actual responsibility.
- [x] Run validation commands.

---

## Phase 8 — fix render regressions and close CI

### 8.1 Investigate failing tests
- [x] Reproduce the 2 failing render/reference tests in isolation.
- [x] Determine whether failures come from grid sizing, projection geometry, resize timing, or cache invalidation.
- [x] Document expected vs actual behavior before changing references.

### 8.2 Fix underlying rendering issue
- [x] Repair raster/projection/grid synchronization as needed.
- [x] Repair cached switch-frame retention semantics if broken.
- [x] Update related code comments/invariants to describe the intended behavior.

### 8.3 Re-run visual/regression suite
- [x] Re-run the 2 targeted tests.
- [x] Re-run broader rendering/offscreen suite.
- [x] Update reference assets only if behavior change is intentional and verified.

### 8.4 Final full validation
- [x] `cargo test`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo fmt --check`
- [x] relevant offscreen/visual suite

### 8.5 Final cleanup pass
- [x] Remove any last compatibility code left from the migration.
- [x] Confirm there are no stale comments mentioning deleted shims.
- [x] Confirm no debug artifacts were introduced.

---

## Cross-phase deletion checklist
- [x] Delete runtime `HudIntent` flow.
- [x] Delete `translate_hud_intents_to_app_commands`.
- [x] Delete task variants from `ConversationCommand`.
- [x] Delete direct dual writes between `AgentTaskStore` and `TerminalNotesState`.
- [x] Delete app-policy behavior from terminal lifecycle helpers.
- [x] Delete `HudModuleModel`.
- [x] Delete composer ownership wrapping around `HudMessageBoxState` / `HudTaskDialogState`.
- [x] Delete obsolete legacy tests/helpers built around old command vocabulary.

---

## Completion gate
- [x] Single runtime command surface only.
- [x] Single-owner task domain only.
- [x] App-level kill/replacement policy owned only by app use cases.
- [x] HUD/widget layer driven by view-models and session state.
- [x] No closed-world widget model remains.
- [x] Composer is first-class and direct.
- [x] Full validation suite is green.
