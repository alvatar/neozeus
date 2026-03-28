# Style guide compliance TODO

Execution checklist for implementing the full `STYLE_GUIDE.md` compliance sweep.

## Phase tracker

### Phase 0 — prune stale audit items and normalize the checklist
- [ ] Reconcile this audit with current HEAD so already-fixed issues are marked done or removed.
- [x] Split the work into execution phases that can be verified independently.
- [x] Keep this file updated as each task/phase is completed.

### Phase 1 — finish remaining compatibility/dead-facade cleanup
- [x] Remove legacy HUD compatibility facades already migrated away (`hud/messages.rs`, `hud/message_box.rs`, legacy HUD shims).
- [x] Remove remaining dead compatibility helpers in `src/hud/modules/debug_toolbar/buttons.rs`.
- [x] Remove remaining test-compatibility helpers in `src/terminals/registry.rs` if no longer needed.
- [x] Phase 1 complete.

### Phase 2 — rustdoc coverage sweep
- [ ] Add rustdoc to all production functions missing canonical `///` docs.
- [ ] Add rustdoc to test helpers/tests that are intentionally documented in this codebase.
- [ ] Re-run the audit and ensure no missing-rustdoc items remain from the checklist.

### Phase 3 — inline comment sweep for complex functions
- [ ] Add explanatory inline comments to all high-complexity functions called out by the audit.
- [ ] Re-run the audit and ensure no zero-inline-comment ≥20-line functions remain where comments are warranted by the style guide.

### Phase 4 — module boundary cleanup
- [ ] Fix `pub(crate) mod` / export-order / singleton-root violations.
- [ ] Reduce root barrel re-exports and convert internal callers to leaf imports.
- [ ] Reduce over-exposed `pub(crate)` visibility to private or `pub(super)` where possible.
- [ ] Resolve mixed-responsibility roots where flattening/splitting improves compliance without widening scope unsafely.

### Phase 5 — test layout cleanup
- [ ] Move large inline test blocks into sibling `tests.rs` modules where required.
- [ ] Move parser/helper/module-local tests out of giant central buckets and next to their owning modules.
- [ ] Keep cross-module/system tests centralized only where appropriate.

### Phase 6 — final audit and verification
- [ ] Reconcile the detailed checklist below against the final code state.
- [ ] Mark all completed tasks in this document.
- [ ] Run `cargo fmt --check`.
- [ ] Run `cargo clippy --all-targets -- -D warnings`.
- [ ] Run the full test suite.

Every item below is a specific violation from the original audit, grouped by rule. Items may be removed as stale during Phase 0 or checked off as work lands.

---

## 1. Documentation: missing rustdoc on functions

> All functions should have canonical Rust doc, explaining in detail what the function does and how it should be used.

208 functions have no `///` doc comment. Full list by file:

### `src/agents/mod.rs` (14)
- [ ] `AgentCatalog::create_agent` (line 55)
- [ ] `AgentCatalog::remove` (line 76)
- [ ] `AgentCatalog::label` (line 82)
- [ ] `AgentCatalog::label_for_terminal` (line 88)
- [ ] `AgentCatalog::iter` (line 98)
- [ ] `AgentRuntimeLifecycle::from_runtime` (line 117)
- [ ] `AgentRuntimeIndex::link_terminal` (line 142)
- [ ] `AgentRuntimeIndex::update_runtime` (line 164)
- [ ] `AgentRuntimeIndex::remove_terminal` (line 177)
- [ ] `AgentRuntimeIndex::agent_for_terminal` (line 187)
- [ ] `AgentRuntimeIndex::agent_for_session` (line 191)
- [ ] `AgentRuntimeIndex::primary_terminal` (line 195)
- [ ] `AgentRuntimeIndex::session_name` (line 201)
- [ ] `AgentRuntimeIndex::lifecycle` (line 208)

### `src/agents/tests.rs` (3)
- [ ] `catalog_assigns_stable_default_labels_in_creation_order` (line 7)
- [ ] `runtime_index_links_terminal_session_and_runtime_state` (line 27)
- [ ] `runtime_index_remove_terminal_clears_reverse_indexes` (line 58)

### `src/app/bootstrap.rs` (4)
- [ ] `resolve_linux_window_backend` (line 173)
- [ ] `should_force_x11_backend` (line 181)
- [ ] `apply_linux_window_backend_policy` (line 210)
- [ ] `normalize_output_for_x11_fallback` (line 228)

### `src/app/commands/tests.rs` (1)
- [ ] `app_command_can_wrap_composer_request` (line 5)

### `src/app/dispatch.rs` (1)
- [ ] `refresh_open_task_editor` (line 93)

### `src/app/session/tests.rs` (1)
- [ ] `session_focus_and_visibility_update_independently` (line 5)

### `src/app/use_cases/composer.rs` (3)
- [ ] `submit_composer` (line 14)
- [ ] `cancel_composer` (line 51)
- [ ] `open_composer` (line 59)

### `src/app/use_cases/conversation.rs` (1)
- [ ] `send_message` (line 15)

### `src/app/use_cases/focus_agent.rs` (1)
- [ ] `focus_agent` (line 16)

### `src/app/use_cases/kill_active_agent.rs` (2)
- [ ] `adjacent_agent_in_catalog` (line 14)
- [ ] `kill_active_agent` (line 30)

### `src/app/use_cases/restore_app.rs` (2)
- [ ] `startup_focus_candidate_is_interactive` (line 18)
- [ ] `restore_app` (line 26)

