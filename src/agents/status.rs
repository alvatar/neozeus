use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::terminals::{TerminalManager, TerminalSurface};

use super::{AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex};

const STATUS_SCAN_WINDOW_LINES: usize = 8;
const STATUS_DEBUG_TAIL_LINES: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AgentStatus {
    #[default]
    Unknown,
    Idle,
    Working,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentStatusSample {
    pub(crate) last_four_lines: Vec<String>,
    pub(crate) status: AgentStatus,
}

#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentStatusStore {
    samples: BTreeMap<AgentId, AgentStatusSample>,
}

impl AgentStatusStore {
    /// Returns the retained derived status for one agent.
    pub(crate) fn status(&self, agent_id: AgentId) -> AgentStatus {
        self.samples
            .get(&agent_id)
            .map(|sample| sample.status)
            .unwrap_or(AgentStatus::Unknown)
    }
}

/// Rebuilds the per-agent derived status cache from the current terminal surfaces.
pub(crate) fn sync_agent_status(
    agent_catalog: Res<AgentCatalog>,
    runtime_index: Res<AgentRuntimeIndex>,
    terminal_manager: Res<TerminalManager>,
    mut status_store: ResMut<AgentStatusStore>,
) {
    let mut samples = BTreeMap::new();

    for (agent_id, _) in agent_catalog.iter() {
        let status = match agent_catalog.kind(agent_id) {
            Some(AgentKind::Terminal) => runtime_index
                .primary_terminal(agent_id)
                .and_then(|terminal_id| terminal_manager.get(terminal_id))
                .and_then(|terminal| terminal.snapshot.surface.as_ref())
                .map(sample_agent_status)
                .unwrap_or_default(),
            _ => AgentStatusSample::default(),
        };
        samples.insert(agent_id, status);
    }

    status_store.samples = samples;
}

fn sample_agent_status(surface: &TerminalSurface) -> AgentStatusSample {
    let visible_rows = extract_visible_rows(surface);
    let last_four_lines = visible_rows
        .iter()
        .rev()
        .take(STATUS_DEBUG_TAIL_LINES)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let status = classify_agent_status(
        &visible_rows
            .iter()
            .rev()
            .take(STATUS_SCAN_WINDOW_LINES)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>(),
    );
    AgentStatusSample {
        last_four_lines,
        status,
    }
}

fn classify_agent_status(lines: &[String]) -> AgentStatus {
    if lines.iter().any(|line| is_working_indicator_line(line)) {
        AgentStatus::Working
    } else if lines.is_empty() {
        AgentStatus::Unknown
    } else {
        AgentStatus::Idle
    }
}

fn is_working_indicator_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    ('\u{2800}'..='\u{28ff}').contains(&first) && trimmed.contains("Working...")
}

fn extract_visible_rows(surface: &TerminalSurface) -> Vec<String> {
    (0..surface.rows)
        .map(|row| row_text(surface, row))
        .collect::<Vec<_>>()
}

fn row_text(surface: &TerminalSurface, row: usize) -> String {
    let mut text = String::new();
    for col in 0..surface.cols {
        let cell = surface.cell(col, row);
        if cell.width == 0 {
            continue;
        }
        text.push_str(&cell.content.to_owned_string());
    }
    text.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{sample_agent_status, sync_agent_status, AgentStatus, AgentStatusStore};
    use crate::{
        agents::{AgentCapabilities, AgentCatalog, AgentKind, AgentRuntimeIndex},
        terminals::{TerminalManager, TerminalRuntimeState},
        tests::{insert_terminal_manager_resources, surface_with_text, test_bridge},
    };
    use bevy::{ecs::system::RunSystemOnce, prelude::*};

    #[test]
    fn sample_agent_status_detects_working_spinner_above_footer_tail() {
        let mut surface = surface_with_text(8, 120, 0, "npm:pi-mcp-adapter");
        surface.set_text_cell(1, 3, "⠋ Working...");
        surface.set_text_cell(0, 5, "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────");
        surface.set_text_cell(0, 7, "claude-opus-4-6 (high) Ctx(auto):░░░░░░░░░░(0.0%) Session:░░░░░░░░░░(2.0%) Week:███████░░░(69.0%) ↑0 ↓0");

        let sample = sample_agent_status(&surface);

        assert_eq!(sample.status, AgentStatus::Working);
        assert_eq!(sample.last_four_lines.len(), 4);
        assert!(!sample
            .last_four_lines
            .iter()
            .any(|line| line.contains("Working...")));
    }

    #[test]
    fn sample_agent_status_falls_back_to_idle_without_spinner_match() {
        let mut surface = surface_with_text(4, 40, 0, "hello");
        surface.set_text_cell(0, 3, "ready");

        let sample = sample_agent_status(&surface);

        assert_eq!(sample.status, AgentStatus::Idle);
        assert_eq!(sample.last_four_lines.len(), 4);
    }

    #[test]
    fn sync_agent_status_only_derives_terminal_agents() {
        let (bridge, _) = test_bridge();
        let mut terminal_manager = TerminalManager::default();
        let terminal_id = terminal_manager.create_terminal(bridge);
        terminal_manager
            .get_mut(terminal_id)
            .expect("terminal should exist")
            .snapshot
            .surface = Some({
            let mut surface = surface_with_text(8, 120, 0, "header");
            surface.set_text_cell(1, 3, "⠙ Working...");
            surface
        });

        let mut catalog = AgentCatalog::default();
        let terminal_agent = catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Terminal,
            AgentCapabilities::terminal_defaults(),
        );
        let verifier_agent = catalog.create_agent(
            Some("verifier".into()),
            AgentKind::Verifier,
            AgentCapabilities::verifier_defaults(),
        );
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(
            terminal_agent,
            terminal_id,
            "neozeus-session-1".into(),
            Some(&TerminalRuntimeState::running("running")),
        );

        let mut world = World::default();
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
        world.insert_resource(AgentStatusStore::default());
        insert_terminal_manager_resources(&mut world, terminal_manager);

        world.run_system_once(sync_agent_status).unwrap();

        let status_store = world.resource::<AgentStatusStore>();
        assert_eq!(status_store.status(terminal_agent), AgentStatus::Working);
        assert_eq!(status_store.status(verifier_agent), AgentStatus::Unknown);
    }
}
