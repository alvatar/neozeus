use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    shared::{
        app_state_file::{
            parse_persisted_app_state, PersistedAgentKind, PersistedAgentState, PersistedAppState,
            APP_STATE_VERSION_V1, APP_STATE_VERSION_V2,
        },
        text_escape::{quote_escaped_string, EXTENDED_QUOTED_STRING_ESCAPES},
    },
    terminals::{
        append_debug_log, load_persisted_terminal_sessions_from, resolve_terminal_sessions_path,
        TerminalFocusState,
    },
};
use bevy::prelude::*;
use std::{collections::BTreeSet, fs, path::PathBuf};

#[cfg(test)]
use crate::shared::app_state_file::resolve_app_state_path_with;

const APP_STATE_SAVE_DEBOUNCE_SECS: f32 = 0.3;

pub(crate) use crate::shared::app_state_file::resolve_app_state_path;

/// Serializes persisted app-state metadata into the current version-2 app-state format.
pub(crate) fn serialize_persisted_app_state(state: &PersistedAppState) -> String {
    let mut output = String::from(APP_STATE_VERSION_V2);
    output.push('\n');
    let mut ordered = state.agents.clone();
    ordered.sort_by_key(|record| record.order_index);
    for record in ordered {
        output.push_str("[agent]\n");
        if let Some(agent_uid) = record.agent_uid {
            output.push_str(&format!(
                "agent_uid={}\n",
                quote_escaped_string(&agent_uid, EXTENDED_QUOTED_STRING_ESCAPES)
            ));
        }
        if let Some(runtime_session_name) = record.runtime_session_name {
            output.push_str(&format!(
                "runtime_session_name={}\n",
                quote_escaped_string(&runtime_session_name, EXTENDED_QUOTED_STRING_ESCAPES)
            ));
        }
        if let Some(label) = record.label {
            output.push_str(&format!(
                "label={}\n",
                quote_escaped_string(&label, EXTENDED_QUOTED_STRING_ESCAPES)
            ));
        }
        output.push_str(&format!(
            "kind={}\n",
            quote_escaped_string(
                record.kind.persistence_key(),
                EXTENDED_QUOTED_STRING_ESCAPES
            )
        ));
        if let Some(clone_source_session_path) = record.clone_source_session_path {
            output.push_str(&format!(
                "clone_source_session_path={}\n",
                quote_escaped_string(&clone_source_session_path, EXTENDED_QUOTED_STRING_ESCAPES)
            ));
        }
        output.push_str(&format!("workdir={}\n", u8::from(record.is_workdir)));
        if let Some(workdir_slug) = record.workdir_slug {
            output.push_str(&format!(
                "workdir_slug={}\n",
                quote_escaped_string(&workdir_slug, EXTENDED_QUOTED_STRING_ESCAPES)
            ));
        }
        output.push_str(&format!(
            "aegis_enabled={}\n",
            u8::from(record.aegis_enabled)
        ));
        if let Some(aegis_prompt_text) = record.aegis_prompt_text {
            output.push_str(&format!(
                "aegis_prompt_text={}\n",
                quote_escaped_string(&aegis_prompt_text, EXTENDED_QUOTED_STRING_ESCAPES)
            ));
        }
        output.push_str(&format!("order_index={}\n", record.order_index));
        output.push_str(&format!("focused={}\n", u8::from(record.last_focused)));
        output.push_str("[/agent]\n");
    }
    output
}

#[derive(Resource, Default)]
pub(crate) struct AppStatePersistenceState {
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty_since_secs: Option<f32>,
}

/// Returns the persisted agents that should be attached at startup in effective display order.
pub(crate) fn ordered_reconciled_persisted_agents(
    restore: &[PersistedAgentState],
    import: &[PersistedAgentState],
) -> Vec<PersistedAgentState> {
    restore.iter().chain(import.iter()).cloned().collect()
}

fn map_legacy_sessions_to_app_state(
    legacy: &crate::terminals::PersistedTerminalSessions,
) -> PersistedAppState {
    PersistedAppState {
        agents: legacy
            .sessions
            .iter()
            .map(|record| PersistedAgentState {
                agent_uid: None,
                runtime_session_name: Some(record.session_name.clone()),
                label: record.label.clone(),
                kind: PersistedAgentKind::Pi,
                clone_source_session_path: None,
                is_workdir: false,
                workdir_slug: None,
                aegis_enabled: false,
                aegis_prompt_text: None,
                order_index: record.creation_index,
                last_focused: record.last_focused,
            })
            .collect(),
    }
}

