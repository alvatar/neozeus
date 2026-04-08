use super::{
    bridge::TerminalBridge,
    debug::append_debug_log,
    types::{TerminalDamage, TerminalSnapshot},
};
use bevy::{
    prelude::{MessageWriter, ResMut, Resource},
    window::RequestRedraw,
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TerminalId(pub(crate) u64);

pub(crate) struct ManagedTerminal {
    pub(crate) bridge: TerminalBridge,
    pub(crate) session_name: String,
    pub(crate) snapshot: TerminalSnapshot,
    pub(crate) pending_damage: Option<TerminalDamage>,
    pub(crate) surface_revision: u64,
}

#[derive(Resource, Default, Clone)]
pub(crate) struct TerminalFocusState {
    active_id: Option<TerminalId>,
    focus_order: Vec<TerminalId>,
}

impl TerminalFocusState {
    /// Makes one terminal active and moves it to the back of focus order.
    ///
    /// Non-existent terminals are ignored, and refocusing the already-active frontmost terminal is a
    /// no-op.
    pub(crate) fn focus_terminal(&mut self, terminal_manager: &TerminalManager, id: TerminalId) {
        if !terminal_manager.contains_terminal(id) {
            return;
        }
        if self.active_id == Some(id) && self.focus_order.last() == Some(&id) {
            return;
        }
        self.active_id = Some(id);
        self.focus_order.retain(|existing| *existing != id);
        self.focus_order.push(id);
        append_debug_log(format!("focused terminal {}", id.0));
    }

    /// Returns the currently active terminal id, if any.
    pub(crate) fn active_id(&self) -> Option<TerminalId> {
        self.active_id
    }

    /// Clears the active terminal slot and returns the id that was previously active.
    pub(crate) fn clear_active_terminal(&mut self) -> Option<TerminalId> {
        let cleared = self.active_id.take()?;
        append_debug_log(format!("cleared active terminal {}", cleared.0));
        Some(cleared)
    }

    /// Implements active bridge.
    pub(crate) fn active_bridge<'a>(
        &self,
        terminal_manager: &'a TerminalManager,
    ) -> Option<&'a TerminalBridge> {
        self.active_id
            .and_then(|id| terminal_manager.get(id).map(|terminal| &terminal.bridge))
    }

    /// Test helper that exposes the focus-order list.
    #[cfg(test)]
    fn focus_order(&self) -> &[TerminalId] {
        &self.focus_order
    }

    /// Removes a terminal from focus ordering and clears active focus if it was focused.
    pub(crate) fn forget_terminal(&mut self, id: TerminalId) {
        self.focus_order.retain(|existing| *existing != id);
        if self.active_id == Some(id) {
            self.active_id = None;
        }
    }
}

#[derive(Resource)]
pub(crate) struct TerminalManager {
    next_id: u64,
    creation_order: Vec<TerminalId>,
    terminals: HashMap<TerminalId, ManagedTerminal>,
    #[cfg(test)]
    test_focus_state: TerminalFocusState,
}

impl Default for TerminalManager {
    /// Creates an empty terminal registry with ids starting at 1.
    fn default() -> Self {
        Self {
            next_id: 1,
            creation_order: Vec::new(),
            terminals: HashMap::new(),
            #[cfg(test)]
            test_focus_state: TerminalFocusState::default(),
        }
    }
}

impl TerminalManager {
    /// Inserts a new managed terminal record and assigns it the next terminal id.
    fn insert_terminal(&mut self, bridge: TerminalBridge, session_name: String) -> TerminalId {
        let id = TerminalId(self.next_id);
        self.next_id += 1;
        self.terminals.insert(
            id,
            ManagedTerminal {
                bridge,
                session_name,
                snapshot: TerminalSnapshot::default(),
                pending_damage: None,
                surface_revision: 0,
            },
        );
        self.creation_order.push(id);
        id
    }

    /// Returns whether the registry currently contains the given terminal id.
    pub(crate) fn contains_terminal(&self, id: TerminalId) -> bool {
        self.terminals.contains_key(&id)
    }

    /// Creates a managed terminal record for an existing session name without changing focus.
    fn create_terminal_without_focus_with_session(
        &mut self,
        bridge: TerminalBridge,
        session_name: String,
    ) -> TerminalId {
        self.insert_terminal(bridge, session_name)
    }

    /// Creates an unfocused terminal and returns both its id and its creation-order slot index.
    pub(super) fn create_terminal_without_focus_with_slot_and_session(
        &mut self,
        bridge: TerminalBridge,
        session_name: String,
    ) -> (TerminalId, usize) {
        let id = self.create_terminal_without_focus_with_session(bridge, session_name);
        let slot = self.creation_order.len().saturating_sub(1);
        debug_assert_eq!(self.creation_order.get(slot), Some(&id));
        (id, slot)
    }

    /// Returns terminal ids in stable creation order.
    pub(crate) fn terminal_ids(&self) -> &[TerminalId] {
        &self.creation_order
    }

