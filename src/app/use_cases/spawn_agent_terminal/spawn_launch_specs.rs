use super::*;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentLaunchSpec {
    pub(crate) startup_command: Option<String>,
    pub(crate) metadata: AgentMetadata,
}

static NEXT_PROVIDER_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) fn generate_provider_session_id() -> String {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = NEXT_PROVIDER_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let mixed = now_nanos ^ counter;
    let tail = (now_nanos.wrapping_add(counter)) & 0xffff_ffff_ffff;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        ((mixed >> 96) & 0xffff_ffff) as u32,
        ((mixed >> 80) & 0xffff) as u16,
        ((mixed >> 64) & 0xffff) as u16,
        ((mixed >> 48) & 0xffff) as u16,
        tail as u64,
    )
}

pub(super) fn build_agent_launch_spec(
    kind: AgentKind,
    working_directory: Option<&str>,
) -> Result<AgentLaunchSpec, String> {
    if kind == AgentKind::Pi {
        let session_path = make_new_session_path(working_directory)?;
        return Ok(pi_launch_spec_for_session_path(session_path, false, None));
    }

    if kind == AgentKind::Claude {
        let session_id = generate_provider_session_id();
        let cwd = crate::shared::pi_session_files::resolve_session_cwd(working_directory)?;
        return Ok(AgentLaunchSpec {
            startup_command: Some(format!("claude --session-id {}", shell_quote(&session_id))),
            metadata: AgentMetadata {
                clone_source_session_path: None,
                recovery: Some(AgentRecoverySpec::Claude {
                    session_id,
                    cwd,
                    model: None,
                    profile: None,
                }),
            },
        });
    }

    Ok(AgentLaunchSpec {
        startup_command: kind.bootstrap_command().map(str::to_owned),
        metadata: AgentMetadata::default(),
    })
}

pub(crate) fn claude_fork_launch_spec(
    parent_session_id: &str,
    child_session_id: String,
    cwd: &str,
    model: Option<String>,
    profile: Option<String>,
) -> AgentLaunchSpec {
    let mut command = format!(
        "claude --resume {} --fork-session --session-id {}",
        shell_quote(parent_session_id),
        shell_quote(&child_session_id)
    );
    if let Some(model) = model.as_deref() {
        command.push_str(" --model ");
        command.push_str(&shell_quote(model));
    }
    if let Some(profile) = profile.as_deref() {
        command.push_str(" -p ");
        command.push_str(&shell_quote(profile));
    }
    AgentLaunchSpec {
        startup_command: Some(command),
        metadata: AgentMetadata {
            clone_source_session_path: None,
            recovery: Some(AgentRecoverySpec::Claude {
                session_id: child_session_id,
                cwd: cwd.to_owned(),
                model,
                profile,
            }),
        },
    }
}

pub(crate) fn codex_fork_launch_spec(
    parent_session_id: &str,
    cwd: &str,
    model: Option<String>,
    profile: Option<String>,
) -> AgentLaunchSpec {
    let mut command = format!(
        "codex fork {} -C {}",
        shell_quote(parent_session_id),
        shell_quote(cwd)
    );
    if let Some(model) = model.as_deref() {
        command.push_str(" -m ");
        command.push_str(&shell_quote(model));
    }
    if let Some(profile) = profile.as_deref() {
        command.push_str(" -p ");
        command.push_str(&shell_quote(profile));
    }
    AgentLaunchSpec {
        startup_command: Some(command),
        metadata: AgentMetadata::default(),
    }
}

pub(crate) fn launch_spec_for_recovery_spec(recovery: &AgentRecoverySpec) -> AgentLaunchSpec {
    match recovery {
        AgentRecoverySpec::Pi {
            session_path,
            is_workdir,
            workdir_slug,
            ..
        } => {
            pi_launch_spec_for_session_path(session_path.clone(), *is_workdir, workdir_slug.clone())
        }
        AgentRecoverySpec::Claude {
            session_id,
            cwd: _,
            model,
            profile,
        } => {
            let mut command = format!("claude --resume {}", shell_quote(session_id));
            if let Some(model) = model {
                command.push_str(" --model ");
                command.push_str(&shell_quote(model));
            }
            if let Some(profile) = profile {
                command.push_str(" -p ");
                command.push_str(&shell_quote(profile));
            }
            AgentLaunchSpec {
                startup_command: Some(command),
                metadata: AgentMetadata {
                    clone_source_session_path: None,
                    recovery: Some(recovery.clone()),
                },
            }
        }
        AgentRecoverySpec::Codex {
            session_id,
            cwd,
            model,
            profile,
        } => {
            let mut command = format!("codex resume {}", shell_quote(session_id));
            if let Some(model) = model {
                command.push_str(" -m ");
                command.push_str(&shell_quote(model));
            }
            if let Some(profile) = profile {
                command.push_str(" -p ");
                command.push_str(&shell_quote(profile));
            }
            command.push_str(" -C ");
            command.push_str(&shell_quote(cwd));
            AgentLaunchSpec {
                startup_command: Some(command),
                metadata: AgentMetadata {
                    clone_source_session_path: None,
                    recovery: Some(recovery.clone()),
                },
            }
        }
    }
}

pub(crate) fn pi_launch_spec_for_session_path(
    session_path: String,
    is_workdir: bool,
    workdir_slug: Option<String>,
) -> AgentLaunchSpec {
    let cwd = crate::shared::pi_session_files::read_session_header(&session_path)
        .map(|header| header.cwd)
        .unwrap_or_default();
    AgentLaunchSpec {
        startup_command: Some(format!("pi --session {}", shell_quote(&session_path))),
        metadata: AgentMetadata {
            clone_source_session_path: Some(session_path.clone()),
            recovery: Some(AgentRecoverySpec::Pi {
                session_path,
                cwd,
                is_workdir,
                workdir_slug,
            }),
        },
    }
}
