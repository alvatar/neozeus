use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex, AgentStatus, AgentStatusStore},
    app::{send_outbound_message, OutboundMessageSource},
    conversations::{
        mark_conversations_dirty, ConversationPersistenceState, ConversationStore,
        MessageTransportAdapter,
    },
    terminals::TerminalRuntimeSpawner,
};
use bevy::prelude::{Res, ResMut, Resource, Time};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const AEGIS_INITIAL_DELAY_SECS: f32 = 5.0;
pub(crate) const AEGIS_POST_CHECK_DELAY_SECS: f32 = 20.0;
pub(crate) const DEFAULT_AEGIS_PROMPT: &str = "If the work is not finalized as agree, continue without stopping. Do not do hacks, sidestep the problem or play tricks for acceptance. Work should be executed cleanly and with adherence to the highest quality standard";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AegisPolicy {
    pub(crate) enabled: bool,
    pub(crate) prompt_text: String,
}

impl Default for AegisPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            prompt_text: DEFAULT_AEGIS_PROMPT.to_owned(),
        }
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AegisPolicyStore {
    policies_by_agent_uid: BTreeMap<String, AegisPolicy>,
}

impl AegisPolicyStore {
    pub(crate) fn policy(&self, agent_uid: &str) -> Option<&AegisPolicy> {
        self.policies_by_agent_uid.get(agent_uid)
    }

    pub(crate) fn enable(&mut self, agent_uid: &str, prompt_text: String) -> bool {
        let policy = self
            .policies_by_agent_uid
            .entry(agent_uid.to_owned())
            .or_default();
        if policy.enabled && policy.prompt_text == prompt_text {
            return false;
        }
        policy.enabled = true;
        policy.prompt_text = prompt_text;
        true
    }

    pub(crate) fn disable(&mut self, agent_uid: &str) -> bool {
        let Some(policy) = self.policies_by_agent_uid.get_mut(agent_uid) else {
            return false;
        };
        if !policy.enabled {
            return false;
        }
        policy.enabled = false;
        true
    }

    pub(crate) fn upsert_disabled_prompt(&mut self, agent_uid: &str, prompt_text: String) -> bool {
        let policy = self
            .policies_by_agent_uid
            .entry(agent_uid.to_owned())
            .or_default();
        if !policy.enabled && policy.prompt_text == prompt_text {
            return false;
        }
        policy.prompt_text = prompt_text;
        true
    }

    pub(crate) fn remove(&mut self, agent_uid: &str) -> bool {
        self.policies_by_agent_uid.remove(agent_uid).is_some()
    }

    pub(crate) fn is_enabled(&self, agent_uid: &str) -> bool {
        self.policy(agent_uid).is_some_and(|policy| policy.enabled)
    }

    pub(crate) fn prompt_text(&self, agent_uid: &str) -> Option<&str> {
        self.policy(agent_uid)
            .map(|policy| policy.prompt_text.as_str())
    }

