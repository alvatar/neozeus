use crate::terminals::{
    append_debug_log, TerminalBridge, TerminalDamage, TerminalDebugStats, TerminalSnapshot,
};
use bevy::prelude::{ResMut, Resource};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TerminalId(pub(crate) u64);

pub(crate) struct ManagedTerminal {
    pub(crate) bridge: TerminalBridge,
    pub(crate) session_name: String,
    pub(crate) snapshot: TerminalSnapshot,
    pub(crate) pending_damage: Option<TerminalDamage>,
    pub(crate) surface_revision: u64,
    pub(crate) requested_dimensions: Option<crate::terminals::TerminalDimensions>,
}

#[derive(Resource, Default, Clone)]
pub(crate) struct TerminalFocusState {
    active_id: Option<TerminalId>,
    focus_order: Vec<TerminalId>,
}

impl TerminalFocusState {
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

    pub(crate) fn active_id(&self) -> Option<TerminalId> {
        self.active_id
    }

    pub(crate) fn clear_active_terminal(&mut self) -> Option<TerminalId> {
        let cleared = self.active_id.take()?;
        append_debug_log(format!("cleared active terminal {}", cleared.0));
        Some(cleared)
    }

    pub(crate) fn active_bridge<'a>(
        &self,
        terminal_manager: &'a TerminalManager,
    ) -> Option<&'a TerminalBridge> {
        self.active_id
            .and_then(|id| terminal_manager.get(id).map(|terminal| &terminal.bridge))
    }

    pub(crate) fn active_snapshot<'a>(
        &self,
        terminal_manager: &'a TerminalManager,
    ) -> Option<&'a TerminalSnapshot> {
        self.active_id
            .and_then(|id| terminal_manager.get(id).map(|terminal| &terminal.snapshot))
    }

    pub(crate) fn active_debug_stats(
        &self,
        terminal_manager: &TerminalManager,
    ) -> TerminalDebugStats {
        self.active_bridge(terminal_manager)
            .map(TerminalBridge::debug_stats_snapshot)
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn focus_order(&self) -> &[TerminalId] {
        &self.focus_order
    }

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
                requested_dimensions: None,
            },
        );
        self.creation_order.push(id);
        id
    }

    pub(crate) fn contains_terminal(&self, id: TerminalId) -> bool {
        self.terminals.contains_key(&id)
    }

    pub(crate) fn create_terminal_without_focus_with_session(
        &mut self,
        bridge: TerminalBridge,
        session_name: String,
    ) -> TerminalId {
        self.insert_terminal(bridge, session_name)
    }

    pub(crate) fn create_terminal_without_focus_with_slot_and_session(
        &mut self,
        bridge: TerminalBridge,
        session_name: String,
    ) -> (TerminalId, usize) {
        let id = self.create_terminal_without_focus_with_session(bridge, session_name);
        let slot = self.creation_order.len().saturating_sub(1);
        debug_assert_eq!(self.creation_order.get(slot), Some(&id));
        (id, slot)
    }

    pub(crate) fn terminal_ids(&self) -> &[TerminalId] {
        &self.creation_order
    }

    pub(crate) fn get(&self, id: TerminalId) -> Option<&ManagedTerminal> {
        self.terminals.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: TerminalId) -> Option<&mut ManagedTerminal> {
        self.terminals.get_mut(&id)
    }

    pub(crate) fn remove_terminal(&mut self, id: TerminalId) -> Option<ManagedTerminal> {
        let removed = self.terminals.remove(&id)?;
        self.creation_order.retain(|existing| *existing != id);
        #[cfg(test)]
        self.test_focus_state.forget_terminal(id);
        Some(removed)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (TerminalId, &ManagedTerminal)> {
        self.terminals.iter().map(|(id, terminal)| (*id, terminal))
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (TerminalId, &mut ManagedTerminal)> {
        self.terminals
            .iter_mut()
            .map(|(id, terminal)| (*id, terminal))
    }

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

    #[cfg(test)]
    pub(crate) fn create_terminal_without_focus(&mut self, bridge: TerminalBridge) -> TerminalId {
        let session_name = format!("terminal-{}", self.next_id);
        self.insert_terminal(bridge, session_name)
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "test compatibility API preserves pre-split focused-create helper"
    )]
    pub(crate) fn create_terminal_with_slot_and_session(
        &mut self,
        bridge: TerminalBridge,
        session_name: String,
    ) -> (TerminalId, usize) {
        let id = self.create_terminal_with_session(bridge, session_name);
        let slot = self.creation_order.len().saturating_sub(1);
        debug_assert_eq!(self.creation_order.get(slot), Some(&id));
        (id, slot)
    }

    #[cfg(test)]
    pub(crate) fn focus_terminal(&mut self, id: TerminalId) {
        let snapshot = self.clone_focus_state();
        let mut focus_state = snapshot;
        focus_state.focus_terminal(self, id);
        self.test_focus_state = focus_state;
    }

    #[cfg(test)]
    pub(crate) fn active_id(&self) -> Option<TerminalId> {
        self.test_focus_state.active_id()
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "test compatibility API preserves pre-split focus helpers"
    )]
    pub(crate) fn clear_active_terminal(&mut self) -> Option<TerminalId> {
        self.test_focus_state.clear_active_terminal()
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "test compatibility API preserves pre-split focus helpers"
    )]
    pub(crate) fn active_bridge(&self) -> Option<&TerminalBridge> {
        self.test_focus_state.active_bridge(self)
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "test compatibility API preserves pre-split focus helpers"
    )]
    pub(crate) fn active_snapshot(&self) -> Option<&TerminalSnapshot> {
        self.test_focus_state.active_snapshot(self)
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "test compatibility API preserves pre-split focus helpers"
    )]
    pub(crate) fn active_debug_stats(&self) -> TerminalDebugStats {
        self.test_focus_state.active_debug_stats(self)
    }

    #[cfg(test)]
    pub(crate) fn focus_order(&self) -> &[TerminalId] {
        self.test_focus_state.focus_order()
    }

    #[cfg(test)]
    pub(crate) fn clone_focus_state(&self) -> TerminalFocusState {
        self.test_focus_state.clone()
    }

    #[cfg(test)]
    pub(crate) fn replace_test_focus_state(&mut self, focus_state: &TerminalFocusState) {
        self.test_focus_state = focus_state.clone();
    }
}

pub(crate) fn poll_terminal_snapshots(mut terminal_manager: ResMut<TerminalManager>) {
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
        }

        if let Some((runtime, surface)) = latest_status {
            terminal.snapshot.runtime = runtime;
            if let Some(surface) = surface {
                terminal.snapshot.surface = Some(surface);
                terminal.surface_revision += 1;
                terminal.pending_damage = Some(TerminalDamage::Full);
            }
            terminal.bridge.note_snapshot_applied();
        }
    }
}
