# PROJECT_RULES

## General engineering rules

- No hacks, no speculative architecture, no demo-shaped dead ends
- For verification, do not auto-run/open app windows unless explicitly asked.
- work/ and refs/ are never committed
- TODOs must elaborate every single task required to execute a feature end-to-end. Make sure decisions are made before starting it. A high-level document is otherwise a PLAN
- TODOs must be fully self-contained: they must have all the information to execute everything by just reading it
- For every task in a TODO, each of these subtasks must always be added:
  - Identify the best way to implement it preserving or improving the architecture. Prepare the approach by respecting the rules of the project and making sure the architecture is well understood
  - Add regressions tests for the features that will be touched, specifically for the aspects of it that we don't want that change
  - Red/green TDD: For the new features: add red tests. We must ensure those tests fail. Do not move to next step until this is done exhaustively
  - Implement the feature. This is the core step in the TODO
  - Red/green TDD: Ensure the red tests are now green. Testing must be thorough, deep and rigorous. Do not skip this.
  - Review the test and the code to test code together
  - Do a supervision verification, with tough and high standards of quality:
    - All the project rules are respected
    - No code smells or hacks introduced
    - DRY
    - Architecture is respected
    - Is it deeply and thoroughly tested?
    - We are not introducing drifting in any previous aspect of the code
    - Every piece of code is placed in the most sensible location. Not just where is immediately convenient, but where it is most clean and reasonable.
    - Performance
  - Review must be thorough. Ask the question yourself: did I do my best?
  - Iterate implementation until the supervision step is satisfactory

## Architecture

- Clean and pristine architecture.
- Clear spine
- Truthful boundaries
- No split brain
- No unjustified repeated code
- File count / split justification
- Naming
- No bootstrapping garbage in active path
- Maintain always a clear flow of information, data-driven.
- Isolate platform-specific code cleanly
- Alignment to the PLAN file

## Code quality

- Prefer minimal clean module boundaries over giant files
- Module layout rule: use a directory module only when it represents a real semantic namespace; if a module has no submodules, prefer a single `name.rs` file. `mod.rs` may contain module docs, submodule declarations, and small curated re-exports that define the namespace surface. Do not put substantive implementation logic in `mod.rs`; that code should go into its own file/module.
- Naming rule: if a module primarily exists to hold one major type, the file name should match the type name clearly (e.g. `RenderState` -> `render_state.rs`)
- Add inline comments for key operations / tricky logic
- Add doc comments for functions / important APIs, in all languages and parts of the project
- For hand-authored shader files, follow the `terrain_heightfield.wgsl` comment standard:
  - keep comments dense, technical, and tutorial-grade
  - explain the math/geometry/model, not just restate the code
  - explain why a mapping / interpolation / constant exists
  - explain important approximations and tradeoffs
  - record bug-history/rationale when a line exists because of a real failure mode
  - prefer section/function comments over noisy line-by-line narration
  - comments must preserve behavior exactly; comment passes are not excuses for semantic rewrites
  - new substantial shader logic should ship with this level of commentary, not as an undocumented block
- Use the project `Error` / `Result<T>` types for fallible project code; do not introduce ad-hoc string error returns
