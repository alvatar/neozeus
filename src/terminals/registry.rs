use crate::terminals::{
    append_debug_log, TerminalBridge, TerminalDamage, TerminalDebugStats, TerminalSnapshot,
};
use bevy::prelude::{ResMut, Resource};
use std::collections::{hash_map, HashMap};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct TerminalId(pub(crate) u64);

pub(crate) struct ManagedTerminal {
    pub(crate) bridge: TerminalBridge,
    pub(crate) snapshot: TerminalSnapshot,
    pub(crate) pending_damage: Option<TerminalDamage>,
    pub(crate) surface_revision: u64,
}

#[derive(Resource, Default)]
pub(crate) struct TerminalManager {
    next_id: u64,
    active_id: Option<TerminalId>,
    order: Vec<TerminalId>,
    terminals: HashMap<TerminalId, ManagedTerminal>,
}

impl TerminalManager {
    pub(crate) fn create_terminal(&mut self, bridge: TerminalBridge) -> TerminalId {
        let id = TerminalId(self.next_id.max(1));
        self.next_id = id.0 + 1;
        self.terminals.insert(
            id,
            ManagedTerminal {
                bridge,
                snapshot: TerminalSnapshot::default(),
                pending_damage: None,
                surface_revision: 0,
            },
        );
        self.focus_terminal(id);
        id
    }

    pub(crate) fn create_terminal_with_slot(
        &mut self,
        bridge: TerminalBridge,
    ) -> (TerminalId, usize) {
        let id = self.create_terminal(bridge);
        let slot = self
            .slot_of(id)
            .expect("newly created terminal must have a layout slot");
        (id, slot)
    }

    pub(crate) fn focus_terminal(&mut self, id: TerminalId) {
        if !self.terminals.contains_key(&id) {
            return;
        }
        self.active_id = Some(id);
        self.order.retain(|existing| *existing != id);
        self.order.push(id);
        append_debug_log(format!("focused terminal {}", id.0));
    }

    pub(crate) fn active_id(&self) -> Option<TerminalId> {
        self.active_id
    }

    pub(crate) fn active_bridge(&self) -> Option<&TerminalBridge> {
        self.active_id
            .and_then(|id| self.terminals.get(&id).map(|terminal| &terminal.bridge))
    }

    pub(crate) fn active_snapshot(&self) -> Option<&TerminalSnapshot> {
        self.active_id
            .and_then(|id| self.terminals.get(&id).map(|terminal| &terminal.snapshot))
    }

    pub(crate) fn active_debug_stats(&self) -> TerminalDebugStats {
        self.active_bridge()
            .map(TerminalBridge::debug_stats_snapshot)
            .unwrap_or_default()
    }

    pub(crate) fn terminal_ids(&self) -> &[TerminalId] {
        &self.order
    }

    pub(crate) fn slot_of(&self, id: TerminalId) -> Option<usize> {
        self.order.iter().position(|existing| *existing == id)
    }

    pub(crate) fn get(&self, id: TerminalId) -> Option<&ManagedTerminal> {
        self.terminals.get(&id)
    }

    pub(crate) fn iter(&self) -> hash_map::Iter<'_, TerminalId, ManagedTerminal> {
        self.terminals.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> hash_map::IterMut<'_, TerminalId, ManagedTerminal> {
        self.terminals.iter_mut()
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
