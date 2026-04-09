use crate::{
    aegis::{AegisPolicyStore, AegisRuntimeState, AegisRuntimeStore},
    agents::{AgentCatalog, AgentId},
    app::{mark_app_state_dirty, AppStatePersistenceState},
};
use bevy::prelude::Time;

/// Enables Aegis for one agent using the provided prompt text.
pub(crate) fn enable_aegis(
    agent_id: AgentId,
    prompt_text: &str,
    agent_catalog: &AgentCatalog,
    policy_store: &mut AegisPolicyStore,
    runtime_store: &mut AegisRuntimeStore,
    app_state_persistence: &mut AppStatePersistenceState,
    time: &Time,
) -> Result<(), String> {
    let clean_prompt = prompt_text.trim();
    if clean_prompt.is_empty() {
        return Err("Aegis prompt is required".to_owned());
    }
    let kind = agent_catalog
        .kind(agent_id)
        .ok_or_else(|| "Aegis target agent is missing".to_owned())?;
    if !kind.capabilities().can_message {
        return Err("Aegis requires a message-capable agent".to_owned());
    }
    let agent_uid = agent_catalog
        .uid(agent_id)
        .ok_or_else(|| "Aegis target agent is missing stable identity".to_owned())?;

    let changed = policy_store.enable(agent_uid, clean_prompt.to_owned())
        | runtime_store.set_state(agent_id, AegisRuntimeState::Armed);
    if changed {
        mark_app_state_dirty(app_state_persistence, Some(time));
    }
    Ok(())
}

/// Disables Aegis for one agent and clears its live runtime execution state.
pub(crate) fn disable_aegis(
    agent_id: AgentId,
    agent_catalog: &AgentCatalog,
    policy_store: &mut AegisPolicyStore,
    runtime_store: &mut AegisRuntimeStore,
    app_state_persistence: &mut AppStatePersistenceState,
    time: &Time,
) -> Result<(), String> {
    let agent_uid = agent_catalog
        .uid(agent_id)
        .ok_or_else(|| "Aegis target agent is missing stable identity".to_owned())?;
    let changed = policy_store.disable(agent_uid) | runtime_store.clear(agent_id);
    if changed {
        mark_app_state_dirty(app_state_persistence, Some(time));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        aegis::DEFAULT_AEGIS_PROMPT,
        agents::{AgentCapabilities, AgentKind},
    };

    fn agent_with_uid(kind: AgentKind, label: &str) -> (AgentCatalog, AgentId, String) {
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(Some(label.into()), kind, kind.capabilities());
        let agent_uid = catalog.uid(agent_id).expect("uid should exist").to_owned();
        (catalog, agent_id, agent_uid)
    }

    #[test]
    fn enable_aegis_stores_trimmed_prompt_and_arms_runtime() {
        let (catalog, agent_id, agent_uid) = agent_with_uid(AgentKind::Pi, "alpha");
        let mut policy_store = AegisPolicyStore::default();
        let mut runtime_store = AegisRuntimeStore::default();
        let mut persistence = AppStatePersistenceState::default();
        let time = Time::<()>::default();

        enable_aegis(
            agent_id,
            "  keep going cleanly  ",
            &catalog,
            &mut policy_store,
            &mut runtime_store,
            &mut persistence,
            &time,
        )
        .expect("enable should succeed");

        assert!(policy_store.is_enabled(&agent_uid));
        assert_eq!(
            policy_store.prompt_text(&agent_uid),
            Some("keep going cleanly")
        );
        assert_eq!(
            runtime_store.state(agent_id),
            Some(AegisRuntimeState::Armed)
        );
        assert_eq!(persistence.dirty_since_secs, Some(0.0));
    }

    #[test]
    fn enable_aegis_rejects_empty_prompt() {
        let (catalog, agent_id, _agent_uid) = agent_with_uid(AgentKind::Pi, "alpha");
        let mut policy_store = AegisPolicyStore::default();
        let mut runtime_store = AegisRuntimeStore::default();
        let mut persistence = AppStatePersistenceState::default();
        let time = Time::<()>::default();

        let error = enable_aegis(
            agent_id,
            "   ",
            &catalog,
            &mut policy_store,
            &mut runtime_store,
            &mut persistence,
            &time,
        )
        .expect_err("empty prompt should be rejected");

        assert_eq!(error, "Aegis prompt is required");
        assert!(policy_store.iter().next().is_none());
        assert!(runtime_store.iter().next().is_none());
        assert_eq!(persistence.dirty_since_secs, None);
    }

    #[test]
    fn disable_aegis_clears_enabled_policy_and_runtime_state() {
        let (catalog, agent_id, agent_uid) = agent_with_uid(AgentKind::Pi, "alpha");
        let mut policy_store = AegisPolicyStore::default();
        let mut runtime_store = AegisRuntimeStore::default();
        let mut persistence = AppStatePersistenceState::default();
        let time = Time::<()>::default();
        assert!(policy_store.enable(&agent_uid, DEFAULT_AEGIS_PROMPT.into()));
        assert!(runtime_store.set_state(
            agent_id,
            AegisRuntimeState::PendingDelay { deadline_secs: 5.0 }
        ));

        disable_aegis(
            agent_id,
            &catalog,
            &mut policy_store,
            &mut runtime_store,
            &mut persistence,
            &time,
        )
        .expect("disable should succeed");

        assert_eq!(
            policy_store.prompt_text(&agent_uid),
            Some(DEFAULT_AEGIS_PROMPT)
        );
        assert!(!policy_store.is_enabled(&agent_uid));
        assert!(runtime_store.state(agent_id).is_none());
        assert_eq!(persistence.dirty_since_secs, Some(0.0));
    }

    #[test]
    fn enable_disable_survives_rename_and_runtime_rebinding_via_stable_identity() {
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Pi,
            AgentKind::Pi.capabilities(),
        );
        let agent_uid = catalog.uid(agent_id).expect("uid should exist").to_owned();
        catalog
            .rename_agent(agent_id, "ALPHA-RENAMED".to_owned())
            .expect("rename should succeed");

        let mut policy_store = AegisPolicyStore::default();
        let mut runtime_store = AegisRuntimeStore::default();
        let mut persistence = AppStatePersistenceState::default();
        let time = Time::<()>::default();

        enable_aegis(
            agent_id,
            "keep going",
            &catalog,
            &mut policy_store,
            &mut runtime_store,
            &mut persistence,
            &time,
        )
        .expect("enable should succeed after rename");
        disable_aegis(
            agent_id,
            &catalog,
            &mut policy_store,
            &mut runtime_store,
            &mut persistence,
            &time,
        )
        .expect("disable should succeed after rename");

        assert_eq!(catalog.label(agent_id), Some("ALPHA-RENAMED"));
        assert_eq!(policy_store.prompt_text(&agent_uid), Some("keep going"));
        assert!(!policy_store.is_enabled(&agent_uid));
        assert!(runtime_store.state(agent_id).is_none());
    }

    #[test]
    fn enable_aegis_rejects_non_message_capable_agents() {
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("verifier".into()),
            AgentKind::Verifier,
            AgentCapabilities::verifier_defaults(),
        );
        let mut policy_store = AegisPolicyStore::default();
        let mut runtime_store = AegisRuntimeStore::default();
        let mut persistence = AppStatePersistenceState::default();
        let time = Time::<()>::default();

        let error = enable_aegis(
            agent_id,
            "keep going",
            &catalog,
            &mut policy_store,
            &mut runtime_store,
            &mut persistence,
            &time,
        )
        .expect_err("verifier should be rejected");

        assert_eq!(error, "Aegis requires a message-capable agent");
    }
}
