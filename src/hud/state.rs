use crate::{agents::AgentId, terminals::TerminalId};

use super::widgets::{HudWidgetDefinition, HudWidgetKey, HUD_WIDGET_DEFINITIONS};
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(crate) const HUD_TITLEBAR_HEIGHT: f32 = 28.0;
pub(crate) const HUD_MODULE_PADDING: f32 = 10.0;
pub(crate) const HUD_ROW_HEIGHT: f32 = 28.0;
pub(crate) const HUD_BUTTON_HEIGHT: f32 = 28.0;
pub(crate) const HUD_BUTTON_GAP: f32 = 8.0;
pub(crate) const HUD_BUTTON_MIN_WIDTH: f32 = 72.0;
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HudModuleShell {
    pub(crate) enabled: bool,
    pub(crate) target_rect: HudRect,
    pub(crate) current_rect: HudRect,
    pub(crate) target_alpha: f32,
    pub(crate) current_alpha: f32,
}

impl HudModuleShell {
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

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AgentListUiState {
    pub(crate) scroll_offset: f32,
    pub(crate) hovered_agent: Option<AgentId>,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct ConversationListUiState {
    pub(crate) scroll_offset: f32,
    pub(crate) hovered_agent: Option<AgentId>,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct ThreadPaneUiState;

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct DebugToolbarUiState;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HudModuleInstance {
    pub(crate) shell: HudModuleShell,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudDragState {
    pub(crate) module_id: HudWidgetKey,
    pub(crate) grab_offset: Vec2,
}

#[derive(Resource, Default)]
pub(crate) struct HudLayoutState {
    pub(crate) modules: BTreeMap<HudWidgetKey, HudModuleInstance>,
    pub(crate) z_order: Vec<HudWidgetKey>,
    pub(crate) drag: Option<HudDragState>,
    pub(crate) dirty_layout: bool,
}

impl HudLayoutState {
    /// Returns the retained module instance for a given module id.
    ///
    /// This is the read-only accessor used by most HUD systems.
    pub(crate) fn get(&self, id: HudWidgetKey) -> Option<&HudModuleInstance> {
        self.modules.get(&id)
    }

    /// Returns mutable access to one retained module instance.
    ///
    /// Systems that mutate shell state go through this helper.
    pub(crate) fn get_mut(&mut self, id: HudWidgetKey) -> Option<&mut HudModuleInstance> {
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
    pub(crate) fn iter_z_order_front_to_back(&self) -> impl Iterator<Item = HudWidgetKey> + '_ {
        self.z_order.iter().rev().copied()
    }

    /// Inserts or replaces a module instance and ensures it exists in z-order.
    ///
    /// First insert appends the module at the back; replacing an existing module preserves its current
    /// z position.
    pub(crate) fn insert(&mut self, id: HudWidgetKey, module: HudModuleInstance) {
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

    /// Enables or disables a module shell and updates its target alpha accordingly.
    ///
    /// The current alpha is left alone so animation can fade toward the new target state.
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
        composer: &mut crate::ui::ComposerState,
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
        composer: &mut crate::ui::ComposerState,
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

#[derive(Resource, Default)]
pub(crate) struct TerminalVisibilityState {
    pub(crate) policy: TerminalVisibilityPolicy,
}

/// Returns the fixed docked rectangle used by the agent-list module.
///
/// The agent list is pinned to the left edge and spans the full window height, with width clamped by
/// the current window width.
pub(crate) fn docked_agent_list_rect(window: &Window) -> HudRect {
    HudRect {
        x: 0.0,
        y: 0.0,
        w: HUD_AGENT_LIST_WIDTH.min(window.width()),
        h: window.height(),
    }
}

/// Builds the default retained instance for one HUD module definition.
///
/// Only generic shell/layout state lives here; widget-local retained state is stored separately in
/// per-widget resources.
pub(crate) fn default_hud_module_instance(definition: &HudWidgetDefinition) -> HudModuleInstance {
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
mod tests;
