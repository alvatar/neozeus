use super::{
    persistence::resolve_state_path_with,
    text_escape::{unquote_escaped_string, EXTENDED_QUOTED_STRING_ESCAPES},
};
use std::{env, path::PathBuf};

pub const APP_STATE_FILENAME: &str = "neozeus-state.v1";
pub const APP_STATE_VERSION_V1: &str = "neozeus state version 1";
pub const APP_STATE_VERSION_V2: &str = "neozeus state version 2";
pub const APP_STATE_VERSION_V3: &str = "neozeus state version 3";
pub const APP_STATE_VERSION_V4: &str = "neozeus state version 4";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PersistedAgentKind {
    #[default]
    Pi,
    Claude,
    Codex,
    Terminal,
    Verifier,
}

impl PersistedAgentKind {
    pub const fn persistence_key(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Terminal => "terminal",
            Self::Verifier => "verifier",
        }
    }

    pub fn from_persistence_key(value: &str) -> Option<Self> {
        match value.trim() {
            "pi" => Some(Self::Pi),
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "terminal" => Some(Self::Terminal),
            "verifier" => Some(Self::Verifier),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersistedAgentRecoverySpec {
    Pi {
        session_path: String,
        cwd: Option<String>,
        is_workdir: bool,
        workdir_slug: Option<String>,
    },
    Claude {
        session_id: String,
        cwd: String,
        model: Option<String>,
        profile: Option<String>,
    },
    Codex {
        session_id: String,
        cwd: String,
        model: Option<String>,
        profile: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedAgentState {
    pub agent_uid: Option<String>,
    pub runtime_session_name: Option<String>,
    pub label: Option<String>,
    pub kind: PersistedAgentKind,
    pub recovery: Option<PersistedAgentRecoverySpec>,
    pub clone_source_session_path: Option<String>,
    pub aegis_enabled: bool,
    pub aegis_prompt_text: Option<String>,
    pub order_index: u64,
    pub last_focused: bool,
}

impl PersistedAgentState {
    pub fn durability(&self) -> crate::shared::agent_durability::AgentDurability {
        if self.recovery.is_some() {
            crate::shared::agent_durability::AgentDurability::Recoverable
        } else {
            crate::shared::agent_durability::AgentDurability::LiveOnly
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PersistedAppState {
    pub agents: Vec<PersistedAgentState>,
}

/// Resolves the app-state persistence file path from explicit directory inputs.
pub fn resolve_app_state_path_with(
    xdg_state_home: Option<&str>,
    home: Option<&str>,
    xdg_config_home: Option<&str>,
) -> Option<PathBuf> {
    resolve_state_path_with(
        xdg_state_home,
        home,
        xdg_config_home,
        "neozeus",
        APP_STATE_FILENAME,
    )
}

/// Resolves the live app-state persistence path from the current environment.
pub fn resolve_app_state_path() -> Option<PathBuf> {
    resolve_app_state_path_with(
        env::var("XDG_STATE_HOME").ok().as_deref(),
        env::var("HOME").ok().as_deref(),
        env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )
}

/// Parses persisted app-state text.
pub fn parse_persisted_app_state(text: &str) -> PersistedAppState {
    let version_line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    match version_line {
        APP_STATE_VERSION_V1 => parse_persisted_app_state_v1(text),
        APP_STATE_VERSION_V2 => parse_persisted_app_state_v2(text),
        APP_STATE_VERSION_V3 | APP_STATE_VERSION_V4 => parse_persisted_app_state_v3(text),
        _ => PersistedAppState::default(),
    }
}

fn parse_persisted_app_state_v1(text: &str) -> PersistedAppState {
    parse_persisted_app_state_with(text, true)
}

fn parse_persisted_app_state_v2(text: &str) -> PersistedAppState {
    parse_persisted_app_state_with(text, false)
}

fn parse_persisted_app_state_v3(text: &str) -> PersistedAppState {
    parse_persisted_app_state_with(text, false)
}

fn finalize_parsed_agent_record(
    persisted: &mut PersistedAppState,
    agent_uid: &mut Option<String>,
    runtime_session_name: &mut Option<String>,
    label: &mut Option<String>,
    kind: &mut Option<PersistedAgentKind>,
    clone_source_session_path: &mut Option<String>,
    recovery_mode: &mut Option<String>,
    recovery_session_path: &mut Option<String>,
    recovery_session_id: &mut Option<String>,
    recovery_cwd: &mut Option<String>,
    recovery_model: &mut Option<String>,
    recovery_profile: &mut Option<String>,
    is_workdir: bool,
    workdir_slug: &mut Option<String>,
    aegis_enabled: bool,
    aegis_prompt_text: &mut Option<String>,
    order_index: &mut Option<u64>,
    last_focused: &mut Option<bool>,
) {
    let has_runtime_hint = runtime_session_name.is_some();
    if let Some(order_index) = order_index.take() {
        if agent_uid.is_some() || has_runtime_hint {
            let kind = kind.take().unwrap_or(PersistedAgentKind::Pi);
            let clone_source_session_path = clone_source_session_path.take();
            let workdir_slug = workdir_slug.take();
            let recovery = match recovery_mode.take().as_deref() {
                Some("pi") => recovery_session_path.take().map(|session_path| {
                    PersistedAgentRecoverySpec::Pi {
                        session_path,
                        cwd: recovery_cwd.take(),
                        is_workdir,
                        workdir_slug: workdir_slug.clone(),
                    }
                }),
                Some("claude") => recovery_session_id.take().and_then(|session_id| {
                    recovery_cwd
                        .take()
                        .map(|cwd| PersistedAgentRecoverySpec::Claude {
                            session_id,
                            cwd,
                            model: recovery_model.take(),
                            profile: recovery_profile.take(),
                        })
                }),
                Some("codex") => recovery_session_id.take().and_then(|session_id| {
                    recovery_cwd
                        .take()
                        .map(|cwd| PersistedAgentRecoverySpec::Codex {
                            session_id,
                            cwd,
                            model: recovery_model.take(),
                            profile: recovery_profile.take(),
                        })
                }),
                _ => match (&kind, clone_source_session_path.as_ref()) {
                    (PersistedAgentKind::Pi, Some(session_path)) => {
                        Some(PersistedAgentRecoverySpec::Pi {
                            session_path: session_path.clone(),
                            cwd: None,
                            is_workdir,
                            workdir_slug: workdir_slug.clone(),
                        })
                    }
                    _ => None,
                },
            };
            persisted.agents.push(PersistedAgentState {
                agent_uid: agent_uid.take(),
                runtime_session_name: runtime_session_name.take(),
                label: label.take(),
                kind,
                recovery,
                clone_source_session_path,
                aegis_enabled,
                aegis_prompt_text: aegis_prompt_text.take(),
                order_index,
                last_focused: last_focused.take().unwrap_or(false),
            });
        }
    }
}

fn parse_persisted_app_state_with(text: &str, legacy_session_name_key: bool) -> PersistedAppState {
    let mut persisted = PersistedAppState::default();
    let mut agent_uid: Option<String> = None;
    let mut runtime_session_name: Option<String> = None;
    let mut label: Option<String> = None;
    let mut kind = None;
    let mut clone_source_session_path: Option<String> = None;
    let mut recovery_mode = None::<String>;
    let mut recovery_session_path: Option<String> = None;
    let mut recovery_session_id: Option<String> = None;
    let mut recovery_cwd: Option<String> = None;
    let mut recovery_model: Option<String> = None;
    let mut recovery_profile: Option<String> = None;
    let mut is_workdir = false;
    let mut workdir_slug = None;
    let mut aegis_enabled = false;
    let mut aegis_prompt_text = None;
    let mut order_index = None;
    let mut last_focused = None;
    let mut in_agent = false;

    for (line_index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line_index == 0 {
            continue;
        }
        match line {
            "[agent]" => {
                in_agent = true;
                agent_uid = None;
                runtime_session_name = None;
                label = None;
                kind = None;
                clone_source_session_path = None;
                recovery_mode = None;
                recovery_session_path = None;
                recovery_session_id = None;
                recovery_cwd = None;
                recovery_model = None;
                recovery_profile = None;
                is_workdir = false;
                workdir_slug = None;
                aegis_enabled = false;
                aegis_prompt_text = None;
                order_index = None;
                last_focused = None;
            }
            "[/agent]" => {
                if in_agent {
                    finalize_parsed_agent_record(
                        &mut persisted,
                        &mut agent_uid,
                        &mut runtime_session_name,
                        &mut label,
                        &mut kind,
                        &mut clone_source_session_path,
                        &mut recovery_mode,
                        &mut recovery_session_path,
                        &mut recovery_session_id,
                        &mut recovery_cwd,
                        &mut recovery_model,
                        &mut recovery_profile,
                        is_workdir,
                        &mut workdir_slug,
                        aegis_enabled,
                        &mut aegis_prompt_text,
                        &mut order_index,
                        &mut last_focused,
                    );
                }
                in_agent = false;
            }
            _ if !in_agent => {}
            _ => {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key {
                    "agent_uid" => {
                        agent_uid = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "runtime_session_name" => {
                        runtime_session_name =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "session_name" if legacy_session_name_key => {
                        runtime_session_name =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "label" => {
                        label = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "kind" => {
                        kind = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                            .and_then(|value| PersistedAgentKind::from_persistence_key(&value))
                    }
                    "clone_source_session_path" => {
                        clone_source_session_path =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "recovery_mode" => {
                        recovery_mode =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "recovery_session_path" => {
                        recovery_session_path =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "recovery_session_id" => {
                        recovery_session_id =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "recovery_cwd" => {
                        recovery_cwd = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "recovery_model" => {
                        recovery_model =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "recovery_profile" => {
                        recovery_profile =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "workdir" => {
                        is_workdir = value.parse::<u8>().ok().is_some_and(|flag| flag != 0)
                    }
                    "workdir_slug" => {
                        workdir_slug = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "aegis_enabled" => {
                        aegis_enabled = value.parse::<u8>().ok().is_some_and(|flag| flag != 0)
                    }
                    "aegis_prompt_text" => {
                        aegis_prompt_text =
                            unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    }
                    "order_index" => order_index = value.parse::<u64>().ok(),
                    "focused" => last_focused = value.parse::<u8>().ok().map(|flag| flag != 0),
                    _ => {}
                }
            }
        }
    }

    if in_agent {
        finalize_parsed_agent_record(
            &mut persisted,
            &mut agent_uid,
            &mut runtime_session_name,
            &mut label,
            &mut kind,
            &mut clone_source_session_path,
            &mut recovery_mode,
            &mut recovery_session_path,
            &mut recovery_session_id,
            &mut recovery_cwd,
            &mut recovery_model,
            &mut recovery_profile,
            is_workdir,
            &mut workdir_slug,
            aegis_enabled,
            &mut aegis_prompt_text,
            &mut order_index,
            &mut last_focused,
        );
    }

    persisted
}