### `src/app/use_cases/spawn_agent_terminal.rs` (2)
- [ ] `spawn_agent_terminal` (line 18)
- [ ] `attach_restored_terminal` (line 80)

### `src/app/use_cases/tasks.rs` (5)
- [ ] `set_task_text` (line 5)
- [ ] `append_task` (line 9)
- [ ] `prepend_task` (line 13)
- [ ] `clear_done_tasks` (line 17)
- [ ] `consume_next_task` (line 21)

### `src/app/use_cases/terminals.rs` (3)
- [ ] `send_terminal_command` (line 6)
- [ ] `toggle_active_display_mode` (line 20)
- [ ] `reset_active_view` (line 29)

### `src/app/use_cases/widgets.rs` (2)
- [ ] `toggle_widget` (line 3)
- [ ] `reset_widget` (line 10)

### `src/app/use_cases/widget.rs` (1)
- [ ] `reset_widget` (line 3)

### `src/conversations/mod.rs` (13)
- [ ] `ConversationStore::conversation_for_agent` (line 54)
- [ ] `ConversationStore::ensure_conversation` (line 58)
- [ ] `ConversationStore::push_message` (line 76)
- [ ] `ConversationStore::set_delivery` (line 101)
- [ ] `ConversationStore::messages_for` (line 116)
- [ ] `AgentTaskStore::text` (line 132)
- [ ] `AgentTaskStore::remove_agent` (line 136)
- [ ] `AgentTaskStore::set_text` (line 140)
- [ ] `AgentTaskStore::append_task` (line 159)
- [ ] `AgentTaskStore::prepend_task` (line 176)
- [ ] `AgentTaskStore::clear_done` (line 193)
- [ ] `AgentTaskStore::consume_next` (line 201)
- [ ] `AgentTaskStore::sync_task_notes_projection` (line 215)

### `src/conversations/persistence.rs` (13)
- [ ] `resolve_conversations_path_with` (line 35)
- [ ] `resolve_conversations_path` (line 63)
- [ ] `quote` (line 71)
- [ ] `unquote` (line 86)
- [ ] `delivery_code` (line 110)
- [ ] `parse_delivery` (line 118)
- [ ] `serialize_persisted_conversations` (line 127)
- [ ] `parse_persisted_conversations` (line 153)
- [ ] `load_persisted_conversations_from` (line 227)
- [ ] `build_persisted_conversations` (line 243)
- [ ] `restore_persisted_conversations` (line 273)
- [ ] `mark_conversations_dirty` (line 295)
- [ ] `save_conversations_if_dirty` (line 304)

### `src/conversations/tests.rs` (6)
- [ ] `ensure_conversation_is_stable_per_agent` (line 9)
- [ ] `push_message_appends_to_conversation_history` (line 17)
- [ ] `task_store_clear_done_and_consume_next_update_text` (line 31)
- [ ] `conversation_persistence_roundtrips_messages_by_session_name` (line 46)
- [ ] `restore_persisted_conversations_reattaches_to_restored_agents` (line 76)
- [ ] `conversations_path_prefers_state_home_then_home_state_then_config` (line 113)

### `src/hud/modules/agent_list/render.rs` (5)
- [ ] `draw_label` (line 54)
- [ ] `draw_button_rect` (line 67)
- [ ] `marker_fill` (line 77)
- [ ] `draw_left_rail` (line 87)
- [ ] `render_content` (line 125)

### `src/hud/modules/conversation_list.rs` (7)
- [ ] `row_stride` (line 24)
- [ ] `rows` (line 28)
- [ ] `handle_pointer_click` (line 54)
- [ ] `handle_hover` (line 76)
- [ ] `clear_hover` (line 100)
- [ ] `handle_scroll` (line 108)
- [ ] `render_content` (line 123)

### `src/hud/modules/debug_toolbar/buttons.rs` (1)
- [ ] `legacy_debug_toolbar_buttons` (line 119)

### `src/hud/modules/mod.rs` (7)
- [ ] `handle_pointer_click` (line 33)
- [ ] `handle_hover` (line 72)
- [ ] `clear_hover` (line 96)
- [ ] `render_module_content` (line 109)
- [ ] `handle_scroll` (line 133)
- [ ] `handle_pointer_click_legacy` (line 166)
- [ ] `handle_scroll_legacy` (line 237)

### `src/hud/modules/thread_pane.rs` (1)
- [ ] `render_content` (line 8)

### `src/hud/render.rs` (2)
- [ ] `draw_startup_connect_overlay` (line 558)
- [ ] `draw_task_dialog` (line 610)

### `src/hud/view_models.rs` (2)
- [ ] `DebugToolbarView::zoom_distance` (line 75)
- [ ] `sync_hud_view_models` (line 84)

### `src/hud/view_models/tests.rs` (1)
- [ ] `sync_hud_view_models_derives_agent_rows_and_threads` (line 11)

### `src/hud/widgets.rs` (5)
- [ ] `HudWidgetKey::number` (line 13)
- [ ] `HudWidgetKey::title` (line 19)
- [ ] `HudWidgetKey::title_key` (line 25)
- [ ] `widget_definition` (line 97)
- [ ] `widget_registry_includes_conversation_and_thread_widgets` (line 108)

