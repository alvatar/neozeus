use crate::{
    hud::message_box::{HudMessageBoxState, HudTaskDialogState},
    terminals::TerminalId,
};
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(crate) const HUD_TITLEBAR_HEIGHT: f32 = 28.0;
pub(crate) const HUD_MODULE_PADDING: f32 = 10.0;
pub(crate) const HUD_ROW_HEIGHT: f32 = 28.0;
pub(crate) const HUD_BUTTON_HEIGHT: f32 = 28.0;
pub(crate) const HUD_BUTTON_GAP: f32 = 8.0;
pub(crate) const HUD_BUTTON_MIN_WIDTH: f32 = 72.0;
pub(crate) const HUD_AGENT_LIST_WIDTH: f32 = 300.0;
pub(crate) const HUD_ANIMATION_EPSILON: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HudModuleId {
    DebugToolbar,
    AgentList,
}

impl HudModuleId {
    // Implements number.
    pub(crate) const fn number(self) -> u8 {
        match self {
            Self::DebugToolbar => 0,
            Self::AgentList => 1,
        }
    }

    // Implements title.
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::DebugToolbar => "Debug Toolbar",
            Self::AgentList => "Agent List",
        }
    }

    // Implements title key.
    pub(crate) const fn title_key(self) -> &'static str {
        match self {
            Self::DebugToolbar => "DebugToolbar",
            Self::AgentList => "AgentList",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HudRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl HudRect {
    // Implements contains.
    pub(crate) fn contains(self, point: Vec2) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.w
            && point.y >= self.y
            && point.y <= self.y + self.h
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HudModuleShell {
    pub(crate) enabled: bool,
    pub(crate) target_rect: HudRect,
    pub(crate) current_rect: HudRect,
    pub(crate) target_alpha: f32,
    pub(crate) current_alpha: f32,
}

impl HudModuleShell {
    // Implements titlebar rect.
    pub(crate) fn titlebar_rect(&self) -> HudRect {
        HudRect {
            x: self.current_rect.x,
            y: self.current_rect.y,
            w: self.current_rect.w,
            h: HUD_TITLEBAR_HEIGHT.min(self.current_rect.h),
        }
    }

    // Returns whether animating.
    pub(crate) fn is_animating(&self) -> bool {
        (self.current_rect.x - self.target_rect.x).abs() > HUD_ANIMATION_EPSILON
            || (self.current_rect.y - self.target_rect.y).abs() > HUD_ANIMATION_EPSILON
            || (self.current_rect.w - self.target_rect.w).abs() > HUD_ANIMATION_EPSILON
            || (self.current_rect.h - self.target_rect.h).abs() > HUD_ANIMATION_EPSILON
            || (self.current_alpha - self.target_alpha).abs() > 0.01
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AgentListState {
    pub(crate) scroll_offset: f32,
    pub(crate) hovered_terminal: Option<TerminalId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DebugToolbarState;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HudModuleModel {
    DebugToolbar(DebugToolbarState),
    AgentList(AgentListState),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HudModuleInstance {
    pub(crate) shell: HudModuleShell,
    pub(crate) model: HudModuleModel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudDragState {
    pub(crate) module_id: HudModuleId,
    pub(crate) grab_offset: Vec2,
}

#[derive(Resource, Default)]
pub(crate) struct HudLayoutState {
    pub(crate) modules: BTreeMap<HudModuleId, HudModuleInstance>,
    pub(crate) z_order: Vec<HudModuleId>,
    pub(crate) drag: Option<HudDragState>,
    pub(crate) dirty_layout: bool,
}

impl HudLayoutState {
    // Implements get.
    pub(crate) fn get(&self, id: HudModuleId) -> Option<&HudModuleInstance> {
        self.modules.get(&id)
    }

    // Implements get mut.
    pub(crate) fn get_mut(&mut self, id: HudModuleId) -> Option<&mut HudModuleInstance> {
        self.modules.get_mut(&id)
    }

    // Implements iter z order.
    pub(crate) fn iter_z_order(&self) -> impl Iterator<Item = HudModuleId> + '_ {
        self.z_order.iter().copied()
    }

    // Implements iter z order front to back.
    pub(crate) fn iter_z_order_front_to_back(&self) -> impl Iterator<Item = HudModuleId> + '_ {
        self.z_order.iter().rev().copied()
    }

    // Inserts this value.
    pub(crate) fn insert(&mut self, id: HudModuleId, module: HudModuleInstance) {
        self.modules.insert(id, module);
        if !self.z_order.contains(&id) {
            self.z_order.push(id);
        }
    }

    // Implements raise to front.
    pub(crate) fn raise_to_front(&mut self, id: HudModuleId) {
        self.z_order.retain(|existing| *existing != id);
        self.z_order.push(id);
    }

    // Sets module enabled.
    pub(crate) fn set_module_enabled(&mut self, id: HudModuleId, enabled: bool) {
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

    // Implements reset module.
    pub(crate) fn reset_module(&mut self, id: HudModuleId) {
        let Some(definition) = HUD_MODULE_DEFINITIONS
            .iter()
            .find(|definition| definition.id == id)
        else {
            return;
        };
        self.modules
            .insert(id, default_hud_module_instance(definition));
        self.raise_to_front(id);
        self.dirty_layout = true;
    }

    // Implements topmost enabled at.
    pub(crate) fn topmost_enabled_at(&self, point: Vec2) -> Option<HudModuleId> {
        self.iter_z_order_front_to_back().find(|id| {
            self.modules.get(id).is_some_and(|module| {
                module.shell.enabled && module.shell.current_rect.contains(point)
            })
        })
    }

    // Returns whether animating.
    pub(crate) fn is_animating(&self) -> bool {
        self.modules
            .values()
            .any(|module| module.shell.is_animating())
    }
}

#[derive(Resource, Default)]
pub(crate) struct HudModalState {
    pub(crate) message_box: HudMessageBoxState,
    pub(crate) task_dialog: HudTaskDialogState,
}

impl HudModalState {
    // Implements keyboard capture active.
    pub(crate) fn keyboard_capture_active(&self, input_capture: &HudInputCaptureState) -> bool {
        self.message_box.visible
            || self.task_dialog.visible
            || input_capture.direct_input_terminal.is_some()
    }

    // Opens message box.
    pub(crate) fn open_message_box(
        &mut self,
        input_capture: &mut HudInputCaptureState,
        target_terminal: TerminalId,
    ) {
        self.task_dialog.close();
        input_capture.direct_input_terminal = None;
        self.message_box.reset_for_target(target_terminal);
    }

    // Closes message box.
    pub(crate) fn close_message_box(&mut self) {
        self.message_box.close();
    }

    // Closes message box and discard draft.
    pub(crate) fn close_message_box_and_discard_draft(&mut self) {
        self.message_box.close_and_discard_current();
    }

    // Opens task dialog.
    pub(crate) fn open_task_dialog(
        &mut self,
        input_capture: &mut HudInputCaptureState,
        target_terminal: TerminalId,
        text: &str,
    ) {
        self.close_message_box();
        input_capture.direct_input_terminal = None;
        self.task_dialog.open_with_text(target_terminal, text);
    }

    // Closes task dialog.
    pub(crate) fn close_task_dialog(&mut self) {
        self.task_dialog.close();
    }
}

#[derive(Resource, Default)]
pub(crate) struct HudInputCaptureState {
    pub(crate) direct_input_terminal: Option<TerminalId>,
}

#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HudState {
    pub(crate) modules: BTreeMap<HudModuleId, HudModuleInstance>,
    pub(crate) z_order: Vec<HudModuleId>,
    pub(crate) drag: Option<HudDragState>,
    pub(crate) dirty_layout: bool,
    pub(crate) message_box: HudMessageBoxState,
    pub(crate) task_dialog: HudTaskDialogState,
    pub(crate) direct_input_terminal: Option<TerminalId>,
}

#[cfg(test)]
#[allow(
    dead_code,
    reason = "test compatibility aggregate preserves pre-split HUD helper ergonomics"
)]
impl HudState {
    // Implements get.
    pub(crate) fn get(&self, id: HudModuleId) -> Option<&HudModuleInstance> {
        self.modules.get(&id)
    }

    // Implements get mut.
    pub(crate) fn get_mut(&mut self, id: HudModuleId) -> Option<&mut HudModuleInstance> {
        self.modules.get_mut(&id)
    }

    // Implements iter z order.
    pub(crate) fn iter_z_order(&self) -> impl Iterator<Item = HudModuleId> + '_ {
        self.z_order.iter().copied()
    }

    // Inserts this value.
    pub(crate) fn insert(&mut self, id: HudModuleId, module: HudModuleInstance) {
        self.modules.insert(id, module);
        if !self.z_order.contains(&id) {
            self.z_order.push(id);
        }
    }

    // Implements raise to front.
    pub(crate) fn raise_to_front(&mut self, id: HudModuleId) {
        self.z_order.retain(|existing| *existing != id);
        self.z_order.push(id);
    }

    // Sets module enabled.
    pub(crate) fn set_module_enabled(&mut self, id: HudModuleId, enabled: bool) {
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

    // Implements reset module.
    pub(crate) fn reset_module(&mut self, id: HudModuleId) {
        let Some(definition) = HUD_MODULE_DEFINITIONS
            .iter()
            .find(|definition| definition.id == id)
        else {
            return;
        };
        self.modules
            .insert(id, default_hud_module_instance(definition));
        self.raise_to_front(id);
        self.dirty_layout = true;
    }

    // Implements topmost enabled at.
    pub(crate) fn topmost_enabled_at(&self, point: Vec2) -> Option<HudModuleId> {
        self.z_order.iter().rev().copied().find(|id| {
            self.modules.get(id).is_some_and(|module| {
                module.shell.enabled && module.shell.current_rect.contains(point)
            })
        })
    }

    // Returns whether animating.
    pub(crate) fn is_animating(&self) -> bool {
        self.modules
            .values()
            .any(|module| module.shell.is_animating())
    }

    // Implements keyboard capture active.
    pub(crate) fn keyboard_capture_active(&self) -> bool {
        self.message_box.visible || self.task_dialog.visible || self.direct_input_terminal.is_some()
    }

    // Opens message box.
    pub(crate) fn open_message_box(&mut self, target_terminal: TerminalId) {
        self.task_dialog.close();
        self.direct_input_terminal = None;
        self.message_box.reset_for_target(target_terminal);
    }

    // Closes message box.
    pub(crate) fn close_message_box(&mut self) {
        self.message_box.close();
    }

    // Closes message box and discard draft.
    pub(crate) fn close_message_box_and_discard_draft(&mut self) {
        self.message_box.close_and_discard_current();
    }

    // Opens task dialog.
    pub(crate) fn open_task_dialog(&mut self, target_terminal: TerminalId, text: &str) {
        self.close_message_box();
        self.direct_input_terminal = None;
        self.task_dialog.open_with_text(target_terminal, text);
    }

    // Closes task dialog.
    pub(crate) fn close_task_dialog(&mut self) {
        self.task_dialog.close();
    }

    // Opens direct terminal input.
    pub(crate) fn open_direct_terminal_input(&mut self, target_terminal: TerminalId) {
        self.close_message_box();
        self.close_task_dialog();
        self.direct_input_terminal = Some(target_terminal);
    }

    // Closes direct terminal input.
    pub(crate) fn close_direct_terminal_input(&mut self) {
        self.direct_input_terminal = None;
    }

    // Toggles direct terminal input.
    pub(crate) fn toggle_direct_terminal_input(&mut self, target_terminal: TerminalId) -> bool {
        if self.direct_input_terminal == Some(target_terminal) {
            self.close_direct_terminal_input();
            return false;
        }
        self.open_direct_terminal_input(target_terminal);
        true
    }

    // Reconciles direct terminal input.
    pub(crate) fn reconcile_direct_terminal_input(&mut self, active_id: Option<TerminalId>) {
        if self
            .direct_input_terminal
            .is_some_and(|terminal_id| Some(terminal_id) != active_id)
        {
            self.close_direct_terminal_input();
        }
    }

    // Implements layout state.
    pub(crate) fn layout_state(&self) -> HudLayoutState {
        HudLayoutState {
            modules: self.modules.clone(),
            z_order: self.z_order.clone(),
            drag: self.drag,
            dirty_layout: self.dirty_layout,
        }
    }

    // Implements modal state.
    pub(crate) fn modal_state(&self) -> HudModalState {
        HudModalState {
            message_box: self.message_box.clone(),
            task_dialog: self.task_dialog.clone(),
        }
    }

    // Implements input capture state.
    pub(crate) fn input_capture_state(&self) -> HudInputCaptureState {
        HudInputCaptureState {
            direct_input_terminal: self.direct_input_terminal,
        }
    }

    // Implements into resources.
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

    // Builds this value from resources.
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
        }
    }
}

impl HudInputCaptureState {
    // Opens direct terminal input.
    pub(crate) fn open_direct_terminal_input(
        &mut self,
        modals: &mut HudModalState,
        target_terminal: TerminalId,
    ) {
        modals.close_message_box();
        modals.close_task_dialog();
        self.direct_input_terminal = Some(target_terminal);
    }

    // Closes direct terminal input.
    pub(crate) fn close_direct_terminal_input(&mut self) {
        self.direct_input_terminal = None;
    }

    // Toggles direct terminal input.
    pub(crate) fn toggle_direct_terminal_input(
        &mut self,
        modals: &mut HudModalState,
        target_terminal: TerminalId,
    ) -> bool {
        if self.direct_input_terminal == Some(target_terminal) {
            self.close_direct_terminal_input();
            return false;
        }
        self.open_direct_terminal_input(modals, target_terminal);
        true
    }

    // Reconciles direct terminal input.
    pub(crate) fn reconcile_direct_terminal_input(&mut self, active_id: Option<TerminalId>) {
        if self
            .direct_input_terminal
            .is_some_and(|terminal_id| Some(terminal_id) != active_id)
        {
            self.close_direct_terminal_input();
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct AgentDirectory {
    pub(crate) labels: BTreeMap<TerminalId, String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalVisibilityPolicy {
    #[default]
    ShowAll,
    Isolate(TerminalId),
}

#[derive(Resource, Default)]
pub(crate) struct TerminalVisibilityState {
    pub(crate) policy: TerminalVisibilityPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudModuleDefinition {
    pub(crate) id: HudModuleId,
    pub(crate) default_enabled: bool,
    pub(crate) default_rect: HudRect,
}

// Implements docked agent list rect.
pub(crate) fn docked_agent_list_rect(window: &Window) -> HudRect {
    HudRect {
        x: 0.0,
        y: 0.0,
        w: HUD_AGENT_LIST_WIDTH.min(window.width()),
        h: window.height(),
    }
}

pub(crate) const HUD_MODULE_DEFINITIONS: [HudModuleDefinition; 2] = [
    HudModuleDefinition {
        id: HudModuleId::DebugToolbar,
        default_enabled: true,
        default_rect: HudRect {
            x: 24.0,
            y: 24.0,
            w: 920.0,
            h: 64.0,
        },
    },
    HudModuleDefinition {
        id: HudModuleId::AgentList,
        default_enabled: true,
        default_rect: HudRect {
            x: 0.0,
            y: 0.0,
            w: HUD_AGENT_LIST_WIDTH,
            h: 720.0,
        },
    },
];

// Implements default HUD module instance.
pub(crate) fn default_hud_module_instance(definition: &HudModuleDefinition) -> HudModuleInstance {
    let shell = HudModuleShell {
        enabled: definition.default_enabled,
        target_rect: definition.default_rect,
        current_rect: definition.default_rect,
        target_alpha: if definition.default_enabled { 1.0 } else { 0.0 },
        current_alpha: if definition.default_enabled { 1.0 } else { 0.0 },
    };
    let model = match definition.id {
        HudModuleId::DebugToolbar => HudModuleModel::DebugToolbar(DebugToolbarState),
        HudModuleId::AgentList => HudModuleModel::AgentList(AgentListState::default()),
    };
    HudModuleInstance { shell, model }
}