    /// Returns the managed-terminal record for one id.
    pub(crate) fn get(&self, id: TerminalId) -> Option<&ManagedTerminal> {
        self.terminals.get(&id)
    }

    /// Returns mutable access to one managed-terminal record.
    #[cfg(test)]
    pub(crate) fn get_mut(&mut self, id: TerminalId) -> Option<&mut ManagedTerminal> {
        self.terminals.get_mut(&id)
    }

    /// Removes a managed terminal from both the id map and creation-order list.
    pub(crate) fn remove_terminal(&mut self, id: TerminalId) -> Option<ManagedTerminal> {
        let removed = self.terminals.remove(&id)?;
        self.creation_order.retain(|existing| *existing != id);
        #[cfg(test)]
        self.test_focus_state.forget_terminal(id);
        Some(removed)
    }

    /// Iterates over all managed terminals as `(id, terminal)` pairs.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (TerminalId, &ManagedTerminal)> {
        self.terminals.iter().map(|(id, terminal)| (*id, terminal))
    }

    /// Iterates mutably over all managed terminals as `(id, terminal)` pairs.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (TerminalId, &mut ManagedTerminal)> {
        self.terminals
            .iter_mut()
            .map(|(id, terminal)| (*id, terminal))
    }

    /// Test helper that creates a terminal with an auto-generated session name and focuses it.
    #[cfg(test)]
    pub(crate) fn create_terminal(&mut self, bridge: TerminalBridge) -> TerminalId {
        let session_name = format!("terminal-{}", self.next_id);
        let id = self.insert_terminal(bridge, session_name);
        let snapshot = self.clone_focus_state();
        let mut focus_state = snapshot;
        focus_state.focus_terminal(self, id);
        self.test_focus_state = focus_state;
        id
    }

    /// Test helper that creates a terminal for an explicit session name and focuses it.
    #[cfg(test)]
    pub(crate) fn create_terminal_with_session(
        &mut self,
        bridge: TerminalBridge,
        session_name: String,
    ) -> TerminalId {
        let id = self.insert_terminal(bridge, session_name);
        let snapshot = self.clone_focus_state();
        let mut focus_state = snapshot;
        focus_state.focus_terminal(self, id);
        self.test_focus_state = focus_state;
        id
    }

    /// Test helper that creates a terminal with an auto-generated session name without changing
    /// focus.
    #[cfg(test)]
    pub(crate) fn create_terminal_without_focus(&mut self, bridge: TerminalBridge) -> TerminalId {
        let session_name = format!("terminal-{}", self.next_id);
        self.insert_terminal(bridge, session_name)
    }

    /// Test helper that focuses a terminal using the embedded compatibility focus state.
    #[cfg(test)]
    pub(crate) fn focus_terminal(&mut self, id: TerminalId) {
        let snapshot = self.clone_focus_state();
        let mut focus_state = snapshot;
        focus_state.focus_terminal(self, id);
        self.test_focus_state = focus_state;
    }

    /// Test helper that clones the embedded compatibility focus state.
    #[cfg(test)]
    pub(crate) fn clone_focus_state(&self) -> TerminalFocusState {
        self.test_focus_state.clone()
    }

    /// Test helper that overwrites the embedded compatibility focus state.
    #[cfg(test)]
    pub(crate) fn replace_test_focus_state(&mut self, focus_state: &TerminalFocusState) {
        self.test_focus_state = focus_state.clone();
    }
}

/// Drains each terminal bridge mailbox and folds the newest frame/status updates into the retained
/// terminal registry state.
///
/// Dropped intermediate frames upgrade damage to `Full` so renderers do not miss changes.
pub(crate) fn poll_terminal_snapshots(
    mut terminal_manager: ResMut<TerminalManager>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let mut applied_any_update = false;
    for (_, terminal) in terminal_manager.iter_mut() {
        let (latest_frame, latest_status, dropped_frames) = terminal.bridge.drain_updates();
        terminal.bridge.note_dropped_updates(dropped_frames);

        if let Some(frame) = latest_frame {
            terminal.snapshot.runtime = frame.runtime;
            terminal.snapshot.surface = Some(frame.surface);
            terminal.surface_revision += 1;
            terminal.pending_damage = Some(if dropped_frames > 0 {
                TerminalDamage::Full
            } else {
                frame.damage
            });
            terminal.bridge.note_snapshot_applied();
            applied_any_update = true;
        }

        if let Some((runtime, surface)) = latest_status {
            terminal.snapshot.runtime = runtime;
            if let Some(surface) = surface {
                terminal.snapshot.surface = Some(surface);
                terminal.surface_revision += 1;
                terminal.pending_damage = Some(TerminalDamage::Full);
            }
            terminal.bridge.note_snapshot_applied();
            applied_any_update = true;
        }
    }
    if applied_any_update {
        redraws.write(RequestRedraw);
    }
}

#[cfg(test)]
mod tests;
