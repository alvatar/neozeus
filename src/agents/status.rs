use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::{
    terminals::{TerminalManager, TerminalSurface},
    verification::VerificationTerminalSurfaceOverrides,
};

use super::{AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex};

const STATUS_SCAN_WINDOW_LINES: usize = 8;
const STATUS_DEBUG_TAIL_LINES: usize = 4;
const TERMINAL_IDLE_TIMEOUT_SECS: f64 = 5.0;

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
    pub(crate) context_pct_milli: Option<i32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct AgentActivityState {
    last_visible_rows: Vec<String>,
    last_output_secs: Option<f64>,
}

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct AgentStatusStore {
    samples: BTreeMap<AgentId, AgentStatusSample>,
    activity: BTreeMap<AgentId, AgentActivityState>,
}

impl AgentStatusStore {
    /// Returns the retained derived status for one agent.
    pub(crate) fn status(&self, agent_id: AgentId) -> AgentStatus {
        self.samples
            .get(&agent_id)
            .map(|sample| sample.status)
            .unwrap_or(AgentStatus::Unknown)
    }

    pub(crate) fn context_pct_milli(&self, agent_id: AgentId) -> Option<i32> {
        self.samples
            .get(&agent_id)
            .and_then(|sample| sample.context_pct_milli)
    }

    #[cfg(test)]
    pub(crate) fn set_status_for_tests(&mut self, agent_id: AgentId, status: AgentStatus) {
        self.samples.entry(agent_id).or_default().status = status;
    }
}

/// Rebuilds the per-agent derived status cache from the current terminal surfaces.
pub(crate) fn sync_agent_status(
    agent_catalog: Res<AgentCatalog>,
    runtime_index: Res<AgentRuntimeIndex>,
    terminal_manager: Res<TerminalManager>,
    verification_overrides: Option<Res<VerificationTerminalSurfaceOverrides>>,
    time: Res<Time>,
    mut status_store: ResMut<AgentStatusStore>,
) {
    let now_secs = f64::from(time.elapsed_secs());
    let mut previous_activity = std::mem::take(&mut status_store.activity);
    let mut samples = BTreeMap::new();
    let mut activity = BTreeMap::new();

    for (agent_id, _) in agent_catalog.iter() {
        let Some(kind) = agent_catalog.kind(agent_id) else {
            samples.insert(agent_id, AgentStatusSample::default());
            continue;
        };

        let Some(surface) = runtime_index
            .primary_terminal(agent_id)
            .and_then(|terminal_id| {
                verification_overrides
                    .as_ref()
                    .and_then(|overrides| overrides.surface_for(terminal_id))
                    .or_else(|| {
                        terminal_manager
                            .get(terminal_id)
                            .and_then(|terminal| terminal.snapshot.surface.as_ref())
                    })
            })
        else {
            samples.insert(agent_id, AgentStatusSample::default());
            continue;
        };

        let mut agent_activity = previous_activity.remove(&agent_id).unwrap_or_default();
        let sample = sample_agent_status(kind, surface, now_secs, &mut agent_activity);
        samples.insert(agent_id, sample);
        activity.insert(agent_id, agent_activity);
    }

    status_store.samples = samples;
    status_store.activity = activity;
}

fn sample_agent_status(
    kind: AgentKind,
    surface: &TerminalSurface,
    now_secs: f64,
    activity: &mut AgentActivityState,
) -> AgentStatusSample {
    let visible_rows = extract_visible_rows(surface);
    if visible_rows != activity.last_visible_rows {
        let has_previous_baseline = !activity.last_visible_rows.is_empty();
        activity.last_visible_rows = visible_rows.clone();
        if has_previous_baseline {
            activity.last_output_secs = Some(now_secs);
        }
    }
    let last_four_lines = visible_rows
        .iter()
        .rev()
        .take(STATUS_DEBUG_TAIL_LINES)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let scan_lines = visible_rows
        .iter()
        .rev()
        .take(STATUS_SCAN_WINDOW_LINES)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let status = classify_agent_status(kind, &scan_lines, activity.last_output_secs, now_secs);
    AgentStatusSample {
        last_four_lines,
        status,
        context_pct_milli: parse_agent_context_pct_milli(surface),
    }
}