### `src/startup.rs` (8)
- [ ] `StartupConnectState::default` (line 96)
- [ ] `StartupConnectState::with_receiver_for_test` (line 109)
- [ ] `StartupConnectState::phase` (line 123)
- [ ] `StartupConnectState::title` (line 127)
- [ ] `StartupConnectState::status` (line 136)
- [ ] `StartupConnectState::modal_visible` (line 140)
- [ ] `StartupConnectState::start_background_connect` (line 144)
- [ ] `advance_startup_connecting` (line 328)

### `src/terminals/box_drawing.rs` (6)
- [ ] `is_box_drawing` (line 3)
- [ ] `rasterize_box_drawing` (line 10)
- [ ] `draw_h` (line 75)
- [ ] `draw_v` (line 83)
- [ ] `stroke_width` (line 91)
- [ ] `set_white` (line 100)

### `src/terminals/fonts.rs` (1)
- [ ] `TerminalFontState::default` (line 50)

### `src/terminals/presentation.rs` (7)
- [ ] `fixed_terminal_cell_size` (line 199)
- [ ] `target_active_terminal_dimensions` (line 206)
- [ ] `active_terminal_cell_size` (line 221)
- [ ] `active_terminal_dimensions` (line 229)
- [ ] `active_terminal_layout` (line 239)
- [ ] `active_terminal_layout_for_dimensions` (line 254)
- [ ] `snap_axis_for_texture_center` (line 460)

### `src/terminals/raster.rs` (1)
- [ ] `try_rasterize_box_drawing` (line 410)

### `src/terminals/runtime.rs` (1)
- [ ] `TerminalRuntimeSpawner::daemon_client` (line 104)

### `src/tests/hud.rs` (1)
- [ ] `run_app_commands` (line 163)

### `src/tests/input.rs` (2)
- [ ] `ensure_app_command_world_resources` (line 184)
- [ ] `run_app_command_cycle` (line 234)

### `src/tests/scene.rs` (3)
- [ ] `resolves_linux_window_backend_policy` (line 400)
- [ ] `pending_runtime_spawner_becomes_ready_when_daemon_is_installed` (line 80)
- [ ] `startup_connecting_advances_to_restoring_when_background_connect_completes` (line 88)

### `src/tests/terminals.rs` (10)
- [ ] `measured_font_state_for_size` (line 105)
- [ ] `measured_cell_metrics_grow_with_font_size` (line 125)
- [ ] `larger_measured_cells_reduce_terminal_grid_in_same_viewport` (line 134)
- [ ] `render_surface_to_terminal_image_with_presentation_state` (line 252)
- [ ] `read_binary_ppm` (line 374)
- [ ] `crop_rgb_rows` (line 407)
- [ ] `crop_image_rgb` (line 418)
- [ ] `surface_from_pi_screen_reference_ansi` (line 433)
- [ ] `active_terminal_target_position_accounts_for_texture_parity` (line 502)
- [ ] `glyph_rasterization_snaps_fractional_baseline_to_same_pixels` (line 1721)

### `src/ui/composer/mod.rs` (57)
- [ ] `ComposerMode::agent_id` (line 41)
- [ ] `ComposerState::keyboard_capture_active` (line 62)
- [ ] `ComposerState::unbind_agent` (line 71)
- [ ] `ComposerState::open_message` (line 82)
- [ ] `ComposerState::open_task_editor` (line 96)
- [ ] `ComposerState::cancel_preserving_draft` (line 106)
- [ ] `ComposerState::discard_current_message` (line 118)
- [ ] `ComposerState::close_task_editor` (line 131)
- [ ] `ComposerState::current_agent` (line 141)
- [ ] `ComposerState::current_message_agent` (line 145)
- [ ] `ComposerState::save_open_message_draft` (line 152)
- [ ] `ComposerState::save_message_draft` (line 158)
- [ ] `TextEditorState::close` (line 165)
- [ ] `TextEditorState::close_and_discard` (line 176)
- [ ] `TextEditorState::load_text` (line 185)
- [ ] `TextEditorState::clear_editor` (line 191)
- [ ] `TextEditorState::snapshot_draft` (line 199)
- [ ] `TextEditorState::restore_draft` (line 208)
- [ ] `TextEditorState::region_bounds` (line 217)
- [ ] `TextEditorState::set_mark` (line 222)
- [ ] `TextEditorState::insert_text` (line 229)
- [ ] `TextEditorState::insert_newline` (line 239)
- [ ] `TextEditorState::newline_and_indent` (line 243)
- [ ] `TextEditorState::open_line` (line 247)
- [ ] `TextEditorState::move_left` (line 256)
- [ ] `TextEditorState::move_right` (line 266)
- [ ] `TextEditorState::move_line_start` (line 276)
- [ ] `TextEditorState::move_line_end` (line 287)
- [ ] `TextEditorState::move_up` (line 298)
- [ ] `TextEditorState::move_down` (line 322)
- [ ] `TextEditorState::move_word_backward` (line 341)
- [ ] `TextEditorState::move_word_forward` (line 352)
- [ ] `TextEditorState::delete_backward_char` (line 363)
- [ ] `TextEditorState::delete_forward_char` (line 371)
- [ ] `TextEditorState::copy_region` (line 379)
- [ ] `TextEditorState::kill_region` (line 390)
- [ ] `TextEditorState::kill_to_end_of_line` (line 401)
- [ ] `TextEditorState::kill_word_forward` (line 414)
- [ ] `TextEditorState::kill_word_backward` (line 422)
- [ ] `TextEditorState::yank` (line 430)
- [ ] `TextEditorState::yank_pop` (line 449)
- [ ] `TextEditorState::cursor_line_and_column` (line 470)
- [ ] `insert_text_internal` (line 480)
- [ ] `delete_range_internal` (line 506)
- [ ] `push_kill` (line 534)
- [ ] `normalize_text` (line 547)
- [ ] `adjust_index_after_delete` (line 551)
- [ ] `word_backward_boundary` (line 561)
- [ ] `word_forward_boundary` (line 578)
- [ ] `previous_char_boundary` (line 595)
- [ ] `next_char_boundary` (line 602)
- [ ] `previous_char` (line 612)
- [ ] `next_char` (line 619)
- [ ] `current_line_bounds` (line 629)
- [ ] `current_line_column_chars` (line 641)
- [ ] `advance_by_chars` (line 646)
- [ ] `is_word_char` (line 658)

