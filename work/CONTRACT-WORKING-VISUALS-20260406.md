# Working visuals contract

Source of truth
- `src/agents/status.rs::AgentStatusStore`
- terminal -> agent mapping via `src/agents/runtime_index.rs`

Consumers
- Agent list row palette: `src/hud/view_models.rs` -> `src/hud/modules/agent_list/render.rs`
- Terminal frame chrome: `src/terminals/presentation.rs::sync_terminal_panel_frames`
- Redraw gate: `src/startup.rs::request_redraw_while_visuals_active`

Required behavior
1. Pi/Claude/Codex/Terminal activity becomes `Working` in `AgentStatusStore`
2. Selected agent row reflects working palette/bloom
3. Interactive working terminal shows working frame chrome
4. One redraw is requested on contract transition even if terminal upload is already caught up
5. Stable signatures do not keep redraw loop alive

Tests enforcing contract
- status derivation: `src/agents/status.rs`
- HUD projection: `src/hud/view_models/tests.rs::sync_hud_view_models_carries_agent_working_status_into_rows`
- terminal chrome: `src/terminals/presentation/tests.rs::working_terminal_shows_green_status_frame`
- redraw transition: `src/tests/scene.rs::working_agent_row_transition_requests_redraw_for_hud_feedback`
- redraw stability: `src/tests/scene.rs::stable_visual_contract_does_not_request_continuous_redraws`
