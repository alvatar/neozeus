# Plan: complete the state-driven architecture migration

## Purpose

The previous refactor moved the codebase in the right direction but stopped with major compatibility layers still active. This plan does **not** redefine the architecture. It finishes the original plan by removing the remaining split ownership, dual command paths, and HUD/domain leakage.

Target end state remains:

```text
Input/UI -> AppCommand -> use-case handler -> authoritative state + adapters
Authoritative state -> UI view-models -> HUD widgets
Authoritative state -> render projections -> ECS/render entities
```

This plan is intentionally narrower than the original architecture plan: it covers only the missing work required to make the original design true in code.

---

## Current gaps to close

### 1. Two command surfaces still exist
- `HudIntent` is still emitted from HUD/input code.
- `translate_hud_intents_to_app_commands` is still in the runtime schedule.
- New widgets still speak terminal-keyed HUD vocabulary instead of app commands.

### 2. Task ownership is still split
- `AgentTaskStore` exists.
- `TerminalNotesState` is still mutated as parallel truth from task use cases.
- task mutation logic currently synchronizes two stores instead of one owner + one adapter.

### 3. Task commands are routed through the wrong domain
- `AppendTask` / `PrependTask` / `ClearDoneTasks` / `ConsumeNextTask` live under `ConversationCommand`.
- This keeps task policy coupled to the messaging command surface.

### 4. Kill/focus policy is still split across layers
- `kill_active_terminal_session_and_remove` still decides replacement focus/visibility/view policy.
- `kill_active_agent` then applies agent-level replacement policy on top.
- Runtime/lifecycle helpers still own product behavior they should not own.

### 5. HUD still depends on raw stores/managers
- `hud/input.rs` reads terminal/domain resources directly to decide user actions.
- `hud/render.rs` still consumes raw terminal/domain stores in addition to view-models.
- This weakens the state-driven boundary and keeps HUD coupled to internals.

### 6. Widget architecture is still partially closed-world
- `HudModuleModel` remains the central sum type for widget-local state.
- Adding a widget still requires enum expansion and central routing edits.
- Registry exists, but extensibility is incomplete.

### 7. Composer migration is incomplete
- Composer still wraps `HudMessageBoxState` / `HudTaskDialogState` instead of fully replacing them.
- Modal UI is still shaped by old types and terminology.

### 8. Use-case layout is muddy
- `src/app/use_cases/conversation.rs` contains unrelated terminal/view/widget handlers.
- Some boundaries are conceptually right but physically unclear.

### 9. CI is still blocked by 2 render regressions
- Pixel/reference tests fail after terminal projection/grid changes.
- This is not architectural, but completion is not real while CI stays red.

---

## Non-negotiable end state

When this plan is done, the following must be true:

1. `HudIntent` does not participate in runtime command flow.
2. Task state has one owner: `AgentTaskStore`.
3. `TerminalNotesState` is updated only as persistence/interop output from task state, never as peer truth.
4. Task commands have their own command subtype.
5. Runtime helpers do runtime I/O + local runtime state only; app use cases own replacement/focus/visibility policy.
6. HUD input/render consume view-models and session/UI state, not raw domain/runtime stores, except for tightly-justified adapter data not yet modeled.
7. Widget-local retained state is no longer centralized in a closed-world `HudModuleModel` enum.
8. Composer owns its own state model directly.
9. All characterization + relevant regression tests pass, including the 2 current render failures.

---

## Work plan

## Phase 1 — collapse to one command surface

### Objective
Remove `HudIntent` from real runtime flow. HUD and input code must produce `AppCommand` directly.

### Changes
- Delete runtime use of:
  - `src/hud/messages.rs`
  - `translate_hud_intents_to_app_commands` in `src/app/dispatch.rs`
  - schedule wiring in `src/app/schedule.rs`
- Replace HUD/input emission sites to write `AppCommand` directly:
  - `src/hud/input.rs`
  - `src/hud/modules/debug_toolbar/input.rs`
  - `src/hud/modules/agent_list/interaction.rs`
  - `src/hud/modules/conversation_list.rs`
- Replace terminal-keyed interaction outputs with app/product identity:
  - focus row click -> `AppCommand::Agent(AgentCommand::Focus(agent_id))`
  - isolate/inspect row click -> `AppCommand::Agent(AgentCommand::Inspect(agent_id))`
  - widget toggles -> `AppCommand::Widget(...)`
  - terminal command actions -> `AppCommand::Terminal(...)`
  - task actions -> new `AppCommand::Task(...)`
- Keep `HudIntent` only in tests temporarily if needed, then delete it there too.

### Acceptance criteria
- No runtime system reads `MessageReader<HudIntent>`.
- No runtime system writes `MessageWriter<HudIntent>`.
- `src/app/schedule.rs` contains a single command path from UI/HUD/input into `AppCommand` handling.
- New widgets no longer emit terminal-keyed HUD intents.

