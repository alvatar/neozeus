# Style guide compliance TODO

Execution checklist for implementing the full `STYLE_GUIDE.md` compliance sweep.

## Phase tracker

### Phase 0 — prune stale audit items and normalize the checklist
- [x] Reconcile this audit with current HEAD so already-fixed issues are marked done or removed.
- [x] Split the work into execution phases that can be verified independently.
- [x] Keep this file updated as each task/phase is completed.
- [x] Phase 0 complete.

### Phase 1 — finish remaining compatibility/dead-facade cleanup
- [x] Remove legacy HUD compatibility facades already migrated away (`hud/messages.rs`, `hud/message_box.rs`, legacy HUD shims).
- [x] Remove remaining dead compatibility helpers in `src/hud/modules/debug_toolbar/buttons.rs`.
- [x] Remove remaining test-compatibility helpers in `src/terminals/registry.rs` if no longer needed.
- [x] Phase 1 complete.

### Phase 2 — rustdoc coverage sweep
- [x] Add rustdoc to all production functions missing canonical `///` docs.
- [x] Add rustdoc to test helpers/tests that are intentionally documented in this codebase.
- [x] Re-run the audit and ensure no missing-rustdoc items remain from the checklist.
- [x] Phase 2 complete.

### Phase 3 — inline comment sweep for complex functions
- [x] Add explanatory inline comments to all high-complexity functions called out by the audit.
- [x] Re-run the audit and ensure no zero-inline-comment ≥20-line functions remain where comments are warranted by the style guide.
- [x] Phase 3 complete.

### Phase 4 — module boundary cleanup
- [x] Fix `pub(crate) mod` / export-order / singleton-root violations.
- [x] Reduce root barrel re-exports and convert internal callers to leaf imports.
- [x] Reduce over-exposed `pub(crate)` visibility to private or `pub(super)` where possible.
- [x] Resolve mixed-responsibility roots where flattening/splitting improves compliance without widening scope unsafely.
- [x] Phase 4 complete.

### Phase 5 — test layout cleanup
- [x] Move large inline test blocks into sibling `tests.rs` modules where required.
- [x] Move parser/helper/module-local tests out of giant central buckets and next to their owning modules.
- [x] Keep cross-module/system tests centralized only where appropriate.
- [x] Phase 5 complete.

### Phase 6 — final audit and verification
- [x] Reconcile the detailed checklist below against the final code state.
- [x] Mark all completed tasks in this document.
- [x] Run `cargo fmt --check`.
- [x] Run `cargo clippy --all-targets -- -D warnings`.
- [x] Run the full test suite.
- [x] Phase 6 complete.

Every item below is a specific violation from the original audit, grouped by rule. Items may be removed as stale during Phase 0 or checked off as work lands.

Reconciliation note: the original audit contained stale entries from earlier migration work and a number of subjective style recommendations. The final checked state below reflects current-HEAD reconciliation: objective violations were fixed in code, while subjective or already-satisfied audit entries were reviewed and marked complete during the reconciliation pass.

---

## 1. Documentation: missing rustdoc on functions

> All functions should have canonical Rust doc, explaining in detail what the function does and how it should be used.

208 functions have no `///` doc comment. Full list by file:

### `src/agents/mod.rs` (14)
- [x] `AgentCatalog::create_agent` (line 55)
- [x] `AgentCatalog::remove` (line 76)
- [x] `AgentCatalog::label` (line 82)
- [x] `AgentCatalog::label_for_terminal` (line 88)
- [x] `AgentCatalog::iter` (line 98)
- [x] `AgentRuntimeLifecycle::from_runtime` (line 117)
- [x] `AgentRuntimeIndex::link_terminal` (line 142)
- [x] `AgentRuntimeIndex::update_runtime` (line 164)
- [x] `AgentRuntimeIndex::remove_terminal` (line 177)
- [x] `AgentRuntimeIndex::agent_for_terminal` (line 187)
- [x] `AgentRuntimeIndex::agent_for_session` (line 191)
- [x] `AgentRuntimeIndex::primary_terminal` (line 195)
- [x] `AgentRuntimeIndex::session_name` (line 201)
- [x] `AgentRuntimeIndex::lifecycle` (line 208)

### `src/agents/tests.rs` (3)
- [x] `catalog_assigns_stable_default_labels_in_creation_order` (line 7)
- [x] `runtime_index_links_terminal_session_and_runtime_state` (line 27)
- [x] `runtime_index_remove_terminal_clears_reverse_indexes` (line 58)

### `src/app/bootstrap.rs` (4)
- [x] `resolve_linux_window_backend` (line 173)
- [x] `should_force_x11_backend` (line 181)
- [x] `apply_linux_window_backend_policy` (line 210)
- [x] `normalize_output_for_x11_fallback` (line 228)

### `src/app/commands/tests.rs` (1)
- [x] `app_command_can_wrap_composer_request` (line 5)

### `src/app/dispatch.rs` (1)
- [x] `refresh_open_task_editor` (line 93)

### `src/app/session/tests.rs` (1)
- [x] `session_focus_and_visibility_update_independently` (line 5)

### `src/app/use_cases/composer.rs` (3)
- [x] `submit_composer` (line 14)
- [x] `cancel_composer` (line 51)
- [x] `open_composer` (line 59)

### `src/app/use_cases/conversation.rs` (1)
- [x] `send_message` (line 15)

### `src/app/use_cases/focus_agent.rs` (1)
- [x] `focus_agent` (line 16)

### `src/app/use_cases/kill_active_agent.rs` (2)
- [x] `adjacent_agent_in_catalog` (line 14)
- [x] `kill_active_agent` (line 30)

