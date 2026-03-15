use crate::terminals::TerminalId;
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(crate) const HUD_TITLEBAR_HEIGHT: f32 = 28.0;
pub(crate) const HUD_MODULE_PADDING: f32 = 10.0;
pub(crate) const HUD_ROW_HEIGHT: f32 = 28.0;
pub(crate) const HUD_BUTTON_HEIGHT: f32 = 28.0;
pub(crate) const HUD_BUTTON_GAP: f32 = 8.0;
pub(crate) const HUD_BUTTON_MIN_WIDTH: f32 = 72.0;
pub(crate) const HUD_ANIMATION_EPSILON: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HudModuleId {
    DebugToolbar,
    AgentList,
}

impl HudModuleId {
    pub(crate) const fn number(self) -> u8 {
        match self {
            Self::DebugToolbar => 0,
            Self::AgentList => 1,
        }
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::DebugToolbar => "Debug Toolbar",
            Self::AgentList => "Agent List",
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
    pub(crate) z: i32,
    pub(crate) target_rect: HudRect,
    pub(crate) current_rect: HudRect,
    pub(crate) target_alpha: f32,
    pub(crate) current_alpha: f32,
}

impl HudModuleShell {
    pub(crate) fn titlebar_rect(&self) -> HudRect {
        HudRect {
            x: self.current_rect.x,
            y: self.current_rect.y,
            w: self.current_rect.w,
            h: HUD_TITLEBAR_HEIGHT.min(self.current_rect.h),
        }
    }

    pub(crate) fn content_rect(&self) -> HudRect {
        let title_h = HUD_TITLEBAR_HEIGHT.min(self.current_rect.h);
        HudRect {
            x: self.current_rect.x,
            y: self.current_rect.y + title_h,
            w: self.current_rect.w,
            h: (self.current_rect.h - title_h).max(0.0),
        }
    }

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
pub(crate) struct HudState {
    pub(crate) modules: BTreeMap<HudModuleId, HudModuleInstance>,
    pub(crate) z_order: Vec<HudModuleId>,
    pub(crate) drag: Option<HudDragState>,
    pub(crate) dirty_layout: bool,
}

impl HudState {
    pub(crate) fn get(&self, id: HudModuleId) -> Option<&HudModuleInstance> {
        self.modules.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: HudModuleId) -> Option<&mut HudModuleInstance> {
        self.modules.get_mut(&id)
    }

    pub(crate) fn iter_z_order(&self) -> impl Iterator<Item = HudModuleId> + '_ {
        self.z_order.iter().copied()
    }

    pub(crate) fn iter_z_order_front_to_back(&self) -> impl Iterator<Item = HudModuleId> + '_ {
        self.z_order.iter().rev().copied()
    }

    pub(crate) fn insert(&mut self, id: HudModuleId, module: HudModuleInstance) {
        self.modules.insert(id, module);
        if !self.z_order.contains(&id) {
            self.z_order.push(id);
        }
        self.sync_z_order();
    }

    pub(crate) fn raise_to_front(&mut self, id: HudModuleId) {
        self.z_order.retain(|existing| *existing != id);
        self.z_order.push(id);
        self.sync_z_order();
    }

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

    pub(crate) fn topmost_enabled_at(&self, point: Vec2) -> Option<HudModuleId> {
        self.iter_z_order_front_to_back().find(|id| {
            self.modules.get(id).is_some_and(|module| {
                module.shell.enabled && module.shell.current_rect.contains(point)
            })
        })
    }

    pub(crate) fn sync_z_order(&mut self) {
        for (index, id) in self.z_order.iter().enumerate() {
            if let Some(module) = self.modules.get_mut(id) {
                module.shell.z = index as i32;
            }
        }
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.modules
            .values()
            .any(|module| module.shell.enabled && module.shell.is_animating())
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
            x: 24.0,
            y: 104.0,
            w: 300.0,
            h: 420.0,
        },
    },
];

pub(crate) fn default_hud_module_instance(definition: &HudModuleDefinition) -> HudModuleInstance {
    let shell = HudModuleShell {
        enabled: definition.default_enabled,
        z: 0,
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