    #[cfg(test)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &AegisPolicy)> {
        self.policies_by_agent_uid
            .iter()
            .map(|entry| (entry.0.as_str(), entry.1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AegisRuntimeState {
    Armed,
    PendingDelay { deadline_secs: f32 },
    PostCheck { deadline_secs: f32 },
    Halted,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AegisRuntimeStore {
    states_by_agent: BTreeMap<AgentId, AegisRuntimeState>,
}

impl AegisRuntimeStore {
    pub(crate) fn state(&self, agent_id: AgentId) -> Option<AegisRuntimeState> {
        self.states_by_agent.get(&agent_id).copied()
    }

    pub(crate) fn set_state(&mut self, agent_id: AgentId, state: AegisRuntimeState) -> bool {
        if self.states_by_agent.get(&agent_id) == Some(&state) {
            return false;
        }
        self.states_by_agent.insert(agent_id, state);
        true
    }

    pub(crate) fn clear(&mut self, agent_id: AgentId) -> bool {
        self.states_by_agent.remove(&agent_id).is_some()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (AgentId, AegisRuntimeState)> + '_ {
        self.states_by_agent
            .iter()
            .map(|(agent_id, state)| (*agent_id, *state))
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AegisStatusTracker {
    previous_by_agent: BTreeMap<AgentId, AgentStatus>,
}

fn enabled_policy_for_agent<'a>(
    agent_catalog: &'a AgentCatalog,
    policy_store: &'a AegisPolicyStore,
    agent_id: AgentId,
) -> Option<&'a AegisPolicy> {
    let agent_uid = agent_catalog.uid(agent_id)?;
    let policy = policy_store.policy(agent_uid)?;
    policy.enabled.then_some(policy)
}

#[allow(
    clippy::too_many_arguments,
    reason = "Aegis automation spans normalized status, policy/runtime state, conversations, and runtime delivery"
)]
pub(crate) fn advance_aegis_runtime(
    time: Res<Time>,
    agent_catalog: Res<AgentCatalog>,
    status_store: Res<AgentStatusStore>,
    policy_store: Res<AegisPolicyStore>,
    mut runtime_store: ResMut<AegisRuntimeStore>,
    mut status_tracker: ResMut<AegisStatusTracker>,
    mut conversations: ResMut<ConversationStore>,
    mut conversation_persistence: ResMut<ConversationPersistenceState>,
    transport: Res<MessageTransportAdapter>,
    runtime_index: Res<AgentRuntimeIndex>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
) {
    let now_secs = time.elapsed_secs();
    let enabled_agents = agent_catalog
        .iter()
        .map(|(agent_id, _)| agent_id)
        .filter(|agent_id| {
            enabled_policy_for_agent(&agent_catalog, &policy_store, *agent_id).is_some()
        })
        .collect::<BTreeSet<_>>();

    let stale_runtime = runtime_store
        .iter()
        .map(|(agent_id, _)| agent_id)
        .filter(|agent_id| !enabled_agents.contains(agent_id))
        .collect::<Vec<_>>();
    for agent_id in stale_runtime {
        let _ = runtime_store.clear(agent_id);
    }
    status_tracker
        .previous_by_agent
        .retain(|agent_id, _| enabled_agents.contains(agent_id));

    for agent_id in enabled_agents {
        let current_status = status_store.status(agent_id);
        let existing_state = runtime_store.state(agent_id);
        let previous_status = status_tracker
            .previous_by_agent
            .insert(agent_id, current_status);
        let mut current_state = existing_state.unwrap_or(AegisRuntimeState::Armed);
        if existing_state.is_none() {
            let _ = runtime_store.set_state(agent_id, AegisRuntimeState::Armed);
            let _ = status_tracker
                .previous_by_agent
                .insert(agent_id, current_status);
            continue;
        }

        if matches!(current_state, AegisRuntimeState::Halted)
            && current_status == AgentStatus::Working
        {
            let _ = runtime_store.set_state(agent_id, AegisRuntimeState::Armed);
            continue;
        }

        match current_state {
            AegisRuntimeState::Armed => {
                if previous_status == Some(AgentStatus::Working)
                    && current_status == AgentStatus::Idle
                {
                    let _ = runtime_store.set_state(
                        agent_id,
                        AegisRuntimeState::PendingDelay {
                            deadline_secs: now_secs + AEGIS_INITIAL_DELAY_SECS,
                        },
                    );
                }
            }
            AegisRuntimeState::PendingDelay { deadline_secs } => {
                if current_status == AgentStatus::Working {
                    let _ = runtime_store.set_state(agent_id, AegisRuntimeState::Armed);
                    continue;
                }
                if current_status != AgentStatus::Idle || now_secs < deadline_secs {
                    continue;
                }
                let Some(policy) =
                    enabled_policy_for_agent(&agent_catalog, &policy_store, agent_id)
                else {
                    let _ = runtime_store.clear(agent_id);
                    continue;
                };
                let result = send_outbound_message(
                    conversations.ensure_conversation(agent_id),
                    agent_id,
                    policy.prompt_text.clone(),
                    OutboundMessageSource::Aegis,
                    &mut conversations,
                    &transport,
                    &runtime_index,
                    &runtime_spawner,
                );
                mark_conversations_dirty(&mut conversation_persistence, Some(&time));
                current_state = if result.is_ok() {
                    AegisRuntimeState::PostCheck {
                        deadline_secs: now_secs + AEGIS_POST_CHECK_DELAY_SECS,
                    }
                } else {
                    AegisRuntimeState::Halted
                };
                let _ = runtime_store.set_state(agent_id, current_state);
            }
            AegisRuntimeState::PostCheck { deadline_secs } => {
                if current_status == AgentStatus::Working {
                    let _ = runtime_store.set_state(agent_id, AegisRuntimeState::Armed);
                } else if current_status == AgentStatus::Idle && now_secs >= deadline_secs {
                    let _ = runtime_store.set_state(agent_id, AegisRuntimeState::Halted);
                }
            }
            AegisRuntimeState::Halted => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agents::AgentKind,
        conversations::MessageDeliveryState,
        tests::{fake_runtime_spawner, FakeDaemonClient},
    };
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    use std::{sync::Arc, time::Duration};

    #[test]
    fn default_aegis_stores_are_empty() {
        let policy_store = AegisPolicyStore::default();
        let runtime_store = AegisRuntimeStore::default();

        assert!(policy_store.iter().next().is_none());
        assert!(runtime_store.iter().next().is_none());
    }

    #[test]
    fn policy_and_runtime_stores_are_independent() {
        let mut policy_store = AegisPolicyStore::default();
        let mut runtime_store = AegisRuntimeStore::default();

        assert!(policy_store.enable("agent-uid-1", "custom prompt".into()));
        assert!(runtime_store.set_state(
            AgentId(7),
            AegisRuntimeState::PendingDelay { deadline_secs: 5.0 }
        ));

        assert_eq!(
            policy_store.prompt_text("agent-uid-1"),
            Some("custom prompt")
        );
        assert_eq!(
            runtime_store.state(AgentId(7)),
            Some(AegisRuntimeState::PendingDelay { deadline_secs: 5.0 })
        );
        assert!(policy_store.policy("agent-uid-7").is_none());
        assert!(runtime_store.state(AgentId(1)).is_none());
    }

    #[test]
    fn disabling_aegis_removes_runtime_state_but_preserves_prompt_policy() {
        let mut policy_store = AegisPolicyStore::default();
        let mut runtime_store = AegisRuntimeStore::default();

        assert!(policy_store.enable("agent-uid-1", "custom prompt".into()));
        assert!(runtime_store.set_state(AgentId(1), AegisRuntimeState::Armed));

        assert!(policy_store.disable("agent-uid-1"));
        assert!(runtime_store.clear(AgentId(1)));

        let policy = policy_store
            .policy("agent-uid-1")
            .expect("policy should still exist");
        assert!(!policy.enabled);
        assert_eq!(policy.prompt_text, "custom prompt");
        assert!(runtime_store.state(AgentId(1)).is_none());
    }

    #[test]
    fn default_policy_uses_requested_default_prompt() {
        assert_eq!(AegisPolicy::default().prompt_text, DEFAULT_AEGIS_PROMPT);
    }

    fn aegis_runtime_world() -> (World, AgentId, String, Arc<FakeDaemonClient>) {
        let client = Arc::new(FakeDaemonClient::default());
        client.set_session_runtime(
            "neozeus-session-a",
            crate::terminals::TerminalRuntimeState::running("ready"),
        );
        let mut world = World::default();
        let mut catalog = AgentCatalog::default();
        let agent_id = catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Pi,
            AgentKind::Pi.capabilities(),
        );
        let agent_uid = catalog.uid(agent_id).expect("uid should exist").to_owned();
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(
            agent_id,
            crate::terminals::TerminalId(1),
            "neozeus-session-a".into(),
            None,
        );
        let mut policy_store = AegisPolicyStore::default();
        assert!(policy_store.enable(&agent_uid, "continue cleanly".into()));
        let mut runtime_store = AegisRuntimeStore::default();
        assert!(runtime_store.set_state(agent_id, AegisRuntimeState::Armed));
        world.insert_resource(Time::<()>::default());
        world.insert_resource(catalog);
        world.insert_resource(AgentStatusStore::default());
        world.insert_resource(policy_store);
        world.insert_resource(runtime_store);
        world.insert_resource(AegisStatusTracker::default());
        world.insert_resource(ConversationStore::default());
        world.insert_resource(ConversationPersistenceState::default());
        world.insert_resource(MessageTransportAdapter);
        world.insert_resource(runtime_index);
        world.insert_resource(fake_runtime_spawner(client.clone()));
        (world, agent_id, agent_uid, client)
    }

    #[test]
    fn working_to_idle_arms_delay_once() {
        let (mut world, agent_id, _agent_uid, client) = aegis_runtime_world();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        world.run_system_once(advance_aegis_runtime).unwrap();
        let state = world
            .resource::<AegisRuntimeStore>()
            .state(agent_id)
            .expect("state should exist");
        assert_eq!(
            state,
            AegisRuntimeState::PendingDelay {
                deadline_secs: AEGIS_INITIAL_DELAY_SECS,
            }
        );
        world.run_system_once(advance_aegis_runtime).unwrap();
        assert_eq!(
            client.sent_commands.lock().unwrap().len(),
            0,
            "repeated idle polls must not send before the deadline"
        );
    }

    #[test]
    fn return_to_working_during_delay_rearms_without_send() {
        let (mut world, agent_id, _agent_uid, client) = aegis_runtime_world();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();

        assert_eq!(
            world.resource::<AegisRuntimeStore>().state(agent_id),
            Some(AegisRuntimeState::Armed)
        );
        assert!(client.sent_commands.lock().unwrap().is_empty());
    }

    #[test]
    fn successful_send_enters_post_check_and_sends_once_per_stop_cycle() {
        let (mut world, agent_id, _agent_uid, client) = aegis_runtime_world();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(5));
        world.run_system_once(advance_aegis_runtime).unwrap();

        assert_eq!(client.sent_commands.lock().unwrap().len(), 1);
        assert_eq!(
            world.resource::<AegisRuntimeStore>().state(agent_id),
            Some(AegisRuntimeState::PostCheck {
                deadline_secs: AEGIS_INITIAL_DELAY_SECS + AEGIS_POST_CHECK_DELAY_SECS,
            })
        );
        assert_eq!(
            world
                .resource::<ConversationPersistenceState>()
                .dirty_since_secs,
            Some(5.0)
        );

        world.run_system_once(advance_aegis_runtime).unwrap();
        assert_eq!(client.sent_commands.lock().unwrap().len(), 1);
    }

    #[test]
    fn post_check_working_rearms() {
        let (mut world, agent_id, _agent_uid, _client) = aegis_runtime_world();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(5));
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();

        assert_eq!(
            world.resource::<AegisRuntimeStore>().state(agent_id),
            Some(AegisRuntimeState::Armed)
        );
    }

