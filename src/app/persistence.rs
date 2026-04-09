use crate::{
    agents::{AgentCatalog, AgentRuntimeIndex},
    shared::{
        app_state_file::{
            parse_persisted_app_state, PersistedAgentKind, PersistedAgentRecoverySpec,
            PersistedAgentState, PersistedAppState, APP_STATE_VERSION_V1, APP_STATE_VERSION_V2,
            APP_STATE_VERSION_V3, APP_STATE_VERSION_V4,
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

/// Serializes persisted app-state metadata into the current version-4 app-state format.
pub(crate) fn serialize_persisted_app_state(state: &PersistedAppState) -> String {
    let mut output = String::from(APP_STATE_VERSION_V4);
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
        match record.recovery {
            Some(PersistedAgentRecoverySpec::Pi {
                session_path,
                cwd,
                is_workdir,
                workdir_slug,
            }) => {
                output.push_str("recovery_mode=\"pi\"\n");
                output.push_str(&format!(
                    "recovery_session_path={}\n",
                    quote_escaped_string(&session_path, EXTENDED_QUOTED_STRING_ESCAPES)
                ));
                if let Some(cwd) = cwd {
                    output.push_str(&format!(
                        "recovery_cwd={}\n",
                        quote_escaped_string(&cwd, EXTENDED_QUOTED_STRING_ESCAPES)
                    ));
                }
                output.push_str(&format!("workdir={}\n", u8::from(is_workdir)));
                if let Some(workdir_slug) = workdir_slug {
                    output.push_str(&format!(
                        "workdir_slug={}\n",
                        quote_escaped_string(&workdir_slug, EXTENDED_QUOTED_STRING_ESCAPES)
                    ));
                }
            }
            Some(PersistedAgentRecoverySpec::Claude {
                session_id,
                cwd,
                model,
                profile,
            }) => {
                output.push_str("recovery_mode=\"claude\"\n");
                output.push_str(&format!(
                    "recovery_session_id={}\n",
                    quote_escaped_string(&session_id, EXTENDED_QUOTED_STRING_ESCAPES)
                ));
                output.push_str(&format!(
                    "recovery_cwd={}\n",
                    quote_escaped_string(&cwd, EXTENDED_QUOTED_STRING_ESCAPES)
                ));
                if let Some(model) = model {
                    output.push_str(&format!(
                        "recovery_model={}\n",
                        quote_escaped_string(&model, EXTENDED_QUOTED_STRING_ESCAPES)
                    ));
                }
                if let Some(profile) = profile {
                    output.push_str(&format!(
                        "recovery_profile={}\n",
                        quote_escaped_string(&profile, EXTENDED_QUOTED_STRING_ESCAPES)
                    ));
                }
            }
            Some(PersistedAgentRecoverySpec::Codex {
                session_id,
                cwd,
                model,
                profile,
            }) => {
                output.push_str("recovery_mode=\"codex\"\n");
                output.push_str(&format!(
                    "recovery_session_id={}\n",
                    quote_escaped_string(&session_id, EXTENDED_QUOTED_STRING_ESCAPES)
                ));
                output.push_str(&format!(
                    "recovery_cwd={}\n",
                    quote_escaped_string(&cwd, EXTENDED_QUOTED_STRING_ESCAPES)
                ));
                if let Some(model) = model {
                    output.push_str(&format!(
                        "recovery_model={}\n",
                        quote_escaped_string(&model, EXTENDED_QUOTED_STRING_ESCAPES)
                    ));
                }
                if let Some(profile) = profile {
                    output.push_str(&format!(
                        "recovery_profile={}\n",
                        quote_escaped_string(&profile, EXTENDED_QUOTED_STRING_ESCAPES)
                    ));
                }
            }
            None => {}
        }
        output.push_str(&format!("order_index={}\n", record.order_index));
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
                recovery: None,
                clone_source_session_path: None,
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
            if matches!(
                version_line,
                APP_STATE_VERSION_V1
                    | APP_STATE_VERSION_V2
                    | APP_STATE_VERSION_V3
                    | crate::shared::app_state_file::APP_STATE_VERSION_V4
            ) {
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
    _focus_state: &TerminalFocusState,
    agent_catalog: &AgentCatalog,
    _runtime_index: &AgentRuntimeIndex,
    _aegis_policy: &crate::aegis::AegisPolicyStore,
) -> PersistedAppState {
    let agents = agent_catalog
        .order
        .iter()
        .filter_map(|agent_id| {
            let agent_uid = agent_catalog.uid(*agent_id).map(str::to_owned);
            let recovery = agent_catalog
                .recovery_spec(*agent_id)
                .map(|spec| match spec {
                    crate::agents::AgentRecoverySpec::Pi {
                        session_path,
                        cwd,
                        is_workdir,
                        workdir_slug,
                    } => PersistedAgentRecoverySpec::Pi {
                        session_path: session_path.clone(),
                        cwd: Some(cwd.clone()),
                        is_workdir: *is_workdir,
                        workdir_slug: workdir_slug.clone(),
                    },
                    crate::agents::AgentRecoverySpec::Claude {
                        session_id,
                        cwd,
                        model,
                        profile,
                    } => PersistedAgentRecoverySpec::Claude {
                        session_id: session_id.clone(),
                        cwd: cwd.clone(),
                        model: model.clone(),
                        profile: profile.clone(),
                    },
                    crate::agents::AgentRecoverySpec::Codex {
                        session_id,
                        cwd,
                        model,
                        profile,
                    } => PersistedAgentRecoverySpec::Codex {
                        session_id: session_id.clone(),
                        cwd: cwd.clone(),
                        model: model.clone(),
                        profile: profile.clone(),
                    },
                })?;
            Some((agent_uid, recovery, *agent_id))
        })
        .enumerate()
        .map(
            |(index, (agent_uid, recovery, agent_id))| PersistedAgentState {
                agent_uid,
                runtime_session_name: None,
                label: agent_catalog.label(agent_id).map(str::to_owned),
                kind: match agent_catalog
                    .kind(agent_id)
                    .unwrap_or(crate::agents::AgentKind::Pi)
                {
                    crate::agents::AgentKind::Pi => PersistedAgentKind::Pi,
                    crate::agents::AgentKind::Claude => PersistedAgentKind::Claude,
                    crate::agents::AgentKind::Codex => PersistedAgentKind::Codex,
                    crate::agents::AgentKind::Terminal => PersistedAgentKind::Terminal,
                    crate::agents::AgentKind::Verifier => PersistedAgentKind::Verifier,
                },
                recovery: Some(recovery),
                clone_source_session_path: None,
                aegis_enabled: false,
                aegis_prompt_text: None,
                order_index: index as u64,
                last_focused: false,
            },
        )
        .collect();
    PersistedAppState { agents }
}

fn write_file_atomically(path: &PathBuf, content: &str) -> Result<(), String> {
    let mut tmp_path = path.clone();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("invalid app state file name {}", path.display()))?;
    tmp_path.set_file_name(format!(".{file_name}.tmp"));
    fs::write(&tmp_path, content).map_err(|error| {
        format!(
            "failed to write temp app state {}: {error}",
            tmp_path.display()
        )
    })?;
    fs::rename(&tmp_path, path).map_err(|error| {
        let _ = fs::remove_file(&tmp_path);
        format!(
            "failed to replace app state {} from {}: {error}",
            path.display(),
            tmp_path.display()
        )
    })
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
    if let Err(error) = write_file_atomically(path, &serialized) {
        append_debug_log(format!("app state save failed {}", error));
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
