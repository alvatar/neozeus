use super::{
    persistence::{
        first_non_empty_trimmed_line, non_empty_trimmed_lines_after_header, resolve_state_path_with,
    },
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
    pub paused: bool,
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
    let version_line = first_non_empty_trimmed_line(text);
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

#[derive(Default)]
struct ParsedAgentRecordBuilder {
    agent_uid: Option<String>,
    runtime_session_name: Option<String>,
    label: Option<String>,
    kind: Option<PersistedAgentKind>,
    clone_source_session_path: Option<String>,
    recovery_mode: Option<String>,
    recovery_session_path: Option<String>,
    recovery_session_id: Option<String>,
    recovery_cwd: Option<String>,
    recovery_model: Option<String>,
    recovery_profile: Option<String>,
    is_workdir: bool,
    workdir_slug: Option<String>,
    aegis_enabled: bool,
    aegis_prompt_text: Option<String>,
    paused: bool,
    order_index: Option<u64>,
    last_focused: Option<bool>,
}

impl ParsedAgentRecordBuilder {
    fn apply_field(&mut self, key: &str, value: &str, legacy_session_name_key: bool) {
        match key {
            "agent_uid" => {
                self.agent_uid = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "runtime_session_name" => {
                self.runtime_session_name =
                    unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "session_name" if legacy_session_name_key => {
                self.runtime_session_name =
                    unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "label" => self.label = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES),
            "kind" => {
                self.kind = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
                    .and_then(|value| PersistedAgentKind::from_persistence_key(&value))
            }
            "clone_source_session_path" => {
                self.clone_source_session_path =
                    unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "recovery_mode" => {
                self.recovery_mode = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "recovery_session_path" => {
                self.recovery_session_path =
                    unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "recovery_session_id" => {
                self.recovery_session_id =
                    unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "recovery_cwd" => {
                self.recovery_cwd = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "recovery_model" => {
                self.recovery_model = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "recovery_profile" => {
                self.recovery_profile =
                    unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "workdir" => self.is_workdir = value.parse::<u8>().ok().is_some_and(|flag| flag != 0),
            "workdir_slug" => {
                self.workdir_slug = unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "aegis_enabled" => {
                self.aegis_enabled = value.parse::<u8>().ok().is_some_and(|flag| flag != 0)
            }
            "aegis_prompt_text" => {
                self.aegis_prompt_text =
                    unquote_escaped_string(value, EXTENDED_QUOTED_STRING_ESCAPES)
            }
            "paused" => self.paused = value.parse::<u8>().ok().is_some_and(|flag| flag != 0),
            "order_index" => self.order_index = value.parse::<u64>().ok(),
            "focused" => self.last_focused = value.parse::<u8>().ok().map(|flag| flag != 0),
            _ => {}
        }
    }

    fn into_persisted_agent(self) -> Option<PersistedAgentState> {
        let order_index = self.order_index?;
        if self.agent_uid.is_none() && self.runtime_session_name.is_none() {
            return None;
        }

        let kind = self.kind.unwrap_or(PersistedAgentKind::Pi);
        let recovery = match self.recovery_mode.as_deref() {
            Some("pi") => {
                self.recovery_session_path
                    .map(|session_path| PersistedAgentRecoverySpec::Pi {
                        session_path,
                        cwd: self.recovery_cwd,
                        is_workdir: self.is_workdir,
                        workdir_slug: self.workdir_slug.clone(),
                    })
            }
            Some("claude") => self.recovery_session_id.and_then(|session_id| {
                self.recovery_cwd
                    .map(|cwd| PersistedAgentRecoverySpec::Claude {
                        session_id,
                        cwd,
                        model: self.recovery_model,
                        profile: self.recovery_profile,
                    })
            }),
            Some("codex") => self.recovery_session_id.and_then(|session_id| {
                self.recovery_cwd
                    .map(|cwd| PersistedAgentRecoverySpec::Codex {
                        session_id,
                        cwd,
                        model: self.recovery_model,
                        profile: self.recovery_profile,
                    })
            }),
            _ => match (&kind, self.clone_source_session_path.as_ref()) {
                (PersistedAgentKind::Pi, Some(session_path)) => {
                    Some(PersistedAgentRecoverySpec::Pi {
                        session_path: session_path.clone(),
                        cwd: None,
                        is_workdir: self.is_workdir,
                        workdir_slug: self.workdir_slug.clone(),
                    })
                }
                _ => None,
            },
        };

        Some(PersistedAgentState {
            agent_uid: self.agent_uid,
            runtime_session_name: self.runtime_session_name,
            label: self.label,
            kind,
            recovery,
            clone_source_session_path: self.clone_source_session_path,
            aegis_enabled: self.aegis_enabled,
            aegis_prompt_text: self.aegis_prompt_text,
            paused: self.paused,
            order_index,
            last_focused: self.last_focused.unwrap_or(false),
        })
    }
}

fn flush_parsed_agent_record(
    persisted: &mut PersistedAppState,
    current: &mut Option<ParsedAgentRecordBuilder>,
) {
    if let Some(agent) = current
        .take()
        .and_then(ParsedAgentRecordBuilder::into_persisted_agent)
    {
        persisted.agents.push(agent);
    }
}

fn parse_persisted_app_state_with(text: &str, legacy_session_name_key: bool) -> PersistedAppState {
    let mut persisted = PersistedAppState::default();
    let mut current = None::<ParsedAgentRecordBuilder>;

    for line in non_empty_trimmed_lines_after_header(text) {
        match line {
            "[agent]" => {
                flush_parsed_agent_record(&mut persisted, &mut current);
                current = Some(ParsedAgentRecordBuilder::default());
            }
            "[/agent]" => flush_parsed_agent_record(&mut persisted, &mut current),
            _ => {
                let Some(current) = current.as_mut() else {
                    continue;
                };
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                current.apply_field(key, value, legacy_session_name_key);
            }
        }
    }

    flush_parsed_agent_record(&mut persisted, &mut current);
    persisted
}