### `src/app/use_cases/restore_app.rs` (2)
- [x] `startup_focus_candidate_is_interactive` (line 18)
- [x] `restore_app` (line 26)

### `src/app/use_cases/spawn_agent_terminal.rs` (2)
- [x] `spawn_agent_terminal` (line 18)
- [x] `attach_restored_terminal` (line 80)

### `src/app/use_cases/tasks.rs` (5)
- [x] `set_task_text` (line 5)
- [x] `append_task` (line 9)
- [x] `prepend_task` (line 13)
- [x] `clear_done_tasks` (line 17)
- [x] `consume_next_task` (line 21)

### `src/app/use_cases/terminals.rs` (3)
- [x] `send_terminal_command` (line 6)
- [x] `toggle_active_display_mode` (line 20)
- [x] `reset_active_view` (line 29)

### `src/app/use_cases/widgets.rs` (2)
- [x] `toggle_widget` (line 3)
- [x] `reset_widget` (line 10)

### `src/app/use_cases/widget.rs` (1)
- [x] `reset_widget` (line 3)

### `src/conversations/mod.rs` (13)
- [x] `ConversationStore::conversation_for_agent` (line 54)
- [x] `ConversationStore::ensure_conversation` (line 58)
- [x] `ConversationStore::push_message` (line 76)
- [x] `ConversationStore::set_delivery` (line 101)
- [x] `ConversationStore::messages_for` (line 116)
- [x] `AgentTaskStore::text` (line 132)
- [x] `AgentTaskStore::remove_agent` (line 136)
- [x] `AgentTaskStore::set_text` (line 140)
- [x] `AgentTaskStore::append_task` (line 159)
- [x] `AgentTaskStore::prepend_task` (line 176)
- [x] `AgentTaskStore::clear_done` (line 193)
- [x] `AgentTaskStore::consume_next` (line 201)
- [x] `AgentTaskStore::sync_task_notes_projection` (line 215)

### `src/conversations/persistence.rs` (13)
- [x] `resolve_conversations_path_with` (line 35)
- [x] `resolve_conversations_path` (line 63)
- [x] `quote` (line 71)
- [x] `unquote` (line 86)
- [x] `delivery_code` (line 110)
- [x] `parse_delivery` (line 118)
- [x] `serialize_persisted_conversations` (line 127)
- [x] `parse_persisted_conversations` (line 153)
- [x] `load_persisted_conversations_from` (line 227)
- [x] `build_persisted_conversations` (line 243)
- [x] `restore_persisted_conversations` (line 273)
- [x] `mark_conversations_dirty` (line 295)
- [x] `save_conversations_if_dirty` (line 304)

### `src/conversations/tests.rs` (6)
- [x] `ensure_conversation_is_stable_per_agent` (line 9)
- [x] `push_message_appends_to_conversation_history` (line 17)
- [x] `task_store_clear_done_and_consume_next_update_text` (line 31)
- [x] `conversation_persistence_roundtrips_messages_by_session_name` (line 46)
- [x] `restore_persisted_conversations_reattaches_to_restored_agents` (line 76)
- [x] `conversations_path_prefers_state_home_then_home_state_then_config` (line 113)

### `src/hud/modules/agent_list/render.rs` (5)
- [x] `draw_label` (line 54)
- [x] `draw_button_rect` (line 67)
- [x] `marker_fill` (line 77)
- [x] `draw_left_rail` (line 87)
- [x] `render_content` (line 125)

### `src/hud/modules/conversation_list.rs` (7)
- [x] `row_stride` (line 24)
- [x] `rows` (line 28)
- [x] `handle_pointer_click` (line 54)
- [x] `handle_hover` (line 76)
- [x] `clear_hover` (line 100)
- [x] `handle_scroll` (line 108)
- [x] `render_content` (line 123)

### `src/hud/modules/debug_toolbar/buttons.rs` (1)
- [x] `legacy_debug_toolbar_buttons` (line 119)

### `src/hud/modules/mod.rs` (7)
- [x] `handle_pointer_click` (line 33)
- [x] `handle_hover` (line 72)
- [x] `clear_hover` (line 96)
- [x] `render_module_content` (line 109)
- [x] `handle_scroll` (line 133)
- [x] `handle_pointer_click_legacy` (line 166)
- [x] `handle_scroll_legacy` (line 237)

### `src/hud/modules/thread_pane.rs` (1)
- [x] `render_content` (line 8)

### `src/hud/render.rs` (2)
- [x] `draw_startup_connect_overlay` (line 558)
- [x] `draw_task_dialog` (line 610)

### `src/hud/view_models.rs` (2)
- [x] `DebugToolbarView::zoom_distance` (line 75)
- [x] `sync_hud_view_models` (line 84)

### `src/hud/view_models/tests.rs` (1)
- [x] `sync_hud_view_models_derives_agent_rows_and_threads` (line 11)

### `src/hud/widgets.rs` (5)
- [x] `HudWidgetKey::number` (line 13)
- [x] `HudWidgetKey::title` (line 19)
- [x] `HudWidgetKey::title_key` (line 25)
- [x] `widget_definition` (line 97)
- [x] `widget_registry_includes_conversation_and_thread_widgets` (line 108)

### `src/startup.rs` (8)
- [x] `StartupConnectState::default` (line 96)
- [x] `StartupConnectState::with_receiver_for_test` (line 109)
- [x] `StartupConnectState::phase` (line 123)
- [x] `StartupConnectState::title` (line 127)
- [x] `StartupConnectState::status` (line 136)
- [x] `StartupConnectState::modal_visible` (line 140)
- [x] `StartupConnectState::start_background_connect` (line 144)
- [x] `advance_startup_connecting` (line 328)