fn classify_agent_status(
    kind: AgentKind,
    lines: &[String],
    last_output_secs: Option<f64>,
    now_secs: f64,
) -> AgentStatus {
    if !lines.iter().any(|line| !line.trim().is_empty()) {
        return AgentStatus::Unknown;
    }

    match kind {
        AgentKind::Pi => {
            if lines.iter().any(|line| is_pi_working_indicator_line(line)) {
                AgentStatus::Working
            } else {
                AgentStatus::Idle
            }
        }
        AgentKind::Claude => {
            if lines
                .iter()
                .any(|line| is_claude_working_indicator_line(line))
            {
                AgentStatus::Working
            } else {
                AgentStatus::Idle
            }
        }
        AgentKind::Codex => {
            if lines
                .iter()
                .any(|line| is_codex_working_indicator_line(line))
            {
                AgentStatus::Working
            } else {
                AgentStatus::Idle
            }
        }
        AgentKind::Terminal => {
            if last_output_secs.is_some_and(|last_output_secs| {
                now_secs - last_output_secs < TERMINAL_IDLE_TIMEOUT_SECS
            }) {
                AgentStatus::Working
            } else {
                AgentStatus::Idle
            }
        }
        AgentKind::Verifier => AgentStatus::Unknown,
    }
}

// Pi exposes a braille spinner plus the literal `Working...` line near the bottom of the visible
// tail while the agent is active.
fn is_pi_working_indicator_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    ('\u{2800}'..='\u{28ff}').contains(&first) && trimmed.contains("Working...")
}

// Claude Code's live TUI exposes a transient gerund status line such as `✻ Harmonizing…` or
// `· Boogieing…` while working. The footer's `(thinking)` text is not stable enough on its own.
fn is_claude_working_indicator_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some((marker, rest)) = trimmed.split_once(' ') else {
        return false;
    };
    if marker.chars().count() != 1 || !matches!(marker.chars().next(), Some('·' | '✶' | '✻')) {
        return false;
    }
    let Some(verb) = rest.trim().strip_suffix('…') else {
        return false;
    };
    verb.ends_with("ing") && verb.chars().all(|ch| ch.is_ascii_alphabetic())
}

// Codex exposes an explicit status line like `• Working (5s • esc to interrupt)` while active.
fn is_codex_working_indicator_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("• Working (") && trimmed.contains("esc to interrupt")
}

fn extract_visible_rows(surface: &TerminalSurface) -> Vec<String> {
    (0..surface.rows)
        .map(|row| row_text(surface, row))
        .collect::<Vec<_>>()
}

pub(crate) fn parse_agent_context_pct_milli(surface: &TerminalSurface) -> Option<i32> {
    (0..surface.rows)
        .rev()
        .take(8)
        .find_map(|row| parse_context_pct_milli(&row_text(surface, row)))
}

fn parse_context_pct_milli(line: &str) -> Option<i32> {
    parse_pi_footer_context_pct_milli(line).or_else(|| parse_codex_footer_context_pct_milli(line))
}

fn parse_pi_footer_context_pct_milli(line: &str) -> Option<i32> {
    let ctx_start = line.find("Ctx(")?;
    let tail = &line[ctx_start..];
    let pct_end = tail.find("%)")?;
    let pct_start = tail[..pct_end].rfind('(')? + 1;
    parse_percent_milli(&tail[pct_start..pct_end])
}

