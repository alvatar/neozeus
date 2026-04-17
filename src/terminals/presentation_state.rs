use super::registry::TerminalId;
use bevy::prelude::*;
use std::collections::HashMap;

/// Derived per-terminal camera/view projection keyed off the projected active terminal.
#[derive(Resource)]
pub(crate) struct TerminalViewState {
    pub(crate) distance: f32,
    pub(crate) offset: Vec2,
    offsets_by_terminal: HashMap<TerminalId, Vec2>,
}

impl Default for TerminalViewState {
    /// Creates the shared terminal view state with neutral zoom and zero offset.
    fn default() -> Self {
        Self {
            distance: 10.0,
            offset: Vec2::ZERO,
            offsets_by_terminal: HashMap::new(),
        }
    }
}

impl TerminalViewState {
    /// Switches the shared view offset to the per-terminal offset remembered for the newly focused
    /// terminal.
    ///
    /// Unfocused state falls back to zero offset.
    pub(crate) fn focus_terminal(&mut self, active_id: Option<TerminalId>) {
        self.offset = active_id
            .map(|id| {
                self.offsets_by_terminal
                    .get(&id)
                    .copied()
                    .unwrap_or(Vec2::ZERO)
            })
            .unwrap_or(Vec2::ZERO);
    }

    /// Applies a pan delta to the current shared offset and persists it for the active terminal if
    /// one exists.
    pub(crate) fn apply_offset_delta(&mut self, active_id: Option<TerminalId>, delta: Vec2) {
        self.offset += delta;
        if let Some(id) = active_id {
            self.offsets_by_terminal.insert(id, self.offset);
        }
    }

    /// Drops any remembered per-terminal pan offset for a terminal that is being removed.
    pub(crate) fn forget_terminal(&mut self, terminal_id: TerminalId) {
        self.offsets_by_terminal.remove(&terminal_id);
    }
}

#[derive(Resource, Default)]
pub(crate) struct TerminalPointerState {
    pub(crate) scroll_drag_remainder_px: f32,
    pub(crate) wheel_scroll_remainder_lines: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalDisplayMode {
    #[default]
    Smooth,
    PixelPerfect,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    pub(crate) desired_texture_state: TerminalTextureState,
    pub(crate) display_mode: TerminalDisplayMode,
    pub(crate) uploaded_revision: u64,
    pub(crate) uploaded_active_override_revision: Option<u64>,
    pub(crate) uploaded_text_selection_revision: Option<u64>,
    pub(crate) uploaded_surface: Option<super::types::TerminalSurface>,
    pub(crate) panel_entity: Entity,
    pub(crate) frame_entity: Entity,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalPresentationPendingState {
    pub(crate) startup_bootstrap: bool,
    pub(crate) awaiting_first_frame: bool,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalPresentationStore {
    terminals: HashMap<TerminalId, PresentedTerminal>,
    pending: HashMap<TerminalId, TerminalPresentationPendingState>,
}

impl TerminalPresentationStore {
    /// Inserts or replaces the presentation-store record for one terminal id.
    pub(crate) fn register(&mut self, id: TerminalId, terminal: PresentedTerminal) {
        self.terminals.insert(id, terminal);
    }

    /// Returns the retained presentation record for one terminal id.
    pub(crate) fn get(&self, id: TerminalId) -> Option<&PresentedTerminal> {
        self.terminals.get(&id)
    }

    /// Returns mutable access to one terminal's presentation-store record.
    pub(crate) fn get_mut(&mut self, id: TerminalId) -> Option<&mut PresentedTerminal> {
        self.terminals.get_mut(&id)
    }

    /// Removes and returns the presentation-store record for one terminal id.
    pub(crate) fn remove(&mut self, id: TerminalId) -> Option<PresentedTerminal> {
        self.pending.remove(&id);
        self.terminals.remove(&id)
    }

    /// Returns all terminal ids currently tracked by the presentation store.
    pub(crate) fn terminal_ids(&self) -> Vec<TerminalId> {
        self.terminals.keys().copied().collect()
    }

    fn pending_state_mut(&mut self, id: TerminalId) -> &mut TerminalPresentationPendingState {
        self.pending.entry(id).or_default()
    }

    fn cleanup_pending_state(&mut self, id: TerminalId) {
        if self.pending.get(&id).is_some_and(|state| *state == TerminalPresentationPendingState::default()) {
            self.pending.remove(&id);
        }
    }

    /// Marks a terminal as pending startup-bootstrap reveal until its first ready-for-capture frame lands.
    pub(crate) fn mark_startup_bootstrap_pending(&mut self, id: TerminalId) {
        self.pending_state_mut(id).startup_bootstrap = true;
    }

    /// Marks a terminal as waiting for its first usable frame outside startup bootstrap.
    pub(crate) fn mark_awaiting_first_frame(&mut self, id: TerminalId) {
        self.pending_state_mut(id).awaiting_first_frame = true;
    }

    /// Clears the startup-bootstrap pending reason for one terminal.
    pub(crate) fn resolve_startup_bootstrap_pending(&mut self, id: TerminalId) {
        if let Some(state) = self.pending.get_mut(&id) {
            state.startup_bootstrap = false;
        }
        self.cleanup_pending_state(id);
    }

    /// Clears the ordinary awaiting-first-frame pending reason for one terminal.
    pub(crate) fn resolve_awaiting_first_frame(&mut self, id: TerminalId) {
        if let Some(state) = self.pending.get_mut(&id) {
            state.awaiting_first_frame = false;
        }
        self.cleanup_pending_state(id);
    }

    /// Clears all pending presentation reasons for one terminal once it is ready-for-capture.
    pub(crate) fn resolve_pending_presentation(&mut self, id: TerminalId) {
        self.resolve_startup_bootstrap_pending(id);
        self.resolve_awaiting_first_frame(id);
    }

    /// Returns the explicit pending state for one terminal.
    pub(crate) fn pending_state(&self, id: TerminalId) -> TerminalPresentationPendingState {
        self.pending.get(&id).copied().unwrap_or_default()
    }

    /// Returns whether one terminal is still pending due to startup bootstrap.
    pub(crate) fn is_startup_bootstrap_pending(&self, id: TerminalId) -> bool {
        self.pending_state(id).startup_bootstrap
    }

    /// Returns whether one terminal is still pending because it awaits its first usable frame.
    pub(crate) fn is_awaiting_first_frame(&self, id: TerminalId) -> bool {
        self.pending_state(id).awaiting_first_frame
    }

    /// Returns the uploaded texture state of the currently active terminal, if any.
    pub(crate) fn active_texture_state(
        &self,
        active_id: Option<TerminalId>,
    ) -> Option<&TerminalTextureState> {
        self.terminals
            .get(&active_id?)
            .map(|terminal| &terminal.texture_state)
    }

    /// Returns the display mode of the currently active terminal, if any.
    pub(crate) fn active_display_mode(
        &self,
        active_id: Option<TerminalId>,
    ) -> Option<TerminalDisplayMode> {
        self.terminals
            .get(&active_id?)
            .map(|terminal| terminal.display_mode)
    }
}
