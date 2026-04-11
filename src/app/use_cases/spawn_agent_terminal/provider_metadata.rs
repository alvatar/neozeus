use super::*;

pub(super) type CodexCaptureSeed = (String, std::collections::BTreeSet<String>);

pub(super) fn prepare_provider_metadata_capture(
    kind: AgentKind,
    launch: &AgentLaunchSpec,
    working_directory: Option<&str>,
) -> Option<CodexCaptureSeed> {
    if kind == AgentKind::Codex && launch.metadata.recovery.is_none() {
        return crate::shared::pi_session_files::resolve_session_cwd(working_directory)
            .ok()
            .map(|cwd| {
                (
                    cwd,
                    crate::shared::codex_state::codex_thread_ids().unwrap_or_default(),
                )
            });
    }

    None
}

pub(super) fn apply_provider_metadata_capture(
    agent_catalog: &mut AgentCatalog,
    agent_id: AgentId,
    codex_capture: Option<CodexCaptureSeed>,
) {
    let Some((cwd, known_thread_ids)) = codex_capture else {
        return;
    };

    match crate::shared::codex_state::wait_for_new_codex_thread_id(
        &cwd,
        &known_thread_ids,
        Duration::from_secs(3),
    ) {
        Ok(Some(session_id)) => {
            let _ = agent_catalog.set_recovery_spec(
                agent_id,
                Some(AgentRecoverySpec::Codex {
                    session_id,
                    cwd,
                    model: None,
                    profile: None,
                }),
            );
        }
        Ok(None) => match crate::shared::codex_state::latest_codex_thread_id_for_cwd(&cwd) {
            Ok(Some(session_id)) => {
                let _ = agent_catalog.set_recovery_spec(
                    agent_id,
                    Some(AgentRecoverySpec::Codex {
                        session_id,
                        cwd,
                        model: None,
                        profile: None,
                    }),
                );
            }
            Ok(None) => append_debug_log(format!(
                "codex recovery capture timed out for agent {}",
                agent_id.0
            )),
            Err(error) => append_debug_log(format!(
                "codex recovery fallback failed for agent {}: {error}",
                agent_id.0
            )),
        },
        Err(error) => append_debug_log(format!(
            "codex recovery capture failed for agent {}: {error}",
            agent_id.0
        )),
    }
}