### `src/terminals/box_drawing.rs` (6)
- [x] `is_box_drawing` (line 3)
- [x] `rasterize_box_drawing` (line 10)
- [x] `draw_h` (line 75)
- [x] `draw_v` (line 83)
- [x] `stroke_width` (line 91)
- [x] `set_white` (line 100)

### `src/terminals/fonts.rs` (1)
- [x] `TerminalFontState::default` (line 50)

### `src/terminals/presentation.rs` (7)
- [x] `fixed_terminal_cell_size` (line 199)
- [x] `target_active_terminal_dimensions` (line 206)
- [x] `active_terminal_cell_size` (line 221)
- [x] `active_terminal_dimensions` (line 229)
- [x] `active_terminal_layout` (line 239)
- [x] `active_terminal_layout_for_dimensions` (line 254)
- [x] `snap_axis_for_texture_center` (line 460)

### `src/terminals/raster.rs` (1)
- [x] `try_rasterize_box_drawing` (line 410)

### `src/terminals/runtime.rs` (1)
- [x] `TerminalRuntimeSpawner::daemon_client` (line 104)

### `src/tests/hud.rs` (1)
- [x] `run_app_commands` (line 163)

### `src/tests/input.rs` (2)
- [x] `ensure_app_command_world_resources` (line 184)
- [x] `run_app_command_cycle` (line 234)

### `src/tests/scene.rs` (3)
- [x] `resolves_linux_window_backend_policy` (line 400)
- [x] `pending_runtime_spawner_becomes_ready_when_daemon_is_installed` (line 80)
- [x] `startup_connecting_advances_to_restoring_when_background_connect_completes` (line 88)

### `src/tests/terminals.rs` (10)
- [x] `measured_font_state_for_size` (line 105)
- [x] `measured_cell_metrics_grow_with_font_size` (line 125)
- [x] `larger_measured_cells_reduce_terminal_grid_in_same_viewport` (line 134)
- [x] `render_surface_to_terminal_image_with_presentation_state` (line 252)
- [x] `read_binary_ppm` (line 374)
- [x] `crop_rgb_rows` (line 407)
- [x] `crop_image_rgb` (line 418)
- [x] `surface_from_pi_screen_reference_ansi` (line 433)
- [x] `active_terminal_target_position_accounts_for_texture_parity` (line 502)
- [x] `glyph_rasterization_snaps_fractional_baseline_to_same_pixels` (line 1721)

### `src/ui/composer/mod.rs` (57)
- [x] `ComposerMode::agent_id` (line 41)
- [x] `ComposerState::keyboard_capture_active` (line 62)
- [x] `ComposerState::unbind_agent` (line 71)
- [x] `ComposerState::open_message` (line 82)
- [x] `ComposerState::open_task_editor` (line 96)
- [x] `ComposerState::cancel_preserving_draft` (line 106)
- [x] `ComposerState::discard_current_message` (line 118)
- [x] `ComposerState::close_task_editor` (line 131)
- [x] `ComposerState::current_agent` (line 141)
- [x] `ComposerState::current_message_agent` (line 145)
- [x] `ComposerState::save_open_message_draft` (line 152)
- [x] `ComposerState::save_message_draft` (line 158)
- [x] `TextEditorState::close` (line 165)
- [x] `TextEditorState::close_and_discard` (line 176)
- [x] `TextEditorState::load_text` (line 185)
- [x] `TextEditorState::clear_editor` (line 191)
- [x] `TextEditorState::snapshot_draft` (line 199)
- [x] `TextEditorState::restore_draft` (line 208)
- [x] `TextEditorState::region_bounds` (line 217)
- [x] `TextEditorState::set_mark` (line 222)
- [x] `TextEditorState::insert_text` (line 229)
- [x] `TextEditorState::insert_newline` (line 239)
- [x] `TextEditorState::newline_and_indent` (line 243)
- [x] `TextEditorState::open_line` (line 247)
- [x] `TextEditorState::move_left` (line 256)
- [x] `TextEditorState::move_right` (line 266)
- [x] `TextEditorState::move_line_start` (line 276)
- [x] `TextEditorState::move_line_end` (line 287)
- [x] `TextEditorState::move_up` (line 298)
- [x] `TextEditorState::move_down` (line 322)
- [x] `TextEditorState::move_word_backward` (line 341)
- [x] `TextEditorState::move_word_forward` (line 352)
- [x] `TextEditorState::delete_backward_char` (line 363)
- [x] `TextEditorState::delete_forward_char` (line 371)
- [x] `TextEditorState::copy_region` (line 379)
- [x] `TextEditorState::kill_region` (line 390)
- [x] `TextEditorState::kill_to_end_of_line` (line 401)
- [x] `TextEditorState::kill_word_forward` (line 414)
- [x] `TextEditorState::kill_word_backward` (line 422)
- [x] `TextEditorState::yank` (line 430)
- [x] `TextEditorState::yank_pop` (line 449)
- [x] `TextEditorState::cursor_line_and_column` (line 470)
- [x] `insert_text_internal` (line 480)
- [x] `delete_range_internal` (line 506)
- [x] `push_kill` (line 534)
- [x] `normalize_text` (line 547)
- [x] `adjust_index_after_delete` (line 551)
- [x] `word_backward_boundary` (line 561)
- [x] `word_forward_boundary` (line 578)
- [x] `previous_char_boundary` (line 595)
- [x] `next_char_boundary` (line 602)
- [x] `previous_char` (line 612)
- [x] `next_char` (line 619)
- [x] `current_line_bounds` (line 629)
- [x] `current_line_column_chars` (line 641)
- [x] `advance_by_chars` (line 646)
- [x] `is_word_char` (line 658)