### `src/ui/composer/tests.rs` (2)
- [ ] `message_composer_preserves_per_agent_drafts` (line 5)
- [ ] `task_editor_reopens_from_supplied_text_not_stale_buffer` (line 26)

---

## 2. Documentation: missing inline comments in complex functions

> Explanatory comments inside functions should be added in all non-obvious pieces and particularly hairy parts.

323 functions of ≥20 lines have zero inline comments. Most critical (≥50 lines):

- [ ] `src/app/dispatch.rs:135` — `apply_app_commands` (213 lines)
- [ ] `src/app/use_cases/restore_app.rs:26` — `restore_app` (160 lines)
- [ ] `src/hud/bloom.rs:534` — `setup_hud_widget_bloom` (252 lines)
- [ ] `src/hud/bloom.rs:945` — `sync_hud_widget_bloom` (243 lines)
- [ ] `src/hud/view_models.rs:84` — `sync_hud_view_models` (149 lines)
- [ ] `src/app/schedule.rs:47` — `configure_app_schedule` (145 lines)
- [ ] `src/hud/input.rs:120` — `handle_hud_pointer_input` (161 lines)
- [ ] `src/hud/render.rs:335` — `draw_text_editor_body` (113 lines)
- [ ] `src/terminals/raster.rs:146` — `sync_terminal_texture` (172 lines)
- [ ] `src/hud/modules/debug_toolbar/buttons.rs:9` — `debug_toolbar_buttons` (108 lines)
- [ ] `src/hud/modules/agent_list/render.rs:125` — `render_content` (102 lines)
- [ ] `src/hud/capture.rs:161` — `request_hud_composite_capture` (86 lines)
- [ ] `src/hud/render.rs:187` — `label_scaled` (75 lines)
- [ ] `src/hud/setup.rs:35` — `setup_hud` (75 lines)
- [ ] `src/hud/input.rs:321` — `handle_hud_module_shortcuts` (71 lines)
- [ ] `src/terminals/raster.rs:339` — `repaint_terminal_pixels` (70 lines)
- [ ] `src/terminals/raster.rs:464` — `rasterize_terminal_glyph` (70 lines)
- [ ] `src/hud/compositor.rs:181` — `sync_hud_offscreen_compositor` (69 lines)
- [ ] `src/terminals/presentation.rs:762` — `sync_terminal_panel_frames` (69 lines)
- [ ] `src/app_config.rs:191` — `parse_neozeus_config` (66 lines)
- [ ] `src/terminals/ansi_surface.rs:18` — `build_surface` (65 lines)
- [ ] `src/terminals/box_drawing.rs:10` — `rasterize_box_drawing` (64 lines)
- [ ] `src/terminals/daemon/client.rs:113` — `connect` (64 lines)
- [ ] `src/terminals/presentation.rs:46` — `spawn_terminal_presentation` (66 lines)
- [ ] `src/hud/render.rs:473` — `draw_message_box` (60 lines)
- [ ] `src/hud/render.rs:603` — `draw_task_dialog` (60 lines)
- [ ] `src/app/dispatch.rs:28` — `sync_agents_from_terminals` (60 lines)
- [ ] `src/app/use_cases/kill_active_agent.rs:29` — `kill_active_agent` (58 lines)
- [ ] `src/app/use_cases/spawn_agent_terminal.rs:18` — `spawn_agent_terminal` (57 lines)
- [ ] `src/startup.rs:267` — `setup_scene` (56 lines)
- [ ] `src/startup.rs:328` — `advance_startup_connecting` (53 lines)
- [ ] `src/terminals/fonts.rs:450` — `parse_kitty_config_file` (53 lines)
- [ ] `src/terminals/fonts.rs:508` — `fc_query_family_for_path` (53 lines)
- [ ] `src/hud/capture.rs:253` — `request_hud_texture_capture` (53 lines)
- [ ] `src/input.rs:512` — `handle_text_editor_event` (53 lines)
- [ ] `src/terminals/presentation.rs:351` — `sync_active_terminal_dimensions` (46 lines)
- [ ] `src/terminals/presentation.rs:836` — `sync_terminal_hud_surface` (51 lines)
- [ ] `src/hud/render.rs:551` — `draw_startup_connect_overlay` (51 lines)
- [ ] `src/terminals/daemon/server.rs:252` — `handle_connection` (50 lines)
- [ ] `src/hud/bloom.rs:467` — `build_bloom_specs` (50 lines)
- [ ] `src/input.rs:733` — `keyboard_input_to_terminal_command` (48 lines)