/// Loads persisted app-state metadata from disk, falling back to legacy terminal-session state when
/// the new app-state file does not yet exist.
pub(crate) fn load_persisted_app_state_from(path: &PathBuf) -> PersistedAppState {
    match fs::read_to_string(path) {
        Ok(text) => {
            let version_line = text
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(str::trim)
                .unwrap_or_default();
            if matches!(version_line, APP_STATE_VERSION_V1 | APP_STATE_VERSION_V2) {
                parse_persisted_app_state(&text)
            } else {
                map_legacy_sessions_to_app_state(&load_persisted_terminal_sessions_from(path))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            resolve_terminal_sessions_path()
                .as_ref()
                .filter(|legacy_path| legacy_path.exists())
                .map(load_persisted_terminal_sessions_from)
                .as_ref()
                .map(map_legacy_sessions_to_app_state)
                .unwrap_or_default()
        }
        Err(error) => {
            append_debug_log(format!("app state load failed {}: {error}", path.display()));
            PersistedAppState::default()
        }
    }
}

/// Marks app-state persistence dirty, recording the first dirty timestamp if needed.
pub(crate) fn mark_app_state_dirty(
    persistence_state: &mut AppStatePersistenceState,
    time: Option<&Time>,
) {
    if persistence_state.dirty_since_secs.is_none() {
        persistence_state.dirty_since_secs = Some(time.map(Time::elapsed_secs).unwrap_or(0.0));
    }
}

fn build_persisted_app_state(
    focus_state: &TerminalFocusState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    aegis_policy: &crate::aegis::AegisPolicyStore,
) -> PersistedAppState {
    let agents = agent_catalog
        .order
        .iter()
        .enumerate()
        .map(|(index, agent_id)| {
            let terminal_id = runtime_index.primary_terminal(*agent_id);
            let agent_uid = agent_catalog.uid(*agent_id).map(str::to_owned);
            let aegis_policy = agent_uid
                .as_deref()
                .and_then(|agent_uid| aegis_policy.policy(agent_uid));
            PersistedAgentState {
                agent_uid,
                runtime_session_name: None,
                label: agent_catalog.label(*agent_id).map(str::to_owned),
                kind: match agent_catalog
                    .kind(*agent_id)
                    .unwrap_or(crate::agents::AgentKind::Pi)
                {
                    crate::agents::AgentKind::Pi => PersistedAgentKind::Pi,
                    crate::agents::AgentKind::Claude => PersistedAgentKind::Claude,
                    crate::agents::AgentKind::Codex => PersistedAgentKind::Codex,
                    crate::agents::AgentKind::Terminal => PersistedAgentKind::Terminal,
                    crate::agents::AgentKind::Verifier => PersistedAgentKind::Verifier,
                },
                clone_source_session_path: agent_catalog
                    .clone_source_session_path(*agent_id)
                    .map(str::to_owned),
                is_workdir: agent_catalog.is_workdir(*agent_id),
                workdir_slug: agent_catalog.workdir_slug(*agent_id).map(str::to_owned),
                aegis_enabled: aegis_policy.is_some_and(|policy| policy.enabled),
                aegis_prompt_text: aegis_policy.map(|policy| policy.prompt_text.clone()),
                order_index: index as u64,
                last_focused: terminal_id
                    .is_some_and(|terminal_id| focus_state.active_id() == Some(terminal_id)),
            }
        })
        .collect();
    PersistedAppState { agents }
}

/// Writes the app-state persistence file once the debounce window has elapsed.
pub(crate) fn save_app_state_if_dirty(
    time: Res<Time>,
    focus_state: Res<TerminalFocusState>,
    agent_catalog: Res<AgentCatalog>,
    runtime_index: Res<AgentRuntimeIndex>,
    aegis_policy: Res<crate::aegis::AegisPolicyStore>,
    mut persistence_state: ResMut<AppStatePersistenceState>,
) {
    let Some(dirty_since) = persistence_state.dirty_since_secs else {
        return;
    };
    if time.elapsed_secs() - dirty_since < APP_STATE_SAVE_DEBOUNCE_SECS {
        return;
    }
    let Some(path) = persistence_state.path.as_ref() else {
        persistence_state.dirty_since_secs = None;
        return;
    };
    let persisted =
        build_persisted_app_state(&focus_state, &agent_catalog, &runtime_index, &aegis_policy);
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            append_debug_log(format!(
                "app state mkdir failed {}: {error}",
                parent.display()
            ));
            persistence_state.dirty_since_secs = None;
            return;
        }
    }
    let serialized = serialize_persisted_app_state(&persisted);
    if let Err(error) = fs::write(path, serialized) {
        append_debug_log(format!("app state save failed {}: {error}", path.display()));
    } else {
        append_debug_log(format!("app state saved {}", path.display()));
    }
    persistence_state.dirty_since_secs = None;
}

/// Reconciles persisted app-state agent metadata against the daemon's currently live sessions.
pub(crate) fn reconcile_persisted_agents(
    persisted: &PersistedAppState,
    live_sessions: &[crate::terminals::DaemonSessionInfo],
) -> (
    Vec<PersistedAgentState>,
    Vec<PersistedAgentState>,
    Vec<String>,
) {
    let live_by_uid = live_sessions
        .iter()
        .filter_map(|session| {
            session
                .metadata
                .agent_uid
                .as_deref()
                .map(|agent_uid| (agent_uid, session.session_id.as_str()))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let live_by_session = live_sessions
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect::<std::collections::HashMap<_, _>>();
    let mut matched_live_sessions = BTreeSet::new();

    let mut restore = Vec::new();
    let mut prune = Vec::new();
    for record in &persisted.agents {
        let matched_session_name = record
            .agent_uid
            .as_deref()
            .and_then(|agent_uid| live_by_uid.get(agent_uid).copied())
            .or_else(|| {
                record
                    .runtime_session_name
                    .as_deref()
                    .and_then(|session_name| {
                        live_by_session
                            .contains_key(session_name)
                            .then_some(session_name)
                    })
            });
        if let Some(session_name) = matched_session_name {
            matched_live_sessions.insert(session_name.to_owned());
            let mut restored = record.clone();
            restored.runtime_session_name = Some(session_name.to_owned());
            restore.push(restored);
        } else {
            prune.push(record.clone());
        }
    }

    let mut import = live_sessions
        .iter()
        .filter(|session| {
            session
                .session_id
                .starts_with(crate::terminals::PERSISTENT_SESSION_PREFIX)
        })
        .filter(|session| !matched_live_sessions.contains(&session.session_id))
        .map(|session| session.session_id.clone())
        .collect::<Vec<_>>();
    import.sort();

    (restore, prune, import)
}

#[cfg(test)]
mod tests;