### Notes
Do this first. As long as `HudIntent` exists in runtime flow, every later cleanup still has two vocabularies.

---

## Phase 2 — give tasks a real domain boundary

### Objective
Make task state single-owner and move task commands out of conversations.

### Changes
- Add `TaskCommand` to `src/app/commands.rs`:
  - `SetText { agent_id, text }`
  - `Append { agent_id, text }`
  - `Prepend { agent_id, text }`
  - `ClearDone { agent_id }`
  - `ConsumeNext { agent_id }`
- Extend `AppCommand` with `Task(TaskCommand)`.
- Remove task variants from `ConversationCommand`.
- Split task handlers out of `src/app/use_cases/conversation.rs` into `src/app/use_cases/tasks.rs`.
- Make `AgentTaskStore` the only authoritative task state.
- Stop task use cases from directly treating `TerminalNotesState` as co-owned truth.

### Required ownership change
Replace current pattern:

```rust
AgentTaskStore <-> TerminalNotesState
```

with:

```text
AgentTaskStore -> task persistence / notes sync adapter -> TerminalNotesState or persisted note storage
```

### Implementation shape
- Task use cases mutate `AgentTaskStore` only.
- A dedicated sync/persistence step projects current task text into terminal/session notes if that compatibility output still matters.
- If notes are only persistence compatibility, hide that inside a narrow helper or adapter, not inline in every task mutation handler.

### Acceptance criteria
- `ConversationCommand` contains only conversation/message actions.
- No task use-case takes both `AgentTaskStore` and `TerminalNotesState` as co-equal mutable inputs for the same concept.
- `AgentTaskStore` is the only source used to derive current task text for UI and behavior.

### Notes
This is the most important correctness cleanup. Do not postpone it behind widget cleanup.

---

## Phase 3 — move product policy out of terminal lifecycle helpers

### Objective
Make runtime/lifecycle helpers stop deciding app behavior.

### Changes
- Refactor `src/terminals/lifecycle.rs`:
  - `kill_active_terminal_session_and_remove` should stop deciding:
    - replacement focus
n    - visibility mode
    - view reset/focus policy
  - replace with a narrower operation that:
    - identifies active terminal/session
    - performs kill I/O
    - removes runtime-local terminal state
    - returns enough info for caller policy
- Move replacement selection + visibility/focus/view policy fully into:
  - `src/app/use_cases/kill_active_agent.rs`
- If needed, add a non-policy runtime helper like:
  - `kill_terminal_session_and_remove(terminal_id, ...) -> Result<KillTerminalOutcome, String>`

### Acceptance criteria
- terminal lifecycle helpers do not mutate `TerminalVisibilityState` policy or choose replacement terminals/agents.
- `kill_active_agent` is the single owner of active-agent replacement behavior.
- kill tests prove behavior through the use case, not helper side effects.

---

## Phase 4 — make HUD read view-models, not raw domain/runtime state

### Objective
Finish the view-model boundary for HUD input/render.

### Changes
- Audit all raw HUD reads in:
  - `src/hud/input.rs`
  - `src/hud/render.rs`
- Introduce/expand derived models as needed:
  - `AgentListView`
  - `ConversationListView`
  - `ThreadView`
  - `ComposerView`
  - `ActiveTerminalView` or `TerminalChromeView` if HUD still needs active terminal status/display metadata
  - `TaskView` if task modal/render still depends on raw notes/runtime state
- Move UI-facing derivation into dedicated sync systems.
- Restrict HUD modules and modal rendering to view-models + session state + widget-local retained UI state.

### Specific fixes
- Agent list click/hover must operate on `AgentId` rows from `AgentListView`, not `TerminalId` row state.
- Conversation list click/hover must operate on `AgentId` / conversation ids from `ConversationListView`.
- Composer/modal rendering should resolve display labels via a view-model rather than raw catalog/runtime lookups inside draw code.

### Acceptance criteria
- `hud/input.rs` does not depend on `TerminalManager`, `TerminalFocusState`, `TerminalPresentationStore`, or `TerminalViewState` for ordinary widget interaction.
- `hud/render.rs` uses view-model resources for widget content and modal labels/status.
- Any remaining raw-store HUD dependency is explicitly justified and isolated behind a named adapter/view.

### Notes
Read-only render leakage is less severe than input leakage, but both should be cleaned.

---

## Phase 5 — remove the closed-world widget state model

### Objective
Finish widget extensibility without inventing a framework.

### Changes
- Remove `HudModuleModel` from `src/hud/state.rs`.
- Keep shell/layout generic in layout/session state only.
- Move widget-local retained state into ordinary per-widget resources/types, e.g.:
  - `AgentListUiState`
  - `ConversationListUiState`
  - `ThreadPaneUiState`
  - `DebugToolbarUiState`