(Full list: 323 functions total; only the ≥50-line ones listed above.)

---

## 3. Modules: `pub(crate) mod` used where private `mod` should suffice

> Avoid `pub(crate) mod`; use only when path traversal is intended.

- [x] `src/terminals/mod.rs:3` — `pub(crate) mod box_drawing`

---

## 4. Modules: test-only exports interleaved with production exports

> Test-only exports live in one `#[cfg(test)]` block, after prod exports.

- [ ] `src/terminals/mod.rs` — `#[cfg(test)]` exports at line 40 interleaved with prod exports continuing through line 102
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

- [ ] `src/terminals/mod.rs` — 24 re-export lines, approaching barrel facade territory
- [ ] `src/hud/mod.rs` — 12 re-export lines with mixed prod/test blocks

---

## 7. Modules: mixed root responsibilities (namespace + impl)

> Root = one role: namespace, curated API, or impl; not mixed.

- [ ] `src/agents/mod.rs` — 14 functions (full domain impl + namespace)
- [ ] `src/conversations/mod.rs` — 13 functions (full domain impl + namespace)
- [ ] `src/ui/composer/mod.rs` — 57 functions (full editor impl + namespace)
- [ ] `src/hud/modules/mod.rs` — 7 functions (routing impl + namespace)
- [ ] `src/hud/modules/agent_list/mod.rs` — 4 functions (impl + namespace)

---

## 8. Modules: test-only visibility widening

> Do not widen prod visibility just to satisfy tests.

51 `#[cfg(test)] pub(crate)` items exist. Most notable non-trivial widenings:

- [ ] `src/terminals/presentation.rs` — 6 functions made `pub(crate)` under `#[cfg(test)]` (`active_terminal_cell_size`, `active_terminal_dimensions`, `active_terminal_layout`, `pixel_perfect_cell_size`, `pixel_perfect_terminal_logical_size`, `snap_to_pixel_grid`)
- [ ] `src/startup.rs:109` — `with_receiver_for_test` widens `StartupConnectState`
- [ ] `src/hud/bloom.rs:1193,1202` — `agent_list_bloom_layer`, `agent_list_bloom_z` test helpers
- [ ] `src/hud/persistence.rs:332` — `apply_persisted_layout` widened for tests
- [ ] `src/terminals/notes.rs` — `has_note_text`, `append_task_from_text`, `prepend_task_from_text`
- [ ] `src/agents/mod.rs:208` — `lifecycle` accessor only for tests

---

## 9. Tests: large inline test blocks in impl files

> Prefer separate sibling test submodules: `foo.rs` + `foo/tests.rs`.
> Move large inline test blocks out of impl files.

- [ ] `src/hud/state.rs` — 576 lines of `#[cfg(test)]` block (test compat aggregate)
- [ ] `src/terminals/presentation.rs` — 667 lines of `#[cfg(test)]` block
- [ ] `src/terminals/fonts.rs` — 350 lines of `#[cfg(test)]` block
- [ ] `src/hud/persistence.rs` — 352 lines of `#[cfg(test)]` block (already has `persistence/tests.rs` too)
- [ ] `src/terminals/daemon/server.rs` — 349 lines of `#[cfg(test)]` block
- [ ] `src/hud/modules/mod.rs` — 266 lines of `#[cfg(test)]` block (legacy shims)
- [ ] `src/terminals/pty_spawn.rs` — 123 lines of `#[cfg(test)]` block
- [ ] `src/main.rs` — 71 lines of `#[cfg(test)]` block

---

## 10. Tests: parser/serializer/helper tests in giant central buckets

> Parser/serializer/helper tests should not live in giant central buckets.

### Config parser tests in `src/tests/scene.rs` (should be near `src/app_config.rs`)
- [ ] `parses_neozeus_toml_config` (line 153)
- [ ] `neozeus_config_path_resolution_prefers_explicit_then_xdg_then_home_then_cwd` (line 183)
- [ ] `primary_window_config_can_use_loaded_toml_overrides` (line 232)
- [ ] `parses_output_mode_and_dimensions` (line 271)
- [ ] `offscreen_synthetic_window_config_is_hidden_and_windowed` (line 292)
- [ ] `parses_optional_window_scale_factor_override` (line 324)
- [ ] `parses_force_fallback_adapter_override` (line 337)
- [ ] `resolves_disable_pipelined_rendering_for_wayland_desktop_only` (line 360)
- [ ] `resolves_linux_window_backend_policy` (line 400)

### Font/kitty config tests in `src/tests/terminals.rs` (should be near `src/terminals/fonts.rs`)
- [ ] `measured_cell_metrics_grow_with_font_size` (line 125)
- [ ] `larger_measured_cells_reduce_terminal_grid_in_same_viewport` (line 134)
- [ ] `parses_font_family_from_included_kitty_config` (line 1489)
- [ ] `kitty_config_lookup_prefers_explicit_directory_over_other_locations` (line 1511)
- [ ] `configured_terminal_font_path_resolves_exact_primary_face` (line 1542)
- [ ] `dump_terminal_font_reference_sample` (line 1561)
- [ ] `resolves_effective_terminal_font_stack_on_host` (line 1676)
- [ ] `detects_special_font_ranges` (line 1687)