    #[test]
    fn post_check_idle_halts() {
        let (mut world, agent_id, _agent_uid, _client) = aegis_runtime_world();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(5));
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(20));
        world.run_system_once(advance_aegis_runtime).unwrap();

        assert_eq!(
            world.resource::<AegisRuntimeStore>().state(agent_id),
            Some(AegisRuntimeState::Halted)
        );
    }

    #[test]
    fn halted_cycle_rearms_on_next_real_work_cycle() {
        let (mut world, agent_id, _agent_uid, _client) = aegis_runtime_world();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(5));
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(20));
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        world.run_system_once(advance_aegis_runtime).unwrap();

        assert_eq!(
            world.resource::<AegisRuntimeStore>().state(agent_id),
            Some(AegisRuntimeState::PendingDelay {
                deadline_secs: 30.0,
            })
        );
    }

    #[test]
    fn send_failure_halts_runtime_state_and_marks_failed_message() {
        let (mut world, agent_id, _agent_uid, client) = aegis_runtime_world();
        *client.fail_send.lock().unwrap() = true;
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        world.run_system_once(advance_aegis_runtime).unwrap();
        world
            .resource_mut::<Time<()>>()
            .advance_by(Duration::from_secs(5));
        world.run_system_once(advance_aegis_runtime).unwrap();

        assert_eq!(
            world.resource::<AegisRuntimeStore>().state(agent_id),
            Some(AegisRuntimeState::Halted)
        );
        let conversation_id = world
            .resource::<ConversationStore>()
            .conversation_for_agent(agent_id)
            .expect("conversation should exist after attempted send");
        assert_eq!(
            world
                .resource::<ConversationStore>()
                .messages_for(conversation_id),
            vec![(
                "continue cleanly".into(),
                MessageDeliveryState::Failed("send failed".into())
            )]
        );
    }

    #[test]
    fn reenable_while_idle_does_not_reuse_stale_working_transition() {
        let (mut world, agent_id, agent_uid, _client) = aegis_runtime_world();
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Working);
        world.run_system_once(advance_aegis_runtime).unwrap();

        {
            let mut policy_store = world.resource_mut::<AegisPolicyStore>();
            assert!(policy_store.disable(&agent_uid));
        }
        {
            let mut runtime_store = world.resource_mut::<AegisRuntimeStore>();
            assert!(runtime_store.clear(agent_id));
        }
        world
            .resource_mut::<AgentStatusStore>()
            .set_status_for_tests(agent_id, AgentStatus::Idle);
        {
            let mut policy_store = world.resource_mut::<AegisPolicyStore>();
            assert!(policy_store.enable(&agent_uid, "continue cleanly".into()));
        }

        world.run_system_once(advance_aegis_runtime).unwrap();

        assert_eq!(
            world.resource::<AegisRuntimeStore>().state(agent_id),
            Some(AegisRuntimeState::Armed)
        );
        assert!(world
            .resource::<ConversationStore>()
            .conversation_for_agent(agent_id)
            .is_none());
    }
}