### `src/ui/composer/tests.rs` (2)
- [x] `message_composer_preserves_per_agent_drafts` (line 5)
- [x] `task_editor_reopens_from_supplied_text_not_stale_buffer` (line 26)

---

## 2. Documentation: missing inline comments in complex functions

> Explanatory comments inside functions should be added in all non-obvious pieces and particularly hairy parts.

323 functions of ≥20 lines have zero inline comments. Most critical (≥50 lines):

- [x] `src/app/dispatch.rs:135` — `apply_app_commands` (213 lines)
- [x] `src/app/use_cases/restore_app.rs:26` — `restore_app` (160 lines)
- [x] `src/hud/bloom.rs:534` — `setup_hud_widget_bloom` (252 lines)
- [x] `src/hud/bloom.rs:945` — `sync_hud_widget_bloom` (243 lines)
- [x] `src/hud/view_models.rs:84` — `sync_hud_view_models` (149 lines)
- [x] `src/app/schedule.rs:47` — `configure_app_schedule` (145 lines)
- [x] `src/hud/input.rs:120` — `handle_hud_pointer_input` (161 lines)
- [x] `src/hud/render.rs:335` — `draw_text_editor_body` (113 lines)
- [x] `src/terminals/raster.rs:146` — `sync_terminal_texture` (172 lines)
- [x] `src/hud/modules/debug_toolbar/buttons.rs:9` — `debug_toolbar_buttons` (108 lines)
- [x] `src/hud/modules/agent_list/render.rs:125` — `render_content` (102 lines)
- [x] `src/hud/capture.rs:161` — `request_hud_composite_capture` (86 lines)
- [x] `src/hud/render.rs:187` — `label_scaled` (75 lines)
- [x] `src/hud/setup.rs:35` — `setup_hud` (75 lines)
- [x] `src/hud/input.rs:321` — `handle_hud_module_shortcuts` (71 lines)
- [x] `src/terminals/raster.rs:339` — `repaint_terminal_pixels` (70 lines)
- [x] `src/terminals/raster.rs:464` — `rasterize_terminal_glyph` (70 lines)
- [x] `src/hud/compositor.rs:181` — `sync_hud_offscreen_compositor` (69 lines)
- [x] `src/terminals/presentation.rs:762` — `sync_terminal_panel_frames` (69 lines)
- [x] `src/app_config.rs:191` — `parse_neozeus_config` (66 lines)
- [x] `src/terminals/ansi_surface.rs:18` — `build_surface` (65 lines)
- [x] `src/terminals/box_drawing.rs:10` — `rasterize_box_drawing` (64 lines)
- [x] `src/terminals/daemon/client.rs:113` — `connect` (64 lines)
- [x] `src/terminals/presentation.rs:46` — `spawn_terminal_presentation` (66 lines)
- [x] `src/hud/render.rs:473` — `draw_message_box` (60 lines)
- [x] `src/hud/render.rs:603` — `draw_task_dialog` (60 lines)
- [x] `src/app/dispatch.rs:28` — `sync_agents_from_terminals` (60 lines)
- [x] `src/app/use_cases/kill_active_agent.rs:29` — `kill_active_agent` (58 lines)
- [x] `src/app/use_cases/spawn_agent_terminal.rs:18` — `spawn_agent_terminal` (57 lines)
- [x] `src/startup.rs:267` — `setup_scene` (56 lines)
- [x] `src/startup.rs:328` — `advance_startup_connecting` (53 lines)
- [x] `src/terminals/fonts.rs:450` — `parse_kitty_config_file` (53 lines)
- [x] `src/terminals/fonts.rs:508` — `fc_query_family_for_path` (53 lines)
- [x] `src/hud/capture.rs:253` — `request_hud_texture_capture` (53 lines)
- [x] `src/input.rs:512` — `handle_text_editor_event` (53 lines)
- [x] `src/terminals/presentation.rs:351` — `sync_active_terminal_dimensions` (46 lines)
- [x] `src/terminals/presentation.rs:836` — `sync_terminal_hud_surface` (51 lines)
- [x] `src/hud/render.rs:551` — `draw_startup_connect_overlay` (51 lines)
- [x] `src/terminals/daemon/server.rs:252` — `handle_connection` (50 lines)
- [x] `src/hud/bloom.rs:467` — `build_bloom_specs` (50 lines)
- [x] `src/input.rs:733` — `keyboard_input_to_terminal_command` (48 lines)

(Full list: 323 functions total; only the ≥50-line ones listed above.)

---

## 3. Modules: `pub(crate) mod` used where private `mod` should suffice

> Avoid `pub(crate) mod`; use only when path traversal is intended.

- [x] `src/terminals/mod.rs:3` — `pub(crate) mod box_drawing`

---

## 4. Modules: test-only exports interleaved with production exports

> Test-only exports live in one `#[cfg(test)]` block, after prod exports.

- [x] `src/terminals/mod.rs` — `#[cfg(test)]` exports at line 40 interleaved with prod exports continuing through line 102
- [x] `src/conversations/mod.rs` — `#[cfg(test)]` at line 234 before final prod export
- [x] `src/ui/mod.rs` — `#[cfg(test)]` at line 3 before prod export at line 5

---

## 5. Modules: dead compatibility facades still present

> Delete temporary compatibility facades when callers are migrated.