### Daemon protocol/server tests in `src/tests/terminals.rs` (should be near `src/terminals/daemon/`)
- [ ] `daemon_socket_path_prefers_override_then_xdg_runtime_then_tmp_user` (line 3063)
- [ ] `daemon_protocol_roundtrip_preserves_terminal_messages` (line 3097)
- [ ] `daemon_server_cleans_up_stale_socket_file` (line 3131)
- [ ] `daemon_create_attach_command_output_and_kill_roundtrip` (line 3145)
- [ ] `daemon_sessions_survive_client_reconnect` (line 3185)
- [ ] `daemon_exited_sessions_remain_listed_until_explicit_kill` (line 3225)
- [ ] `daemon_session_listing_preserves_creation_order_not_lexical_order` (line 3271)
- [ ] `daemon_runtime_bridge_pushes_initial_snapshot_and_forwards_commands` (line 3320)
- [ ] `daemon_resize_session_request_succeeds` (line 3346)
- [ ] `daemon_runtime_bridge_applies_streamed_updates` (line 3361)
- [ ] `daemon_attach_missing_session_returns_error` (line 3391)
- [ ] `daemon_kill_missing_session_returns_error` (line 3403)
- [ ] `daemon_multiple_clients_receive_updates_for_same_session` (line 3416)
- [ ] `daemon_protocol_rejects_truncated_frame` (line 3476)
- [ ] `daemon_protocol_rejects_trailing_bytes_in_frame` (line 3486)
- [ ] `daemon_resize_session_updates_attached_surface_dimensions` (line 3507)
- [ ] `daemon_duplicate_attach_in_same_client_is_rejected` (line 3526)
- [ ] `daemon_killing_one_session_preserves_other_sessions` (line 3544)
- [ ] `daemon_session_lifecycle_churn_stays_consistent` (line 3567)

---

## 11. Modules: over-exposed `pub(crate)` items (only used within own module)

> `pub(crate)` > `pub`; expose only true subsystem API.

94 items are `pub(crate)` but never referenced outside their own module tree. Should be private or `pub(super)`.

### `src/agents/mod.rs`
- [ ] `AgentRecord` (line 41)
- [ ] `AgentRuntimeLifecycle` (line 107)
- [ ] `AgentRuntimeLifecycle::from_runtime` (line 117)
- [ ] `AgentRuntimeLink` (line 128)

### `src/app/bootstrap.rs`
- [ ] `primary_window_plugin_config_for_with_config` (line 263)

### `src/app/dispatch.rs`
- [ ] `AppCommandContext` (line 109)

### `src/app/output.rs`
- [ ] `create_final_frame_image` (line 137)
- [ ] `FinalFrameReadbackMeta` (line 193)
- [ ] `final_frame_format` (line 459)

### `src/app/session.rs`
- [ ] `HudWidgetPlacement` (line 13)

### `src/app_config.rs`
- [ ] `NeoZeusTerminalConfig` (line 26)
- [ ] `NeoZeusWindowConfig` (line 33)
- [ ] `resolve_neozeus_config_path` (line 67)

### `src/conversations/mod.rs`
- [ ] `MessageId` (line 14)
- [ ] `MessageRecord` (line 29)
- [ ] `ConversationRecord` (line 38)

### `src/conversations/persistence.rs`
- [ ] `PersistedConversationMessage` (line 13)
- [ ] `PersistedConversationRecord` (line 19)
- [ ] `PersistedConversations` (line 25)

### `src/hud/bloom.rs`
- [ ] `resolve_agent_list_bloom_intensity` (line 82)
- [ ] `resolve_agent_list_bloom_debug_previews` (line 93)
- [ ] `AgentListBloomCameraMarker` (line 105)
- [ ] `AgentListBloomCompositeMarker` (line 108)
- [ ] `AgentListBloomBlurUniform` (line 144)
- [ ] `AgentListBloomSourceKind` (line 175)
- [ ] `AgentListBloomSourceSegment` (line 181)
- [ ] `AgentListBloomSourceSprite` (line 189)
- [ ] `HudWidgetBloomSetupContext` (line 519)
- [ ] `HudWidgetBloomContext` (line 857)
- [ ] `agent_list_bloom_layer` (line 1193)
- [ ] `agent_list_bloom_z` (line 1202)

### `src/hud/input.rs`
- [ ] `HudPointerContext` (line 48)

### `src/hud/modules/agent_list/mod.rs`
- [ ] `AGENT_LIST_HEADER_HEIGHT` (line 10)
- [ ] `AGENT_LIST_LEFT_RAIL_WIDTH` (line 11)
- [ ] `AGENT_LIST_ROW_MARKER_WIDTH` (line 12)
- [ ] `AGENT_LIST_ROW_MARKER_GAP` (line 13)
- [ ] `AGENT_LIST_ROW_GAP` (line 14)
- [ ] `AgentRow` (line 33)
- [ ] `agent_row_stride` (line 76)
- [ ] `agent_list_content_height` (line 84)

### `src/hud/modules/conversation_list.rs`
- [ ] `ConversationRow` (line 15)

### `src/hud/modules/debug_toolbar/mod.rs`
- [ ] `DebugToolbarAction` (line 14)
- [ ] `DebugToolbarButton` (line 24)

### `src/hud/persistence.rs`
- [ ] `PersistedHudModuleState` (line 15)
- [ ] `PersistedHudState` (line 21)
- [ ] `resolve_hud_layout_path_with` (line 34)
- [ ] `parse_persisted_hud_state` (line 186)
- [ ] `serialize_persisted_hud_state` (line 205)
- [ ] `apply_persisted_layout` (line 332)

