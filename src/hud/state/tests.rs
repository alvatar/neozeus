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