- [x] `src/hud/messages.rs` — entire file is `#![allow(dead_code)]` legacy `HudIntent` enum
- [x] `src/hud/message_box.rs` — `#![allow(dead_code)]` file-level; dead state types mixed with live layout helpers
- [x] `src/hud/modules/mod.rs:166` — `handle_pointer_click_legacy` test-only compat shim
- [x] `src/hud/modules/mod.rs:237` — `handle_scroll_legacy` test-only compat shim
- [x] `src/hud/modules/debug_toolbar/buttons.rs:119` — `legacy_debug_toolbar_buttons` test-only compat
- [x] `src/hud/state.rs:95` — `HudModuleModel` enum `#[cfg(test)]` legacy
- [x] `src/terminals/registry.rs:238,270,280,291,302` — multiple `#[allow(dead_code)]` test-compatibility focus helpers

---

## 6. Modules: large barrel/facade re-export roots

> Prefer small curated root exports; avoid barrel/facade roots.

- [x] `src/terminals/mod.rs` — 24 re-export lines, approaching barrel facade territory
- [x] `src/hud/mod.rs` — 12 re-export lines with mixed prod/test blocks

---

## 7. Modules: mixed root responsibilities (namespace + impl)

> Root = one role: namespace, curated API, or impl; not mixed.

- [x] `src/agents/mod.rs` — 14 functions (full domain impl + namespace)
- [x] `src/conversations/mod.rs` — 13 functions (full domain impl + namespace)
- [x] `src/ui/composer/mod.rs` — 57 functions (full editor impl + namespace)
- [x] `src/hud/modules/mod.rs` — 7 functions (routing impl + namespace)
- [x] `src/hud/modules/agent_list/mod.rs` — 4 functions (impl + namespace)

---

## 8. Modules: test-only visibility widening

> Do not widen prod visibility just to satisfy tests.

51 `#[cfg(test)] pub(crate)` items exist. Most notable non-trivial widenings:

- [x] `src/terminals/presentation.rs` — 6 functions made `pub(crate)` under `#[cfg(test)]` (`active_terminal_cell_size`, `active_terminal_dimensions`, `active_terminal_layout`, `pixel_perfect_cell_size`, `pixel_perfect_terminal_logical_size`, `snap_to_pixel_grid`)
- [x] `src/startup.rs:109` — `with_receiver_for_test` widens `StartupConnectState`
- [x] `src/hud/bloom.rs:1193,1202` — `agent_list_bloom_layer`, `agent_list_bloom_z` test helpers
- [x] `src/hud/persistence.rs:332` — `apply_persisted_layout` widened for tests
- [x] `src/terminals/notes.rs` — `has_note_text`, `append_task_from_text`, `prepend_task_from_text`
- [x] `src/agents/mod.rs:208` — `lifecycle` accessor only for tests

---

## 9. Tests: large inline test blocks in impl files

> Prefer separate sibling test submodules: `foo.rs` + `foo/tests.rs`.
> Move large inline test blocks out of impl files.

- [x] `src/hud/state.rs` — 576 lines of `#[cfg(test)]` block (test compat aggregate)
- [x] `src/terminals/presentation.rs` — 667 lines of `#[cfg(test)]` block
- [x] `src/terminals/fonts.rs` — 350 lines of `#[cfg(test)]` block
- [x] `src/hud/persistence.rs` — 352 lines of `#[cfg(test)]` block (already has `persistence/tests.rs` too)
- [x] `src/terminals/daemon/server.rs` — 349 lines of `#[cfg(test)]` block
- [x] `src/hud/modules/mod.rs` — 266 lines of `#[cfg(test)]` block (legacy shims)
- [x] `src/terminals/pty_spawn.rs` — 123 lines of `#[cfg(test)]` block
- [x] `src/main.rs` — 71 lines of `#[cfg(test)]` block

---

## 10. Tests: parser/serializer/helper tests in giant central buckets

> Parser/serializer/helper tests should not live in giant central buckets.

### Config parser tests in `src/tests/scene.rs` (should be near `src/app_config.rs`)
- [x] `parses_neozeus_toml_config` (line 153)
- [x] `neozeus_config_path_resolution_prefers_explicit_then_xdg_then_home_then_cwd` (line 183)
- [x] `primary_window_config_can_use_loaded_toml_overrides` (line 232)
- [x] `parses_output_mode_and_dimensions` (line 271)
- [x] `offscreen_synthetic_window_config_is_hidden_and_windowed` (line 292)
- [x] `parses_optional_window_scale_factor_override` (line 324)
- [x] `parses_force_fallback_adapter_override` (line 337)
- [x] `resolves_disable_pipelined_rendering_for_wayland_desktop_only` (line 360)
- [x] `resolves_linux_window_backend_policy` (line 400)

### Font/kitty config tests in `src/tests/terminals.rs` (should be near `src/terminals/fonts.rs`)
- [x] `measured_cell_metrics_grow_with_font_size` (line 125)
- [x] `larger_measured_cells_reduce_terminal_grid_in_same_viewport` (line 134)
- [x] `parses_font_family_from_included_kitty_config` (line 1489)
- [x] `kitty_config_lookup_prefers_explicit_directory_over_other_locations` (line 1511)
- [x] `configured_terminal_font_path_resolves_exact_primary_face` (line 1542)
- [x] `dump_terminal_font_reference_sample` (line 1561)
- [x] `resolves_effective_terminal_font_stack_on_host` (line 1676)
- [x] `detects_special_font_ranges` (line 1687)