- Change module dispatchers so they route by widget key + dedicated widget state resource, not by enum variants.
- Keep registry metadata-only.

### Implementation constraints
- No dynamic trait registry.
- No callback-based widget framework.
- Ordinary systems/resources only.

### Acceptance criteria
- Adding a widget does not require adding a new `HudModuleModel` enum variant.
- Widget shell state remains generic.
- Widget-local state lives with the widget, not in a shared sum type.

### Notes
Do this after command + task cleanup. It is important, but less correctness-critical.

---

## Phase 6 — finish composer migration

### Objective
Make composer a first-class state model instead of a wrapper around historical modal state.

### Changes
- Replace direct embedding of:
  - `HudMessageBoxState`
  - `HudTaskDialogState`
  inside `src/ui/composer/mod.rs`
- Introduce direct composer-owned state for:
  - draft text
  - cursor/selection state
  - target identity
  - mode/action semantics
  - visibility/open session
- Rename remaining message-box/task-dialog vocabulary toward composer terminology.
- Update HUD modal render/input to consume composer-owned state.

### Acceptance criteria
- `ComposerState` does not wrap old modal state types.
- old modal types are deleted or reduced to temporary render helpers with no ownership semantics.
- composer tests validate draft identity and submit/cancel behavior against composer types directly.

---

## Phase 7 — clean use-case layout and naming

### Objective
Make module/file layout match actual responsibility.

### Changes
- Break `src/app/use_cases/conversation.rs` into focused modules:
  - `conversation.rs` -> message/domain conversation behavior only
  - `tasks.rs` -> task mutation behavior
  - `terminals.rs` -> terminal command/reset/display-mode behavior
  - `widgets.rs` -> toggle/reset widget behavior
- Update `src/app/use_cases/mod.rs` exports accordingly.

### Acceptance criteria
- no conversation file contains widget toggling or terminal-view handlers.
- use-case files are named by domain/intent, not historical convenience.

---

## Phase 8 — fix the 2 render regressions and close CI

### Objective
Get the migration actually green.

### Changes
- Investigate and fix:
  - `rendered_pi_screen_matches_reference_per_character_pixels`
  - `sync_terminal_texture_keeps_cached_switch_frame_until_resized_surface_arrives`
- Confirm whether failure source is:
  - grid sizing
  - terminal projection geometry
  - resize synchronization
  - raster cache invalidation
- Update reference data only if the new behavior is intentionally correct and verified.

### Acceptance criteria
- `cargo test` passes fully.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo fmt --check` passes.
- relevant offscreen/visual suite passes if still part of the expected gate.

---

## Execution order

Do the work in this order:

1. Phase 1 — one command surface
2. Phase 2 — single-owner tasks + `TaskCommand`
3. Phase 3 — kill policy cleanup
4. Phase 4 — HUD/view-model boundary cleanup
5. Phase 5 — remove `HudModuleModel`
6. Phase 6 — finish composer migration
7. Phase 7 — use-case file cleanup
8. Phase 8 — final render regressions / CI closure

Reasoning:
- phases 1-3 remove the biggest architectural lies
- phase 4 makes the HUD boundary real
- phases 5-6 finish the remaining partial migrations
- phase 8 is the final truth test

---

## Verification plan

### Characterization/regression coverage to add or strengthen
- command emission tests from HUD/widgets now assert `AppCommand`, not `HudIntent`
- agent-list click/hover/focus tests use `AgentId`
- conversation-list click tests no longer depend on `TerminalId`
- task mutation tests assert `AgentTaskStore` is sole truth
- persistence/sync tests cover task -> notes/session compatibility output if kept
- kill-active-agent tests assert replacement focus/visibility from the use case only
- composer tests assert direct composer state ownership
- widget extensibility test proves new widget state can be added without shared enum edits
- render regression tests for the current 2 failing cases

### Required validation after each phase
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt --check`

For any phase that touches rendering/projection:
- run the relevant offscreen/visual subset immediately, not only at the end

---

## Deletion checklist

These are the compatibility artifacts this plan is expected to delete by the end:
- runtime `HudIntent` flow
- `translate_hud_intents_to_app_commands`
- task variants inside `ConversationCommand`
- direct task co-ownership between `AgentTaskStore` and `TerminalNotesState`
- replacement-policy logic from terminal lifecycle helpers
- `HudModuleModel`
- composer wrapping of `HudMessageBoxState` / `HudTaskDialogState`
- stale tests that still encode the legacy command vocabulary

---

## Final completion criteria

This plan is complete only if all of the following are true at once:

- architecture matches the original command/state/view/projection split in real runtime code
- no dual command path remains
- no split task truth remains
- no runtime helper owns app-level replacement policy
- HUD/widget code is view-model driven
- widget state is not centralized in a closed-world enum
- composer is first-class
- full test + lint + fmt + relevant visual validation are green
