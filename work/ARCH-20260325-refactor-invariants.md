# Architecture refactor invariants

Recorded before the large follow-up refactor.

## Authority split
- `TerminalManager` is currently the terminal domain authority:
  - terminal identity
  - registry membership
  - creation order
  - focus order
  - active terminal
  - per-terminal runtime snapshot / pending damage / requested dimensions
- `TerminalPresentationStore` is the presentation projection authority:
  - terminal image handle
  - texture state
  - display mode
  - panel/frame entity links
  - uploaded revision
- `HudState` is the retained HUD authority in the pre-split design:
  - module shells/models/z-order/drag
  - modal editors
  - direct terminal input capture
- ECS entities and Vello scenes are projections, not the primary source of truth.

## Startup invariants
- normal startup restores/imports daemon sessions
- verifier startup bypasses restore/import and creates its own deterministic session
- focus and visibility remain separate concerns
- startup focus prefers persisted focused restore, then first restored, then first imported session

## Redraw invariants
- redraw can be requested by:
  - pending terminal work
  - presentation animation
  - HUD drag/animation
- redraw should remain reactive rather than continuous when idle

## Persistence invariants
- UI metadata persistence is best-effort and non-fatal
- durable continuity is session/process-based, never `TerminalId`-based
- HUD layout persistence and terminal session persistence remain separate concerns

## Phase 1 / 2 non-goals that were preserved
- no UX changes for spawn/focus/isolate/reset view/direct input/task dialogs
- no daemon protocol change
- no renderer/perf change
- no persistence format change

## Structural changes completed in Phases 1 / 2
- app bootstrap, schedule wiring, and startup restore logic were split out of `scene.rs`
- HUD intent fanout and command handlers were split out of the monolithic dispatcher file by concern
- behavior and public call surface were preserved