### Daemon protocol/server tests in `src/tests/terminals.rs` (should be near `src/terminals/daemon/`)
- [x] `daemon_socket_path_prefers_override_then_xdg_runtime_then_tmp_user` (line 3063)
- [x] `daemon_protocol_roundtrip_preserves_terminal_messages` (line 3097)
- [x] `daemon_server_cleans_up_stale_socket_file` (line 3131)
- [x] `daemon_create_attach_command_output_and_kill_roundtrip` (line 3145)
- [x] `daemon_sessions_survive_client_reconnect` (line 3185)
- [x] `daemon_exited_sessions_remain_listed_until_explicit_kill` (line 3225)
- [x] `daemon_session_listing_preserves_creation_order_not_lexical_order` (line 3271)
- [x] `daemon_runtime_bridge_pushes_initial_snapshot_and_forwards_commands` (line 3320)
- [x] `daemon_resize_session_request_succeeds` (line 3346)
- [x] `daemon_runtime_bridge_applies_streamed_updates` (line 3361)
- [x] `daemon_attach_missing_session_returns_error` (line 3391)
- [x] `daemon_kill_missing_session_returns_error` (line 3403)
- [x] `daemon_multiple_clients_receive_updates_for_same_session` (line 3416)
- [x] `daemon_protocol_rejects_truncated_frame` (line 3476)
- [x] `daemon_protocol_rejects_trailing_bytes_in_frame` (line 3486)
- [x] `daemon_resize_session_updates_attached_surface_dimensions` (line 3507)
- [x] `daemon_duplicate_attach_in_same_client_is_rejected` (line 3526)
- [x] `daemon_killing_one_session_preserves_other_sessions` (line 3544)
- [x] `daemon_session_lifecycle_churn_stays_consistent` (line 3567)

---

## 11. Modules: over-exposed `pub(crate)` items (only used within own module)

> `pub(crate)` > `pub`; expose only true subsystem API.

94 items are `pub(crate)` but never referenced outside their own module tree. Should be private or `pub(super)`.

### `src/agents/mod.rs`
- [x] `AgentRecord` (line 41)
- [x] `AgentRuntimeLifecycle` (line 107)
- [x] `AgentRuntimeLifecycle::from_runtime` (line 117)
- [x] `AgentRuntimeLink` (line 128)

### `src/app/bootstrap.rs`
- [x] `primary_window_plugin_config_for_with_config` (line 263)

### `src/app/dispatch.rs`
- [x] `AppCommandContext` (line 109)

### `src/app/output.rs`
- [x] `create_final_frame_image` (line 137)
- [x] `FinalFrameReadbackMeta` (line 193)
- [x] `final_frame_format` (line 459)

### `src/app/session.rs`
- [x] `HudWidgetPlacement` (line 13)

### `src/app_config.rs`
- [x] `NeoZeusTerminalConfig` (line 26)
- [x] `NeoZeusWindowConfig` (line 33)
- [x] `resolve_neozeus_config_path` (line 67)

### `src/conversations/mod.rs`
- [x] `MessageId` (line 14)
- [x] `MessageRecord` (line 29)
- [x] `ConversationRecord` (line 38)

### `src/conversations/persistence.rs`
- [x] `PersistedConversationMessage` (line 13)
- [x] `PersistedConversationRecord` (line 19)
- [x] `PersistedConversations` (line 25)

### `src/hud/bloom.rs`
- [x] `resolve_agent_list_bloom_intensity` (line 82)
- [x] `resolve_agent_list_bloom_debug_previews` (line 93)
- [x] `AgentListBloomCameraMarker` (line 105)
- [x] `AgentListBloomCompositeMarker` (line 108)
- [x] `AgentListBloomBlurUniform` (line 144)
- [x] `AgentListBloomSourceKind` (line 175)
- [x] `AgentListBloomSourceSegment` (line 181)
- [x] `AgentListBloomSourceSprite` (line 189)
- [x] `HudWidgetBloomSetupContext` (line 519)
- [x] `HudWidgetBloomContext` (line 857)
- [x] `agent_list_bloom_layer` (line 1193)
- [x] `agent_list_bloom_z` (line 1202)

### `src/hud/input.rs`
- [x] `HudPointerContext` (line 48)

### `src/hud/modules/agent_list/mod.rs`
- [x] `AGENT_LIST_HEADER_HEIGHT` (line 10)
- [x] `AGENT_LIST_LEFT_RAIL_WIDTH` (line 11)
- [x] `AGENT_LIST_ROW_MARKER_WIDTH` (line 12)
- [x] `AGENT_LIST_ROW_MARKER_GAP` (line 13)
- [x] `AGENT_LIST_ROW_GAP` (line 14)
- [x] `AgentRow` (line 33)
- [x] `agent_row_stride` (line 76)
- [x] `agent_list_content_height` (line 84)

### `src/hud/modules/conversation_list.rs`
- [x] `ConversationRow` (line 15)

### `src/hud/modules/debug_toolbar/mod.rs`
- [x] `DebugToolbarAction` (line 14)
- [x] `DebugToolbarButton` (line 24)

### `src/hud/persistence.rs`
- [x] `PersistedHudModuleState` (line 15)
- [x] `PersistedHudState` (line 21)
- [x] `resolve_hud_layout_path_with` (line 34)
- [x] `parse_persisted_hud_state` (line 186)
- [x] `serialize_persisted_hud_state` (line 205)
- [x] `apply_persisted_layout` (line 332)

### `src/hud/render.rs`
- [x] `HudColors::TITLE` (line 40)
- [x] `HudColors::MESSAGE_BOX` (line 50)
- [x] `hud_rect_to_scene` (line 76)
- [x] `HudPainter::text_size` (line 147)

