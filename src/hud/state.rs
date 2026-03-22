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

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HudMessageBoxState {
    pub(crate) visible: bool,
    pub(crate) target_terminal: Option<TerminalId>,
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) preferred_column: Option<usize>,
    pub(crate) kill_buffer: String,
}

impl HudMessageBoxState {
    pub(crate) fn reset_for_target(&mut self, target_terminal: TerminalId) {
        self.visible = true;
        self.target_terminal = Some(target_terminal);
        self.text.clear();
        self.cursor = 0;
        self.preferred_column = None;
        self.kill_buffer.clear();
    }

    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return false;
        }
        self.text.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
        self.preferred_column = None;
        true
    }

    pub(crate) fn insert_newline(&mut self) -> bool {
        self.insert_text("\n")
    }

    pub(crate) fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.cursor = next;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_line_start(&mut self) -> bool {
        let (line_start, _) = current_line_bounds(&self.text, self.cursor);
        if self.cursor == line_start {
            return false;
        }
        self.cursor = line_start;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_line_end(&mut self) -> bool {
        let (_, line_end) = current_line_bounds(&self.text, self.cursor);
        if self.cursor == line_end {
            return false;
        }
        self.cursor = line_end;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_up(&mut self) -> bool {
        let (line_start, _) = current_line_bounds(&self.text, self.cursor);
        if line_start == 0 {
            return false;
        }
        let target_column = self
            .preferred_column
            .unwrap_or_else(|| current_line_column_chars(&self.text, self.cursor));
        let previous_line_end = line_start - 1;
        let previous_line_start = self.text[..previous_line_end]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        self.cursor = advance_by_chars(
            &self.text,
            previous_line_start,
            previous_line_end,
            target_column,
        );
        self.preferred_column = Some(target_column);
        true
    }

    pub(crate) fn move_down(&mut self) -> bool {
        let (_, line_end) = current_line_bounds(&self.text, self.cursor);
        if line_end >= self.text.len() {
            return false;
        }
        let target_column = self
            .preferred_column
            .unwrap_or_else(|| current_line_column_chars(&self.text, self.cursor));
        let next_line_start = line_end + 1;
        let next_line_end = self.text[next_line_start..]
            .find('\n')
            .map(|offset| next_line_start + offset)
            .unwrap_or(self.text.len());
        self.cursor = advance_by_chars(&self.text, next_line_start, next_line_end, target_column);
        self.preferred_column = Some(target_column);
        true
    }

    pub(crate) fn move_word_backward(&mut self) -> bool {
        let original = self.cursor;
        let mut cursor = self.cursor;
        while let Some((previous, ch)) = previous_char(&self.text, cursor) {
            if is_word_char(ch) {
                break;
            }
            cursor = previous;
        }
        while let Some((previous, ch)) = previous_char(&self.text, cursor) {
            if !is_word_char(ch) {
                break;
            }
            cursor = previous;
        }
        if cursor == original {
            return false;
        }
        self.cursor = cursor;
        self.preferred_column = None;
        true
    }

    pub(crate) fn move_word_forward(&mut self) -> bool {
        let original = self.cursor;
        let mut cursor = self.cursor;
        while let Some((next, ch)) = next_char(&self.text, cursor) {
            if is_word_char(ch) {
                break;
            }
            cursor = next;
        }
        while let Some((next, ch)) = next_char(&self.text, cursor) {
            if !is_word_char(ch) {
                break;
            }
            cursor = next;
        }
        if cursor == original {
            return false;
        }
        self.cursor = cursor;
        self.preferred_column = None;
        true
    }

    pub(crate) fn delete_backward_char(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
        self.preferred_column = None;
        true
    }

    pub(crate) fn delete_forward_char(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return false;
        };
        self.text.drain(self.cursor..next);
        self.preferred_column = None;
        true
    }

    pub(crate) fn kill_to_end_of_line(&mut self) -> bool {
        let (_, line_end) = current_line_bounds(&self.text, self.cursor);
        let kill_end = if self.cursor == line_end && line_end < self.text.len() {
            line_end + 1
        } else {
            line_end
        };
        if kill_end <= self.cursor {
            return false;
        }
        self.kill_buffer = self.text[self.cursor..kill_end].to_owned();
        self.text.drain(self.cursor..kill_end);
        self.preferred_column = None;
        true
    }

    pub(crate) fn yank(&mut self) -> bool {
        if self.kill_buffer.is_empty() {
            return false;
        }
        let payload = self.kill_buffer.clone();
        self.text.insert_str(self.cursor, &payload);
        self.cursor += payload.len();
        self.preferred_column = None;
        true
    }

    pub(crate) fn cursor_line_and_column(&self) -> (usize, usize) {
        (
            self.text[..self.cursor]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
            current_line_column_chars(&self.text, self.cursor),
        )
    }
}

fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last().map(|(index, _)| index)
}

fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

fn previous_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last()
}

fn next_char(text: &str, cursor: usize) -> Option<(usize, char)> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| (cursor + ch.len_utf8(), ch))
}

fn current_line_bounds(text: &str, cursor: usize) -> (usize, usize) {
    let line_start = text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let line_end = text[cursor..]
        .find('\n')
        .map(|offset| cursor + offset)
        .unwrap_or(text.len());
    (line_start, line_end)
}

fn current_line_column_chars(text: &str, cursor: usize) -> usize {
    let (line_start, _) = current_line_bounds(text, cursor);
    text[line_start..cursor].chars().count()
}

fn advance_by_chars(text: &str, start: usize, end: usize, count: usize) -> usize {
    let mut cursor = start;
    for ch in text[start..end].chars().take(count) {
        cursor += ch.len_utf8();
    }
    if count >= text[start..end].chars().count() {
        end
    } else {
        cursor
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[derive(Resource, Default)]
pub(crate) struct HudState {
    pub(crate) modules: BTreeMap<HudModuleId, HudModuleInstance>,
    pub(crate) z_order: Vec<HudModuleId>,
    pub(crate) drag: Option<HudDragState>,
    pub(crate) dirty_layout: bool,
    pub(crate) message_box: HudMessageBoxState,
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
    }

    pub(crate) fn raise_to_front(&mut self, id: HudModuleId) {
        self.z_order.retain(|existing| *existing != id);
        self.z_order.push(id);
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

    pub(crate) fn topmost_enabled_at(&self, point: Vec2) -> Option<HudModuleId> {
        self.iter_z_order_front_to_back().find(|id| {
            self.modules.get(id).is_some_and(|module| {
                module.shell.enabled && module.shell.current_rect.contains(point)
            })
        })
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.modules
            .values()
            .any(|module| module.shell.enabled && module.shell.is_animating())
    }

    pub(crate) fn open_message_box(&mut self, target_terminal: TerminalId) {
        self.message_box.reset_for_target(target_terminal);
    }

    pub(crate) fn close_message_box(&mut self) {
        self.message_box = HudMessageBoxState::default();
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
