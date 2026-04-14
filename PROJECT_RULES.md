# PROJECT_RULES

## General engineering rules

- No hacks, no speculative architecture, no demo-shaped dead ends
- Isolate platform-specific code cleanly
- Keep OS/backend details at platform boundaries; keep renderer core platform-agnostic
- For verification, do not auto-run/open app windows unless explicitly asked. If you run web verification, always close the window after verification and shutdown the server
- work/ is never committed
- TODOs must elaborate every single task required to execute a feature end-to-end. Make sure decisions are made before starting it. A high-level document is otherwise a PLAN
- TODOs must be fully self-contained: they must have all the information to execute everything by just reading it
- For every task in a TODO, each of these subtasks must always be added:
  - Identify the best way to implement it preserving or improving the architecture. Prepare the approach by respecting the rules of the project and making sure the architecture is well understood
  - Add regressions tests for the features that will be touched, specifically for the aspects of it that we don't want that change
  - Red/green TDD: For the new features: add red tests. We must ensure those tests fail. Do not move to next step until this is done exhaustively
  - Implement the feature. This is the core step in the TODO
  - Red/green TDD: Ensure the red tests are now green
  - Review the test and the code to test code together
  - Do a supervision verification, with tough and high standards of quality:
    - All the project rules are respected
    - No code smells or hacks introduced
    - DRY
    - Architecture is respected
    - We are not introducing drifting in any previous aspect of the code
    - Every piece of code is placed in the most sensible location. Not just where is immediately convenient, but where it is most clean and reasonable.
    - Performance
  - Iterate implementation until the supervision step is satisfactory

## Code style

- Prefer minimal clean module boundaries over giant files
- Module layout rule: use `mod.rs` only for directory namespace modules; if a module has no submodules, prefer a single `name.rs` file
- Naming rule: if a module primarily exists to hold one major type, the file name should match the type name clearly (e.g. `RenderState` -> `render_state.rs`)
- Add inline comments for key operations / tricky logic
- Add doc comments for functions / important APIs, in all languages and parts of the project
- Use the project `Error` / `Result<T>` types for fallible project code; do not introduce ad-hoc string error returns
