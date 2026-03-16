use crate::terminals::TerminalId;
use bevy::prelude::*;
use std::collections::HashMap;

#[derive(Resource)]
pub(crate) struct TerminalViewState {
    pub(crate) distance: f32,
    pub(crate) offset: Vec2,
}

impl Default for TerminalViewState {
    fn default() -> Self {
        Self {
            distance: 10.0,
            offset: Vec2::ZERO,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct TerminalPointerState {
    pub(crate) scroll_drag_remainder_px: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalDisplayMode {
    #[default]
    Smooth,
    PixelPerfect,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TerminalTextureState {
    pub(crate) texture_size: UVec2,
    pub(crate) cell_size: UVec2,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalPanel {
    pub(crate) id: TerminalId,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalPanelFrame {
    pub(crate) id: TerminalId,
}

#[derive(Component)]
pub(crate) struct TerminalPanelSprite;

#[derive(Component)]
pub(crate) struct TerminalCameraMarker;

#[derive(Component)]
pub(crate) struct TerminalHudSurfaceMarker;

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct TerminalPresentation {
    pub(crate) home_position: Vec2,
    pub(crate) current_position: Vec2,
    pub(crate) target_position: Vec2,
    pub(crate) current_size: Vec2,
    pub(crate) target_size: Vec2,
    pub(crate) current_alpha: f32,
    pub(crate) target_alpha: f32,
    pub(crate) current_z: f32,
    pub(crate) target_z: f32,
}

pub(crate) struct PresentedTerminal {
    pub(crate) image: Handle<Image>,
    pub(crate) texture_state: TerminalTextureState,
    pub(crate) display_mode: TerminalDisplayMode,
    pub(crate) uploaded_revision: u64,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalPresentationStore {
    terminals: HashMap<TerminalId, PresentedTerminal>,
}

impl TerminalPresentationStore {
    pub(crate) fn register(&mut self, id: TerminalId, terminal: PresentedTerminal) {
        self.terminals.insert(id, terminal);
    }

    pub(crate) fn get(&self, id: TerminalId) -> Option<&PresentedTerminal> {
        self.terminals.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: TerminalId) -> Option<&mut PresentedTerminal> {
        self.terminals.get_mut(&id)
    }

    pub(crate) fn remove(&mut self, id: TerminalId) -> Option<PresentedTerminal> {
        self.terminals.remove(&id)
    }

    pub(crate) fn active_texture_state(
        &self,
        active_id: Option<TerminalId>,
    ) -> Option<&TerminalTextureState> {
        self.terminals
            .get(&active_id?)
            .map(|terminal| &terminal.texture_state)
    }

    pub(crate) fn active_display_mode(
        &self,
        active_id: Option<TerminalId>,
    ) -> Option<TerminalDisplayMode> {
        self.terminals
            .get(&active_id?)
            .map(|terminal| terminal.display_mode)
    }

    pub(crate) fn toggle_active_display_mode(&mut self, active_id: Option<TerminalId>) {
        let Some(terminal) = active_id.and_then(|id| self.terminals.get_mut(&id)) else {
            return;
        };
        terminal.display_mode = match terminal.display_mode {
            TerminalDisplayMode::Smooth => TerminalDisplayMode::PixelPerfect,
            TerminalDisplayMode::PixelPerfect => TerminalDisplayMode::Smooth,
        };
    }
}
