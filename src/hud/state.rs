use crate::{agents::AgentId, hud::view_models::AgentListRowKey, terminals::TerminalId};

use super::widgets::{HudWidgetDefinition, HudWidgetKey, HUD_WIDGET_DEFINITIONS};
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(crate) const HUD_TITLEBAR_HEIGHT: f32 = 28.0;
pub(crate) const HUD_INFO_BAR_HEIGHT: f32 = 60.0;
pub(crate) const HUD_MODULE_PADDING: f32 = 10.0;
pub(crate) const HUD_ROW_HEIGHT: f32 = 28.0;
pub(crate) const HUD_AGENT_LIST_WIDTH: f32 = 300.0;
const HUD_ANIMATION_EPSILON: f32 = 0.25;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HudRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl HudRect {
    /// Returns whether a point lies inside the rectangle, inclusive of its edges.
    ///
    /// Inclusive comparisons make hit-testing stable on exact borders.
    pub(crate) fn contains(self, point: Vec2) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.w
            && point.y >= self.y
            && point.y <= self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudModuleLayout {
    pub(crate) enabled: bool,
    pub(crate) rect: HudRect,
    pub(crate) alpha: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::hud) struct HudModuleShell {
    pub(crate) enabled: bool,
    pub(crate) target_rect: HudRect,
    pub(crate) current_rect: HudRect,
    pub(crate) target_alpha: f32,
    pub(crate) current_alpha: f32,
}

impl HudModuleShell {
    pub(crate) fn canonical_layout(&self) -> HudModuleLayout {
        HudModuleLayout {
            enabled: self.enabled,
            rect: self.target_rect,
            alpha: self.target_alpha,
        }
    }

    pub(crate) fn presentation_layout(&self) -> HudModuleLayout {
        HudModuleLayout {
            enabled: self.enabled,
            rect: self.current_rect,
            alpha: self.current_alpha,
        }
    }

    pub(crate) fn set_canonical_rect(&mut self, rect: HudRect, snap_presentation: bool) {
        self.target_rect = rect;
        if snap_presentation {
            self.current_rect = rect;
        }
    }

    pub(crate) fn set_canonical_alpha(&mut self, alpha: f32, snap_presentation: bool) {
        self.target_alpha = alpha;
        if snap_presentation {
            self.current_alpha = alpha;
        }
    }

    /// Returns the draggable titlebar strip for the module's current onscreen rectangle.
    ///
    /// The titlebar height is capped by the module height so tiny modules still produce a valid rect.
    pub(crate) fn titlebar_rect(&self) -> HudRect {
        HudRect {
            x: self.current_rect.x,
            y: self.current_rect.y,
            w: self.current_rect.w,
            h: HUD_TITLEBAR_HEIGHT.min(self.current_rect.h),
        }
    }

    /// Returns whether the module shell is still interpolating toward its target rect/alpha.
    ///
    /// Position and size use the shared HUD epsilon, while alpha uses a slightly looser fixed
    /// threshold.
    pub(crate) fn is_animating(&self) -> bool {
        (self.current_rect.x - self.target_rect.x).abs() > HUD_ANIMATION_EPSILON
            || (self.current_rect.y - self.target_rect.y).abs() > HUD_ANIMATION_EPSILON
            || (self.current_rect.w - self.target_rect.w).abs() > HUD_ANIMATION_EPSILON
            || (self.current_rect.h - self.target_rect.h).abs() > HUD_ANIMATION_EPSILON
            || (self.current_alpha - self.target_alpha).abs() > 0.01
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AgentListDragState {
    pub(crate) pressed_row: Option<AgentListRowKey>,
    pub(crate) pressed_agent: Option<AgentId>,
    pub(crate) press_origin: Option<Vec2>,
    pub(crate) dragging_agent: Option<AgentId>,
    pub(crate) drag_cursor: Option<Vec2>,
    pub(crate) drag_grab_offset_y: f32,
    pub(crate) last_reorder_index: Option<usize>,
}

impl AgentListDragState {
    /// Clears the transient press/drag preview state for the agent list.
    pub(crate) fn clear(&mut self) {
        self.pressed_row = None;
        self.pressed_agent = None;
        self.press_origin = None;
        self.dragging_agent = None;
        self.drag_cursor = None;
        self.drag_grab_offset_y = 0.0;
        self.last_reorder_index = None;
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AgentListUiState {
    pub(crate) scroll_offset: f32,
    pub(crate) hovered_row: Option<AgentListRowKey>,
    /// When enabled, render the selected agent context card even without pointer hover.
    pub(crate) show_selected_context: bool,
    pub(crate) drag: AgentListDragState,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct ConversationListUiState {
    pub(crate) scroll_offset: f32,
    pub(crate) hovered_agent: Option<AgentId>,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct ThreadPaneUiState;

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct InfoBarUiState;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::hud) struct HudModuleInstance {
    pub(crate) shell: HudModuleShell,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudDragState {
    pub(crate) module_id: HudWidgetKey,
    pub(crate) grab_offset: Vec2,
}

#[derive(Resource, Default)]
pub(crate) struct HudLayoutState {
    pub(super) modules: BTreeMap<HudWidgetKey, HudModuleInstance>,
    pub(super) z_order: Vec<HudWidgetKey>,
    pub(super) drag: Option<HudDragState>,
    pub(super) dirty_layout: bool,
}

impl HudLayoutState {
    /// Returns the retained module instance for a given module id.
    ///
    /// This is the read-only accessor used by most HUD systems.
    pub(in crate::hud) fn get(&self, id: HudWidgetKey) -> Option<&HudModuleInstance> {
        self.modules.get(&id)
    }

    /// Returns whether one module is currently enabled.
    pub(crate) fn module_enabled(&self, id: HudWidgetKey) -> bool {
        self.modules
            .get(&id)
            .is_some_and(|module| module.shell.enabled)
    }

    /// Returns the current docked agent-list width when that module is enabled.
    pub(crate) fn module_layout(&self, id: HudWidgetKey) -> Option<HudModuleLayout> {
        self.modules
            .get(&id)
            .map(|module| module.shell.canonical_layout())
    }

    pub(crate) fn docked_agent_list_width(&self) -> f32 {
        self.module_layout(HudWidgetKey::AgentList)
            .filter(|layout| layout.enabled)
            .map(|layout| layout.rect.w)
            .unwrap_or(0.0)
    }

    /// Returns the reserved top header height when the info-bar module is enabled.
    pub(crate) fn reserved_header_height(&self) -> f32 {
        self.module_layout(HudWidgetKey::InfoBar)
            .filter(|layout| layout.enabled)
            .map(|layout| layout.rect.h)
            .unwrap_or(0.0)
    }

    /// Returns mutable access to one retained module instance.
    ///
    /// Systems that mutate shell state go through this helper.
    pub(in crate::hud) fn get_mut(&mut self, id: HudWidgetKey) -> Option<&mut HudModuleInstance> {
        self.modules.get_mut(&id)
    }

    /// Iterates module ids from back to front in the stored z-order vector.
    ///
    /// The backmost module appears first; use the front-to-back helper when hit-testing.
    pub(crate) fn iter_z_order(&self) -> impl Iterator<Item = HudWidgetKey> + '_ {
        self.z_order.iter().copied()
    }

    /// Iterates module ids from frontmost to backmost.
    ///
    /// This is the ordering needed for pointer hit-testing so the visually topmost module wins.
    fn iter_z_order_front_to_back(&self) -> impl Iterator<Item = HudWidgetKey> + '_ {
        self.z_order.iter().rev().copied()
    }

    /// Inserts or replaces a module instance and ensures it exists in z-order.
    ///
    /// First insert appends the module at the back; replacing an existing module preserves its current
    /// z position.
    pub(in crate::hud) fn insert(&mut self, id: HudWidgetKey, module: HudModuleInstance) {
        self.modules.insert(id, module);
        if !self.z_order.contains(&id) {
            self.z_order.push(id);
        }
    }

    /// Moves a module id to the front of the z-order list.
    ///
    /// Any previous occurrence is removed first so the vector stays deduplicated.
    pub(crate) fn raise_to_front(&mut self, id: HudWidgetKey) {
        self.z_order.retain(|existing| *existing != id);
        self.z_order.push(id);
    }

    /// Enables or disables a module shell and snaps its visible alpha to the new state.
    ///
    /// Widget show/hide toggles are intentionally instant rather than faded so shortcut-driven HUD
    /// visibility changes feel immediate.
    pub(crate) fn set_module_enabled(&mut self, id: HudWidgetKey, enabled: bool) {
        let Some(module) = self.modules.get_mut(&id) else {
            return;
        };
        if module.shell.enabled == enabled {
            return;
        }
        module.shell.enabled = enabled;
        module
            .shell
            .set_canonical_alpha(if enabled { 1.0 } else { 0.0 }, true);
        self.dirty_layout = true;
    }

    /// Restores a module to its baked-in default shell state.
    ///
    /// Resetting also brings the module to the front and marks layout dirty so persistence/rendering
    /// will pick up the change.
    pub(crate) fn reset_module(&mut self, id: HudWidgetKey) {
        let Some(definition) = HUD_WIDGET_DEFINITIONS
            .iter()
            .find(|definition| definition.key == id)
        else {
            return;
        };
        self.modules
            .insert(id, default_hud_module_instance(definition));
        self.raise_to_front(id);
        self.dirty_layout = true;
    }

    /// Returns the frontmost enabled module whose current rect contains the point.
    ///
    /// Hit-testing uses current rects rather than target rects so interaction tracks what the user is
    /// actually seeing during animation.
    pub(crate) fn topmost_enabled_at(&self, point: Vec2) -> Option<HudWidgetKey> {
        self.iter_z_order_front_to_back().find(|id| {
            self.modules.get(id).is_some_and(|module| {
                module.shell.enabled && module.shell.current_rect.contains(point)
            })
        })
    }

    /// Returns whether any module shell in the layout is still animating.
    ///
    /// This is used as a coarse "HUD still moving" signal for redraw decisions.
    pub(crate) fn is_animating(&self) -> bool {
        self.modules
            .values()
            .any(|module| module.shell.is_animating())
    }
}

/// Derived input-capture projection for the currently active terminal.
///
/// Direct-input capture is reconciled against projected terminal focus; it should not outlive or
/// diverge from the authoritative focus intent.
#[derive(Resource, Default)]
pub(crate) struct HudInputCaptureState {
    pub(crate) direct_input_terminal: Option<TerminalId>,
}

impl HudInputCaptureState {
    /// Enables direct terminal input capture for one terminal in the split input-capture resource.
    ///
    /// Opening direct input also closes both modal editors so only one input sink remains active.
    pub(crate) fn open_direct_terminal_input(
        &mut self,
        composer: &mut crate::composer::ComposerState,
        target_terminal: TerminalId,
    ) {
        composer.discard_current_message();
        composer.close_task_editor();
        self.direct_input_terminal = Some(target_terminal);
    }

    /// Clears direct terminal input capture in the split input-capture resource.
    pub(crate) fn close_direct_terminal_input(&mut self) {
        self.direct_input_terminal = None;
    }

    /// Toggles direct terminal input capture for a terminal in the split input-capture resource.
    ///
    /// Returns `true` when the requested terminal ended up capturing input.
    pub(crate) fn toggle_direct_terminal_input(
        &mut self,
        composer: &mut crate::composer::ComposerState,
        target_terminal: TerminalId,
    ) -> bool {
        if self.direct_input_terminal == Some(target_terminal) {
            self.close_direct_terminal_input();
            return false;
        }
        self.open_direct_terminal_input(composer, target_terminal);
        true
    }

    /// Clears split direct-input capture if it no longer matches the active terminal.
    pub(crate) fn reconcile_direct_terminal_input(&mut self, active_id: Option<TerminalId>) {
        if self
            .direct_input_terminal
            .is_some_and(|terminal_id| Some(terminal_id) != active_id)
        {
            self.close_direct_terminal_input();
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalVisibilityPolicy {
    #[default]
    ShowAll,
    Isolate(TerminalId),
}

/// Derived terminal-visibility policy projected from focus intent and visibility mode.
#[derive(Resource, Default)]
pub(crate) struct TerminalVisibilityState {
    pub(crate) policy: TerminalVisibilityPolicy,
}

/// Returns the fixed docked rectangle used by the top info-bar module.
///
/// The info bar is pinned to the top edge and spans the full window width with a fixed two-row
/// height matching the Zeus-style reference layout.
pub(crate) fn docked_info_bar_rect(window: &Window) -> HudRect {
    HudRect {
        x: 0.0,
        y: 0.0,
        w: window.width(),
        h: HUD_INFO_BAR_HEIGHT.min(window.height()),
    }
}

/// Returns the fixed docked rectangle used by the agent-list module with no top inset.
///
/// Tests use this helper when they want the agent list docked without the structural info-bar
/// reservation.
#[cfg(test)]
pub(crate) fn docked_agent_list_rect(window: &Window) -> HudRect {
    docked_agent_list_rect_with_top_inset(window, 0.0)
}

/// Returns the fixed docked rectangle used by the agent-list module beneath one reserved top inset.
pub(crate) fn docked_agent_list_rect_with_top_inset(window: &Window, top_inset: f32) -> HudRect {
    let top_inset = top_inset.clamp(0.0, window.height());
    HudRect {
        x: 0.0,
        y: top_inset,
        w: HUD_AGENT_LIST_WIDTH.min(window.width()),
        h: (window.height() - top_inset).max(0.0),
    }
}

/// Builds the default retained instance for one HUD module definition.
///
/// Only generic shell/layout state lives here; widget-local retained state is stored separately in
/// per-widget resources.
pub(in crate::hud) fn default_hud_module_instance(
    definition: &HudWidgetDefinition,
) -> HudModuleInstance {
    HudModuleInstance {
        shell: HudModuleShell {
            enabled: definition.default_enabled,
            target_rect: definition.default_rect,
            current_rect: definition.default_rect,
            target_alpha: if definition.default_enabled { 1.0 } else { 0.0 },
            current_alpha: if definition.default_enabled { 1.0 } else { 0.0 },
        },
    }
}

#[cfg(test)]
pub(crate) use tests::{HudModalState, HudState};


#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::TextEditorState;
    use std::collections::BTreeMap;

    #[derive(Resource, Default)]
    pub(crate) struct HudModalState {
        pub(crate) message_box: TextEditorState,
        pub(crate) task_dialog: TextEditorState,
    }

    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct HudState {
        modules: BTreeMap<HudWidgetKey, HudModuleInstance>,
        pub(crate) z_order: Vec<HudWidgetKey>,
        pub(crate) drag: Option<HudDragState>,
        pub(crate) dirty_layout: bool,
        pub(crate) message_box: TextEditorState,
        pub(crate) task_dialog: TextEditorState,
        pub(crate) direct_input_terminal: Option<TerminalId>,
        message_box_drafts: BTreeMap<TerminalId, String>,
    }

    #[allow(
        dead_code,
        reason = "test compatibility aggregate preserves pre-split HUD helper ergonomics"
    )]
    impl HudState {
        /// Returns the retained test module instance for a given module id.
        ///
        /// This mirrors [`HudLayoutState::get`] on the legacy aggregate test helper.
        pub(in crate::hud) fn get(&self, id: HudWidgetKey) -> Option<&HudModuleInstance> {
            self.modules.get(&id)
        }

        /// Returns mutable access to one module inside the aggregate test HUD state.
        pub(in crate::hud) fn get_mut(&mut self, id: HudWidgetKey) -> Option<&mut HudModuleInstance> {
            self.modules.get_mut(&id)
        }

        /// Iterates test HUD module ids in stored back-to-front order.
        pub(crate) fn iter_z_order(&self) -> impl Iterator<Item = HudWidgetKey> + '_ {
            self.z_order.iter().copied()
        }

        /// Inserts or replaces a module in the aggregate test HUD state and ensures z-order membership.
        pub(in crate::hud) fn insert(&mut self, id: HudWidgetKey, module: HudModuleInstance) {
            self.modules.insert(id, module);
            if !self.z_order.contains(&id) {
                self.z_order.push(id);
            }
        }

        /// Inserts one module using its built-in default shell definition.
        pub(crate) fn insert_default_module(&mut self, id: HudWidgetKey) {
            let Some(definition) = HUD_WIDGET_DEFINITIONS
                .iter()
                .find(|definition| definition.key == id)
            else {
                return;
            };
            self.insert(id, default_hud_module_instance(definition));
        }

        /// Overwrites all externally relevant shell fields for one test HUD module.
        pub(crate) fn set_module_shell_state(
            &mut self,
            id: HudWidgetKey,
            enabled: bool,
            current_rect: HudRect,
            target_rect: HudRect,
            current_alpha: f32,
            target_alpha: f32,
        ) {
            let Some(module) = self.modules.get_mut(&id) else {
                return;
            };
            module.shell.enabled = enabled;
            module.shell.current_rect = current_rect;
            module.shell.target_rect = target_rect;
            module.shell.current_alpha = current_alpha;
            module.shell.target_alpha = target_alpha;
        }

        /// Returns whether one test HUD module is enabled.
        pub(crate) fn module_enabled(&self, id: HudWidgetKey) -> Option<bool> {
            self.modules.get(&id).map(|module| module.shell.enabled)
        }

        /// Returns one test HUD module's target rectangle.
        pub(crate) fn module_target_rect(&self, id: HudWidgetKey) -> Option<HudRect> {
            self.modules.get(&id).map(|module| module.shell.target_rect)
        }

        /// Returns one test HUD module's current alpha.
        pub(crate) fn module_current_alpha(&self, id: HudWidgetKey) -> Option<f32> {
            self.modules
                .get(&id)
                .map(|module| module.shell.current_alpha)
        }

        /// Moves a module id to the front of the aggregate test HUD z-order.
        pub(crate) fn raise_to_front(&mut self, id: HudWidgetKey) {
            self.z_order.retain(|existing| *existing != id);
            self.z_order.push(id);
        }

        /// Enables or disables a module in the aggregate test HUD state and updates target alpha.
        pub(crate) fn set_module_enabled(&mut self, id: HudWidgetKey, enabled: bool) {
            let Some(module) = self.modules.get_mut(&id) else {
                return;
            };
            if module.shell.enabled == enabled {
                return;
            }
            module.shell.enabled = enabled;
            module.shell.target_alpha = if enabled { 1.0 } else { 0.0 };
            self.dirty_layout = true;
        }

        /// Restores one module in the aggregate test HUD state to its default definition.
        pub(crate) fn reset_module(&mut self, id: HudWidgetKey) {
            let Some(definition) = HUD_WIDGET_DEFINITIONS
                .iter()
                .find(|definition| definition.key == id)
            else {
                return;
            };
            self.modules
                .insert(id, default_hud_module_instance(definition));
            self.raise_to_front(id);
            self.dirty_layout = true;
        }

        /// Returns the frontmost enabled test module whose current rect contains the point.
        pub(crate) fn topmost_enabled_at(&self, point: Vec2) -> Option<HudWidgetKey> {
            self.z_order.iter().rev().copied().find(|id| {
                self.modules.get(id).is_some_and(|module| {
                    module.shell.enabled && module.shell.current_rect.contains(point)
                })
            })
        }

        /// Returns whether any module in the aggregate test HUD state is still animating.
        pub(crate) fn is_animating(&self) -> bool {
            self.modules
                .values()
                .any(|module| module.shell.is_animating())
        }

        /// Returns whether any modal/editor flag in the aggregate test HUD state owns keyboard input.
        pub(crate) fn keyboard_capture_active(&self) -> bool {
            self.message_box.visible || self.task_dialog.visible || self.direct_input_terminal.is_some()
        }

        /// Opens the message box inside the aggregate test HUD state and clears competing capture modes.
        pub(crate) fn open_message_box(&mut self, target_terminal: TerminalId) {
            self.task_dialog.close();
            self.direct_input_terminal = None;
            self.message_box.visible = true;
            self.message_box.load_text(
                self.message_box_drafts
                    .get(&target_terminal)
                    .map(String::as_str)
                    .unwrap_or_default(),
            );
            self.message_box.target_terminal = Some(target_terminal);
        }

        /// Closes the message box in the aggregate test HUD state while preserving drafts.
        pub(crate) fn close_message_box(&mut self) {
            if let Some(target_terminal) = self.message_box.target_terminal {
                self.message_box_drafts
                    .insert(target_terminal, self.message_box.text.clone());
            }
            self.message_box.close();
        }

        /// Closes the message box in the aggregate test HUD state and discards the current draft.
        fn close_message_box_and_discard_draft(&mut self) {
            if let Some(target_terminal) = self.message_box.target_terminal {
                self.message_box_drafts.remove(&target_terminal);
            }
            self.message_box.close_and_discard();
        }

        /// Opens the task dialog in the aggregate test HUD state and clears competing capture modes.
        pub(crate) fn open_task_dialog(&mut self, target_terminal: TerminalId, text: &str) {
            self.close_message_box();
            self.direct_input_terminal = None;
            self.task_dialog.visible = true;
            self.task_dialog.load_text(text);
            self.task_dialog.target_terminal = Some(target_terminal);
        }

        /// Closes the task dialog in the aggregate test HUD state.
        pub(crate) fn close_task_dialog(&mut self) {
            self.task_dialog.close();
        }

        /// Switches the aggregate test HUD state into direct-terminal-input mode for one terminal.
        pub(crate) fn open_direct_terminal_input(&mut self, target_terminal: TerminalId) {
            self.close_message_box();
            self.close_task_dialog();
            self.direct_input_terminal = Some(target_terminal);
        }

        /// Leaves direct-terminal-input mode in the aggregate test HUD state.
        pub(crate) fn close_direct_terminal_input(&mut self) {
            self.direct_input_terminal = None;
        }

        /// Toggles direct-terminal-input mode for the requested terminal in the aggregate test HUD state.
        ///
        /// Returns `true` when the mode ended up enabled and `false` when toggling disabled it.
        pub(crate) fn toggle_direct_terminal_input(&mut self, target_terminal: TerminalId) -> bool {
            if self.direct_input_terminal == Some(target_terminal) {
                self.close_direct_terminal_input();
                return false;
            }
            self.open_direct_terminal_input(target_terminal);
            true
        }

        /// Clears aggregate test direct-terminal-input capture if it no longer matches the active
        /// terminal.
        pub(crate) fn reconcile_direct_terminal_input(&mut self, active_id: Option<TerminalId>) {
            if self
                .direct_input_terminal
                .is_some_and(|terminal_id| Some(terminal_id) != active_id)
            {
                self.close_direct_terminal_input();
            }
        }

        /// Extracts the split layout resource view from the aggregate test HUD state.
        pub(crate) fn layout_state(&self) -> HudLayoutState {
            HudLayoutState {
                modules: self.modules.clone(),
                z_order: self.z_order.clone(),
                drag: self.drag,
                dirty_layout: self.dirty_layout,
            }
        }

        /// Extracts the split modal resource view from the aggregate test HUD state.
        pub(crate) fn modal_state(&self) -> HudModalState {
            HudModalState {
                message_box: self.message_box.clone(),
                task_dialog: self.task_dialog.clone(),
            }
        }

        /// Extracts the split input-capture resource view from the aggregate test HUD state.
        fn input_capture_state(&self) -> HudInputCaptureState {
            HudInputCaptureState {
                direct_input_terminal: self.direct_input_terminal,
            }
        }

        /// Consumes the aggregate test HUD state into the three split runtime resources.
        pub(crate) fn into_resources(self) -> (HudLayoutState, HudModalState, HudInputCaptureState) {
            (
                HudLayoutState {
                    modules: self.modules,
                    z_order: self.z_order,
                    drag: self.drag,
                    dirty_layout: self.dirty_layout,
                },
                HudModalState {
                    message_box: self.message_box,
                    task_dialog: self.task_dialog,
                },
                HudInputCaptureState {
                    direct_input_terminal: self.direct_input_terminal,
                },
            )
        }

        /// Reconstructs the aggregate test HUD state from the split runtime resources.
        ///
        /// This exists solely for test ergonomics around the newer resource split.
        pub(crate) fn from_resources(
            layout_state: &HudLayoutState,
            modal_state: &HudModalState,
            input_capture: &HudInputCaptureState,
        ) -> Self {
            Self {
                modules: layout_state.modules.clone(),
                z_order: layout_state.z_order.clone(),
                drag: layout_state.drag,
                dirty_layout: layout_state.dirty_layout,
                message_box: modal_state.message_box.clone(),
                task_dialog: modal_state.task_dialog.clone(),
                direct_input_terminal: input_capture.direct_input_terminal,
                message_box_drafts: BTreeMap::new(),
            }
        }
    }

    /// Verifies that the extracted agent-list drag state can be reset independently of the rest of the
    /// retained UI state.
    #[test]
    fn agent_list_drag_state_clear_resets_transient_fields() {
        let mut drag = AgentListDragState {
            pressed_row: None,
            pressed_agent: Some(crate::agents::AgentId(1)),
            press_origin: Some(Vec2::new(5.0, 7.0)),
            dragging_agent: Some(crate::agents::AgentId(1)),
            drag_cursor: Some(Vec2::new(11.0, 13.0)),
            drag_grab_offset_y: 9.0,
            last_reorder_index: Some(2),
        };

        drag.clear();

        assert_eq!(drag, AgentListDragState::default());
    }
}