### `src/hud/render.rs`
- [ ] `HudColors::TITLE` (line 40)
- [ ] `HudColors::MESSAGE_BOX` (line 50)
- [ ] `hud_rect_to_scene` (line 76)
- [ ] `HudPainter::text_size` (line 147)

### `src/hud/state.rs`
- [ ] `HUD_ANIMATION_EPSILON` (line 18)
- [ ] `HudModuleShell` (line 41)
- [ ] `HudModuleInstance` (line 94)
- [ ] `HudLayoutState::iter_z_order_front_to_back` (line 137)
- [ ] `HudModalState::close_message_box_and_discard_draft` (line 348)
- [ ] `HudState::input_capture_state` (line 423) [#[cfg(test)]]

### `src/hud/view_models.rs`
- [ ] `ThreadMessageView` (line 43)

### `src/hud/widgets.rs`
- [ ] `widget_definition` (line 97)

### `src/startup.rs`
- [ ] `SceneSetupContext` (line 154)

### `src/terminals/daemon/client.rs`
- [ ] `connect_or_start_default` (line 93)

### `src/terminals/daemon/session.rs`
- [ ] `AttachedSubscriber` (line 40)

### `src/terminals/fonts.rs`
- [ ] `initialize_terminal_text_renderer` (line 217)
- [ ] `resolve_terminal_font_report` (line 263)
- [ ] `find_kitty_config_path` (line 391)

### `src/terminals/lifecycle.rs`
- [ ] `remove_terminal_with_projection` (line 39)

### `src/terminals/mailbox.rs`
- [ ] `MailboxPush` (line 15)
- [ ] `push_frame` (line 30)
- [ ] `push_status` (line 47)

### `src/terminals/notes.rs`
- [ ] `append_task_from_text` (line 75)
- [ ] `prepend_task_from_text` (line 100)
- [ ] `resolve_terminal_notes_path_with` (line 124)
- [ ] `parse_terminal_notes` (line 190)
- [ ] `serialize_terminal_notes` (line 238)

### `src/terminals/presentation.rs`
- [ ] `HUD_FRAME_PADDING` (line 13)
- [ ] `ACTIVE_TERMINAL_MARGIN` (line 14)
- [ ] `DIRECT_INPUT_FRAME_OUTSET` (line 15)
- [ ] `ActiveTerminalLayout` (line 23)
- [ ] `spawn_terminal_presentation` (line 46)

### `src/terminals/registry.rs`
- [ ] `create_terminal_without_focus_with_session` (line 144)
- [ ] `create_terminal_with_slot_and_session` (line 241)

### `src/terminals/runtime.rs`
- [ ] `TerminalRuntimeSpawner::noop` (line 33)
- [ ] `spawn_daemon_terminal_runtime` (line 201)

### `src/terminals/session_persistence.rs`
- [ ] `ReconciledTerminalSessions` (line 33)
- [ ] `resolve_terminal_sessions_path_with` (line 57)
- [ ] `parse_persisted_terminal_sessions` (line 273)
- [ ] `build_persisted_terminal_sessions` (line 354)

### `src/ui/composer/layout.rs`
- [ ] `MessageBoxActionButton` (line 22)
- [ ] `TaskDialogActionButton` (line 29)

### `src/ui/composer/mod.rs`
- [ ] `TextEditorYankState` (line 14)

### `src/verification.rs`
- [ ] `resolve_verification_scenario` (line 51)
- [ ] `VerificationScenarioContext` (line 190)

---

## 12. Modules: singleton `mod.rs` root without subtree clarity

> Flatten singleton `mod.rs` roots when no subtree clarity is gained.

- [ ] `src/ui/mod.rs` — wraps only `composer/` submodule; `ui` namespace adds no clarity over `crate::composer`

---

## 13. Modules: leaf imports over root re-export churn

(Previously section 11)

> Prefer leaf imports over root re-export churn.

`terminals/mod.rs` re-exports 126 items. `hud/mod.rs` re-exports ~50 items. Internal sibling modules import through the root instead of from the leaf submodule directly.

### Internal `terminals/` callers using root re-exports (17 import lines)
- [ ] `src/terminals/ansi_surface.rs` — imports through `crate::terminals::` not leaf
- [ ] `src/terminals/damage/tests.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/damage.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/presentation_state.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/bridge.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/mailbox.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/runtime.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/backend.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/notes.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/lifecycle.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/pty_spawn.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/registry.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/daemon/protocol.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/daemon/server.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/daemon/client.rs` — imports through `crate::terminals::`
- [ ] `src/terminals/daemon/protocol/tests.rs` — imports through `crate::terminals::`

### Internal `hud/` callers using root re-exports (12 import lines)
- [ ] `src/hud/capture.rs` — imports through `crate::hud::`
- [ ] `src/hud/widgets.rs` — imports through `crate::hud::`
- [ ] `src/hud/animation.rs` — imports through `crate::hud::`
- [ ] `src/hud/persistence.rs` — imports through `crate::hud::` (3 import lines)
- [ ] `src/hud/persistence/tests.rs` — imports through `crate::hud::`
- [ ] `src/hud/modules/thread_pane.rs` — imports through `crate::hud::`
- [ ] `src/hud/modules/agent_list/render.rs` — imports through `crate::hud::`
- [ ] `src/hud/modules/debug_toolbar/buttons.rs` — imports through `crate::hud::`
- [ ] `src/hud/modules/debug_toolbar/mod.rs` — imports through `crate::hud::`
- [ ] `src/hud/modules/debug_toolbar/render.rs` — imports through `crate::hud::`

---

## 14. Tests: module-local behavior tests in central test files

> Module-local behavior tests belong with the module.

These test a single module's behavior but live in `src/tests/` instead of near the owning module.

### Agent-list tests in `src/tests/hud.rs` (should be near `hud/modules/agent_list/`)
- [ ] `sync_structural_hud_layout_docks_agent_list_to_full_height_left_column` (line 148)
- [ ] `agent_row_rect_splits_main_and_marker_geometry` (line 180)
- [ ] `agent_rows_use_derived_agent_view_labels` (line 740)
- [ ] `agent_rows_follow_terminal_order_and_focus` (line 781)
- [ ] `agent_rows_mark_hovered_agent` (line 832)
- [ ] `agent_list_is_not_draggable` (line 887)
- [ ] `clicking_agent_list_row_emits_focus_and_isolate_commands` (line 1327)
- [ ] `agent_list_scroll_clamps_to_content_height` (line 1458)

### Conversation-list tests in `src/tests/hud.rs` (should be near `hud/modules/conversation_list.rs`)
- [ ] `clicking_conversation_list_row_emits_focus_and_isolate_commands` (line 1408)

### Animation tests in `src/tests/hud.rs` (should be near `hud/animation.rs`)
- [ ] `animate_hud_modules_moves_current_rect_and_alpha_toward_target` (line 1176)
- [ ] `hud_needs_redraw_when_drag_or_animation_is_active` (line 1601)

### Setup tests in `src/tests/hud.rs` (should be near `hud/setup.rs`)
- [ ] `setup_hud_requests_initial_redraw` (line 98)

### Compositor tests in `src/tests/hud.rs` (should be near `hud/compositor.rs`)
- [ ] `sync_hud_offscreen_compositor_hides_vello_canvas_and_binds_texture` (line 204)
- [ ] `sync_hud_offscreen_compositor_leaves_modal_vello_canvas_visible` (line 298)
- [ ] `hud_composite_quad_matches_upstream_vello_canvas_contract` (line 361)
- [ ] `upstream_vello_present_contract_preserves_target_orange_bytes` (line 423)

### Raster tests in `src/tests/terminals.rs` (should be near `terminals/raster.rs`)
- [ ] `alpha_blend_preserves_transparent_glyph_background` (line 462)
- [ ] `standalone_text_renderer_rasterizes_ascii_glyph` (line 1696)
- [ ] `sync_terminal_texture_renders_visible_text_on_last_row` (line 1818)
- [ ] `sync_terminal_texture_updates_pixels_when_last_row_text_changes` (line 1852)
- [ ] `sync_terminal_texture_keeps_cached_switch_frame_until_resized_surface_arrives` (line 1146)
- [ ] `sync_terminal_texture_promotes_active_terminal_once_resized_surface_arrives` (line 1264)

### Presentation tests in `src/tests/terminals.rs` (should be near `terminals/presentation.rs`)
- [ ] `snap_to_pixel_grid_respects_window_scale_factor` (line 494)
- [ ] `active_terminal_target_position_accounts_for_texture_parity` (line 502)
- [ ] `active_terminal_viewport_reserves_agent_list_column` (line 558)
- [ ] `active_terminal_resize_requests_follow_viewport_grid_policy` (line 1359)
- [ ] `show_all_presentations_remain_visible_when_no_terminal_is_active` (line 2015)
- [ ] `terminal_panel_frames_are_hidden_without_direct_input_mode` (line 2090)
- [ ] `direct_input_mode_shows_orange_terminal_frame` (line 2114)
- [ ] `disconnected_terminal_shows_red_status_frame` (line 2179)
- [ ] `startup_loading_shows_active_placeholder_before_first_surface_arrives` (line 2245)
- [ ] `startup_loading_temporarily_overrides_isolate_to_show_all_pending_terminals` (line 2316)
- [ ] `isolate_visibility_policy_with_missing_terminal_degrades_to_show_all` (line 2771)

### Registry/manager tests in `src/tests/terminals.rs` (should be near `terminals/registry.rs`)
- [ ] `drain_terminal_updates_keeps_latest_frame_and_status` (line 1410)
- [ ] `poll_terminal_snapshots_keeps_latest_status_over_latest_frame_runtime` (line 1449)
- [ ] `terminal_creation_order_stays_stable_when_focus_changes` (line 1955)
- [ ] `terminal_can_be_created_without_becoming_active` (line 1970)
- [ ] `terminal_with_session_name_is_retained_in_manager_state` (line 1983)

### Backend tests in `src/tests/terminals.rs` (should be near `terminals/backend.rs`)
- [ ] `send_command_payload_bytes_turn_multiline_text_into_enter_sequences` (line 1918)

### Debug toolbar tests in `src/tests/hud.rs` (should be near `debug_toolbar/`)
- [ ] `clicking_debug_toolbar_button_emits_spawn_terminal_command` (line 1201)
- [ ] `clicking_debug_toolbar_command_button_emits_terminal_command` (line 1267)
- [ ] `debug_toolbar_buttons_include_module_toggle_entries` (line 1524)
- [ ] `debug_toolbar_module_toggle_buttons_reflect_enabled_state` (line 1558)

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
