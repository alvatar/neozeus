use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex, AgentStatus, AgentStatusStore},
    hud::HudInputCaptureState,
    terminals::{TerminalId, TerminalLifecycle, TerminalManager, TerminalRuntimeState},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum VisualAgentActivity {
    #[default]
    Idle,
    Working,
}

impl VisualAgentActivity {
    pub(crate) fn from_status(status: AgentStatus) -> Self {
        match status {
            AgentStatus::Working => Self::Working,
            AgentStatus::Unknown | AgentStatus::Idle => Self::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TerminalFrameVisualState {
    #[default]
    Hidden,
    DirectInput,
    Working,
    Exited,
    Disconnected,
    Failed,
}

pub(crate) fn terminal_frame_visual_state(
    direct_input: bool,
    runtime: &TerminalRuntimeState,
    agent_status: AgentStatus,
) -> TerminalFrameVisualState {
    if direct_input {
        return TerminalFrameVisualState::DirectInput;
    }
    if !runtime.is_interactive() {
        return match runtime.lifecycle {
            TerminalLifecycle::Exited { .. } => TerminalFrameVisualState::Exited,
            TerminalLifecycle::Disconnected => TerminalFrameVisualState::Disconnected,
            TerminalLifecycle::Failed => TerminalFrameVisualState::Failed,
            TerminalLifecycle::Running => TerminalFrameVisualState::Hidden,
        };
    }
    if agent_status == AgentStatus::Working {
        TerminalFrameVisualState::Working
    } else {
        TerminalFrameVisualState::Hidden
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VisualContractState {
    agent_activity: BTreeMap<AgentId, VisualAgentActivity>,
    terminal_frames: BTreeMap<TerminalId, TerminalFrameVisualState>,
}

impl VisualContractState {
    pub(crate) fn activity_for_agent(&self, agent_id: AgentId) -> VisualAgentActivity {
        self.agent_activity
            .get(&agent_id)
            .copied()
            .unwrap_or(VisualAgentActivity::Idle)
    }

    pub(crate) fn frame_for_terminal(&self, terminal_id: TerminalId) -> TerminalFrameVisualState {
        self.terminal_frames
            .get(&terminal_id)
            .copied()
            .unwrap_or(TerminalFrameVisualState::Hidden)
    }
}

pub(crate) fn sync_visual_contract_state(
    agent_catalog: Res<AgentCatalog>,
    runtime_index: Res<AgentRuntimeIndex>,
    status_store: Res<AgentStatusStore>,
    input_capture: Res<HudInputCaptureState>,
    terminal_manager: Res<TerminalManager>,
    mut visual_contract: ResMut<VisualContractState>,
) {
    let mut next = VisualContractState::default();

    for (agent_id, _) in agent_catalog.iter() {
        next.agent_activity.insert(
            agent_id,
            VisualAgentActivity::from_status(status_store.status(agent_id)),
        );
    }

    for (terminal_id, terminal) in terminal_manager.iter() {
        next.terminal_frames.insert(
            terminal_id,
            terminal_frame_visual_state(
                input_capture.direct_input_terminal == Some(terminal_id),
                &terminal.snapshot.runtime,
                status_store.status_for_terminal(&runtime_index, terminal_id),
            ),
        );
    }

    if *visual_contract != next {
        *visual_contract = next;
    }
}