fn parse_codex_footer_context_pct_milli(line: &str) -> Option<i32> {
    let pct_end = line.find("% left")?;
    let prefix = line[..pct_end].trim_end();
    let pct_start = prefix
        .rfind(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .map_or(0, |index| index + 1);
    let remaining = parse_percent_milli(prefix[pct_start..].trim())?;
    Some((100_000 - remaining).clamp(0, 100_000))
}

fn parse_percent_milli(raw: &str) -> Option<i32> {
    let pct = raw.trim().parse::<f32>().ok()?;
    ((0.0..=100.0).contains(&pct)).then_some((pct * 1000.0).round() as i32)
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
    use super::{
        sample_agent_status, sync_agent_status, AgentActivityState, AgentStatus, AgentStatusStore,
    };
    use crate::{
        agents::{AgentCatalog, AgentKind, AgentRuntimeIndex},
        terminals::{TerminalManager, TerminalRuntimeState},
        tests::{insert_terminal_manager_resources, surface_with_text, test_bridge},
    };
    use bevy::{ecs::system::RunSystemOnce, prelude::*};
    use std::time::Duration;

    #[test]
    fn sample_agent_status_detects_pi_working_spinner_above_footer_tail() {
        let mut surface = surface_with_text(8, 120, 0, "npm:pi-mcp-adapter");
        surface.set_text_cell(1, 3, "⠋ Working...");
        surface.set_text_cell(0, 5, "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────");
        surface.set_text_cell(0, 7, "claude-opus-4-6 (high) Ctx(auto):░░░░░░░░░░(0.0%) Session:░░░░░░░░░░(2.0%) Week:███████░░░(69.0%) ↑0 ↓0");

        let sample = sample_agent_status(
            AgentKind::Pi,
            &surface,
            0.0,
            &mut AgentActivityState::default(),
        );

        assert_eq!(sample.status, AgentStatus::Working);
        assert_eq!(sample.last_four_lines.len(), 4);
        assert!(!sample
            .last_four_lines
            .iter()
            .any(|line| line.contains("Working...")));
    }

    #[test]
    fn sample_agent_status_detects_claude_working_gerund_line() {
        let mut surface = surface_with_text(8, 120, 0, "header");
        surface.set_text_cell(0, 4, "✻ Harmonizing…");
        surface.set_text_cell(0, 5, "⎿  Tip: Hit shift+tab to cycle modes");

        let sample = sample_agent_status(
            AgentKind::Claude,
            &surface,
            0.0,
            &mut AgentActivityState::default(),
        );

        assert_eq!(sample.status, AgentStatus::Working);
    }

    #[test]
    fn sample_agent_status_detects_codex_working_line() {
        let mut surface = surface_with_text(8, 120, 0, "header");
        surface.set_text_cell(0, 5, "• Working (5s • esc to interrupt)");

        let sample = sample_agent_status(
            AgentKind::Codex,
            &surface,
            0.0,
            &mut AgentActivityState::default(),
        );

        assert_eq!(sample.status, AgentStatus::Working);
    }

    #[test]
    fn sample_agent_status_terminal_starts_idle_then_tracks_real_output_changes() {
        let surface = surface_with_text(4, 40, 0, "shell prompt $");
        let changed_surface = surface_with_text(4, 40, 0, "shell prompt $ ls");
        let mut activity = AgentActivityState::default();

        let initial = sample_agent_status(AgentKind::Terminal, &surface, 0.0, &mut activity);
        let active = sample_agent_status(AgentKind::Terminal, &changed_surface, 1.0, &mut activity);
        let idle = sample_agent_status(AgentKind::Terminal, &changed_surface, 7.0, &mut activity);

        assert_eq!(initial.status, AgentStatus::Idle);
        assert_eq!(active.status, AgentStatus::Working);
        assert_eq!(idle.status, AgentStatus::Idle);
    }

    #[test]
    fn sync_agent_status_only_derives_user_facing_agents() {
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
        let pi_agent = catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Pi,
            AgentKind::Pi.capabilities(),
        );
        let verifier_agent = catalog.create_agent(
            Some("verifier".into()),
            AgentKind::Verifier,
            AgentKind::Verifier.capabilities(),
        );
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(
            pi_agent,
            terminal_id,
            "neozeus-session-1".into(),
            Some(&TerminalRuntimeState::running("running")),
        );

        let mut world = World::default();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        world.insert_resource(time);
        world.insert_resource(catalog);
        world.insert_resource(runtime_index);
        world.insert_resource(AgentStatusStore::default());
        insert_terminal_manager_resources(&mut world, terminal_manager);

        world.run_system_once(sync_agent_status).unwrap();

        let status_store = world.resource::<AgentStatusStore>();
        assert_eq!(status_store.status(pi_agent), AgentStatus::Working);
        assert_eq!(status_store.status(verifier_agent), AgentStatus::Unknown);
    }
}