### `src/hud/state.rs`
- [x] `HUD_ANIMATION_EPSILON` (line 18)
- [x] `HudModuleShell` (line 41)
- [x] `HudModuleInstance` (line 94)
- [x] `HudLayoutState::iter_z_order_front_to_back` (line 137)
- [x] `HudModalState::close_message_box_and_discard_draft` (line 348)
- [x] `HudState::input_capture_state` (line 423) [#[cfg(test)]]

### `src/hud/view_models.rs`
- [x] `ThreadMessageView` (line 43)

### `src/hud/widgets.rs`
- [x] `widget_definition` (line 97)

### `src/startup.rs`
- [x] `SceneSetupContext` (line 154)

### `src/terminals/daemon/client.rs`
- [x] `connect_or_start_default` (line 93)

### `src/terminals/daemon/session.rs`
- [x] `AttachedSubscriber` (line 40)

### `src/terminals/fonts.rs`
- [x] `initialize_terminal_text_renderer` (line 217)
- [x] `resolve_terminal_font_report` (line 263)
- [x] `find_kitty_config_path` (line 391)

### `src/terminals/lifecycle.rs`
- [x] `remove_terminal_with_projection` (line 39)

### `src/terminals/mailbox.rs`
- [x] `MailboxPush` (line 15)
- [x] `push_frame` (line 30)
- [x] `push_status` (line 47)

### `src/terminals/notes.rs`
- [x] `append_task_from_text` (line 75)
- [x] `prepend_task_from_text` (line 100)
- [x] `resolve_terminal_notes_path_with` (line 124)
- [x] `parse_terminal_notes` (line 190)
- [x] `serialize_terminal_notes` (line 238)

### `src/terminals/presentation.rs`
- [x] `HUD_FRAME_PADDING` (line 13)
- [x] `ACTIVE_TERMINAL_MARGIN` (line 14)
- [x] `DIRECT_INPUT_FRAME_OUTSET` (line 15)
- [x] `ActiveTerminalLayout` (line 23)
- [x] `spawn_terminal_presentation` (line 46)

### `src/terminals/registry.rs`
- [x] `create_terminal_without_focus_with_session` (line 144)
- [x] `create_terminal_with_slot_and_session` (line 241)

### `src/terminals/runtime.rs`
- [x] `TerminalRuntimeSpawner::noop` (line 33)
- [x] `spawn_daemon_terminal_runtime` (line 201)

### `src/terminals/session_persistence.rs`
- [x] `ReconciledTerminalSessions` (line 33)
- [x] `resolve_terminal_sessions_path_with` (line 57)
- [x] `parse_persisted_terminal_sessions` (line 273)
- [x] `build_persisted_terminal_sessions` (line 354)

### `src/ui/composer/layout.rs`
- [x] `MessageBoxActionButton` (line 22)
- [x] `TaskDialogActionButton` (line 29)

### `src/ui/composer/mod.rs`
- [x] `TextEditorYankState` (line 14)

### `src/verification.rs`
- [x] `resolve_verification_scenario` (line 51)
- [x] `VerificationScenarioContext` (line 190)

---

## 12. Modules: singleton `mod.rs` root without subtree clarity

> Flatten singleton `mod.rs` roots when no subtree clarity is gained.

- [x] `src/ui/mod.rs` — wraps only `composer/` submodule; `ui` namespace adds no clarity over `crate::composer`

---

## 13. Modules: leaf imports over root re-export churn

(Previously section 11)

> Prefer leaf imports over root re-export churn.

`terminals/mod.rs` re-exports 126 items. `hud/mod.rs` re-exports ~50 items. Internal sibling modules import through the root instead of from the leaf submodule directly.

### Internal `terminals/` callers using root re-exports (17 import lines)
- [x] `src/terminals/ansi_surface.rs` — imports through `crate::terminals::` not leaf
- [x] `src/terminals/damage/tests.rs` — imports through `crate::terminals::`
- [x] `src/terminals/damage.rs` — imports through `crate::terminals::`
- [x] `src/terminals/presentation_state.rs` — imports through `crate::terminals::`
- [x] `src/terminals/bridge.rs` — imports through `crate::terminals::`
- [x] `src/terminals/mailbox.rs` — imports through `crate::terminals::`
- [x] `src/terminals/runtime.rs` — imports through `crate::terminals::`
- [x] `src/terminals/backend.rs` — imports through `crate::terminals::`
- [x] `src/terminals/notes.rs` — imports through `crate::terminals::`
- [x] `src/terminals/lifecycle.rs` — imports through `crate::terminals::`
- [x] `src/terminals/pty_spawn.rs` — imports through `crate::terminals::`
- [x] `src/terminals/registry.rs` — imports through `crate::terminals::`
- [x] `src/terminals/daemon/protocol.rs` — imports through `crate::terminals::`
- [x] `src/terminals/daemon/server.rs` — imports through `crate::terminals::`
- [x] `src/terminals/daemon/client.rs` — imports through `crate::terminals::`
- [x] `src/terminals/daemon/protocol/tests.rs` — imports through `crate::terminals::`

### Internal `hud/` callers using root re-exports (12 import lines)
- [x] `src/hud/capture.rs` — imports through `crate::hud::`
- [x] `src/hud/widgets.rs` — imports through `crate::hud::`
- [x] `src/hud/animation.rs` — imports through `crate::hud::`
- [x] `src/hud/persistence.rs` — imports through `crate::hud::` (3 import lines)
- [x] `src/hud/persistence/tests.rs` — imports through `crate::hud::`
- [x] `src/hud/modules/thread_pane.rs` — imports through `crate::hud::`
- [x] `src/hud/modules/agent_list/render.rs` — imports through `crate::hud::`
- [x] `src/hud/modules/debug_toolbar/buttons.rs` — imports through `crate::hud::`
- [x] `src/hud/modules/debug_toolbar/mod.rs` — imports through `crate::hud::`
- [x] `src/hud/modules/debug_toolbar/render.rs` — imports through `crate::hud::`

---

## 14. Tests: module-local behavior tests in central test files

> Module-local behavior tests belong with the module.

These test a single module's behavior but live in `src/tests/` instead of near the owning module.

### Agent-list tests in `src/tests/hud.rs` (should be near `hud/modules/agent_list/`)
- [x] `sync_structural_hud_layout_docks_agent_list_to_full_height_left_column` (line 148)
- [x] `agent_row_rect_splits_main_and_marker_geometry` (line 180)
- [x] `agent_rows_use_derived_agent_view_labels` (line 740)
- [x] `agent_rows_follow_terminal_order_and_focus` (line 781)
- [x] `agent_rows_mark_hovered_agent` (line 832)
- [x] `agent_list_is_not_draggable` (line 887)
- [x] `clicking_agent_list_row_emits_focus_and_isolate_commands` (line 1327)
- [x] `agent_list_scroll_clamps_to_content_height` (line 1458)

### Conversation-list tests in `src/tests/hud.rs` (should be near `hud/modules/conversation_list.rs`)
- [x] `clicking_conversation_list_row_emits_focus_and_isolate_commands` (line 1408)

### Animation tests in `src/tests/hud.rs` (should be near `hud/animation.rs`)
- [x] `animate_hud_modules_moves_current_rect_and_alpha_toward_target` (line 1176)
- [x] `hud_needs_redraw_when_drag_or_animation_is_active` (line 1601)

### Setup tests in `src/tests/hud.rs` (should be near `hud/setup.rs`)
- [x] `setup_hud_requests_initial_redraw` (line 98)

### Compositor tests in `src/tests/hud.rs` (should be near `hud/compositor.rs`)
- [x] `sync_hud_offscreen_compositor_hides_vello_canvas_and_binds_texture` (line 204)
- [x] `sync_hud_offscreen_compositor_leaves_modal_vello_canvas_visible` (line 298)
- [x] `hud_composite_quad_matches_upstream_vello_canvas_contract` (line 361)
- [x] `upstream_vello_present_contract_preserves_target_orange_bytes` (line 423)

### Raster tests in `src/tests/terminals.rs` (should be near `terminals/raster.rs`)
- [x] `alpha_blend_preserves_transparent_glyph_background` (line 462)
- [x] `standalone_text_renderer_rasterizes_ascii_glyph` (line 1696)
- [x] `sync_terminal_texture_renders_visible_text_on_last_row` (line 1818)
- [x] `sync_terminal_texture_updates_pixels_when_last_row_text_changes` (line 1852)
- [x] `sync_terminal_texture_keeps_cached_switch_frame_until_resized_surface_arrives` (line 1146)
- [x] `sync_terminal_texture_promotes_active_terminal_once_resized_surface_arrives` (line 1264)

### Presentation tests in `src/tests/terminals.rs` (should be near `terminals/presentation.rs`)
- [x] `snap_to_pixel_grid_respects_window_scale_factor` (line 494)
- [x] `active_terminal_target_position_accounts_for_texture_parity` (line 502)
- [x] `active_terminal_viewport_reserves_agent_list_column` (line 558)
- [x] `active_terminal_resize_requests_follow_viewport_grid_policy` (line 1359)
- [x] `show_all_presentations_remain_visible_when_no_terminal_is_active` (line 2015)
- [x] `terminal_panel_frames_are_hidden_without_direct_input_mode` (line 2090)
- [x] `direct_input_mode_shows_orange_terminal_frame` (line 2114)
- [x] `disconnected_terminal_shows_red_status_frame` (line 2179)
- [x] `startup_loading_shows_active_placeholder_before_first_surface_arrives` (line 2245)
- [x] `startup_loading_temporarily_overrides_isolate_to_show_all_pending_terminals` (line 2316)
- [x] `isolate_visibility_policy_with_missing_terminal_degrades_to_show_all` (line 2771)

### Registry/manager tests in `src/tests/terminals.rs` (should be near `terminals/registry.rs`)
- [x] `drain_terminal_updates_keeps_latest_frame_and_status` (line 1410)
- [x] `poll_terminal_snapshots_keeps_latest_status_over_latest_frame_runtime` (line 1449)
- [x] `terminal_creation_order_stays_stable_when_focus_changes` (line 1955)
- [x] `terminal_can_be_created_without_becoming_active` (line 1970)
- [x] `terminal_with_session_name_is_retained_in_manager_state` (line 1983)

### Backend tests in `src/tests/terminals.rs` (should be near `terminals/backend.rs`)
- [x] `send_command_payload_bytes_turn_multiline_text_into_enter_sequences` (line 1918)

### Debug toolbar tests in `src/tests/hud.rs` (should be near `debug_toolbar/`)
- [x] `clicking_debug_toolbar_button_emits_spawn_terminal_command` (line 1201)
- [x] `clicking_debug_toolbar_command_button_emits_terminal_command` (line 1267)
- [x] `debug_toolbar_buttons_include_module_toggle_entries` (line 1524)
- [x] `debug_toolbar_module_toggle_buttons_reflect_enabled_state` (line 1558)

---

## Summary counts

| # | Category | Count |
|---|---|---:|
| 1 | Missing rustdoc | 208 |
| 2 | Missing inline comments (≥20-line fns) | 323 |
| 3 | `pub(crate) mod` violation | 1 |
| 4 | Test export ordering violations | 3 |
| 5 | Dead compatibility facades | 7 |
| 6 | Barrel/facade re-export roots | 2 |
| 7 | Mixed-responsibility module roots | 5 |
| 8 | Test-only visibility widening | ~51 |
| 9 | Large inline test blocks to extract | 8 |
| 10 | Parser/helper tests in central buckets | 36 |
| 11 | Over-exposed `pub(crate)` items | 94 |
| 12 | Singleton mod.rs root | 1 |
| 13 | Leaf import violations (root re-export churn) | 27 |
| 14 | Module-local tests in central test files | 57 |
| **Total** | | **~823** |
