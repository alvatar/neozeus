use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex},
    app::AppStatePersistenceState,
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        ActiveTerminalContentState, OwnedTmuxSessionStore, TerminalFocusState, TerminalManager,
        TerminalViewState,
    },
};

use super::super::session::{AppSessionState, FocusIntentTarget, VisibilityMode};
use bevy::{prelude::Time, window::RequestRedraw};

fn terminal_visibility_policy(
    visibility_mode: VisibilityMode,
    terminal_id: Option<crate::terminals::TerminalId>,
) -> TerminalVisibilityPolicy {
    match (visibility_mode, terminal_id) {
        (VisibilityMode::FocusedOnly, Some(terminal_id)) => {
            TerminalVisibilityPolicy::Isolate(terminal_id)
        }
        (VisibilityMode::ShowAll, _) | (VisibilityMode::FocusedOnly, None) => {
            TerminalVisibilityPolicy::ShowAll
        }
    }
}

fn reconcile_focus_intent(
    session: &mut AppSessionState,
    agent_catalog: &AgentCatalog,
    _runtime_index: &AgentRuntimeIndex,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
) {
    match &session.focus_intent.target {
        FocusIntentTarget::None => {}
        FocusIntentTarget::Agent(agent_id) => {
            if agent_catalog.uid(*agent_id).is_none() {
                session.focus_intent.clear(VisibilityMode::ShowAll);
            }
        }
        FocusIntentTarget::OwnedTmux(session_uid) => {
            if owned_tmux_sessions.session(session_uid).is_none() {
                session.focus_intent.clear(VisibilityMode::ShowAll);
            }
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "focus intent fans out into selection, active terminal content, focus, view, visibility, and input projections"
)]
pub(crate) fn apply_focus_intent(
    session: &mut AppSessionState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    selection: &mut crate::hud::AgentListSelection,
    active_terminal_content: &mut ActiveTerminalContentState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
) {
    reconcile_focus_intent(session, agent_catalog, runtime_index, owned_tmux_sessions);

    match &session.focus_intent.target {
        FocusIntentTarget::None => {
            *selection = crate::hud::AgentListSelection::None;
            active_terminal_content.clear();
            let _ = focus_state.clear_active_terminal();
            #[cfg(test)]
            terminal_manager.replace_test_focus_state(focus_state);
            view_state.focus_terminal(None);
            visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
        }
        FocusIntentTarget::Agent(agent_id) => {
            let terminal_id = runtime_index.primary_terminal(*agent_id);
            *selection = crate::hud::AgentListSelection::Agent(*agent_id);
            active_terminal_content.clear();
            if let Some(terminal_id) = terminal_id {
                focus_state.focus_terminal(terminal_manager, terminal_id);
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                view_state.focus_terminal(Some(terminal_id));
            } else {
                let _ = focus_state.clear_active_terminal();
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                view_state.focus_terminal(None);
            }
            visibility_state.policy =
                terminal_visibility_policy(session.visibility_mode(), terminal_id);
        }
        FocusIntentTarget::OwnedTmux(session_uid) => {
            *selection = crate::hud::AgentListSelection::OwnedTmux(session_uid.clone());
            let owner_terminal_id = owned_tmux_sessions
                .session(session_uid)
                .and_then(|owned_tmux| agent_catalog.find_by_uid(&owned_tmux.owner_agent_uid))
                .and_then(|agent_id| runtime_index.primary_terminal(agent_id));
            active_terminal_content.select_owned_tmux(session_uid.clone(), owner_terminal_id);
            if let Some(terminal_id) = owner_terminal_id {
                focus_state.focus_terminal(terminal_manager, terminal_id);
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                view_state.focus_terminal(Some(terminal_id));
            } else {
                let _ = focus_state.clear_active_terminal();
                #[cfg(test)]
                terminal_manager.replace_test_focus_state(focus_state);
                view_state.focus_terminal(None);
            }
            visibility_state.policy =
                terminal_visibility_policy(session.visibility_mode(), owner_terminal_id);
        }
    }

    input_capture.reconcile_direct_terminal_input(focus_state.active_id());
}

#[allow(
    clippy::too_many_arguments,
    reason = "focus agent updates focus intent plus all runtime-facing mirrors"
)]
pub(crate) fn focus_agent_without_persist(
    agent_id: AgentId,
    visibility_mode: VisibilityMode,
    session: &mut AppSessionState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    selection: &mut crate::hud::AgentListSelection,
    active_terminal_content: &mut ActiveTerminalContentState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    session.focus_intent.focus_agent(agent_id, visibility_mode);
    apply_focus_intent(
        session,
        agent_catalog,
        runtime_index,
        owned_tmux_sessions,
        selection,
        active_terminal_content,
        terminal_manager,
        focus_state,
        input_capture,
        view_state,
        visibility_state,
    );
    redraws.write(RequestRedraw);
}

#[allow(
    clippy::too_many_arguments,
    reason = "focus agent updates focus intent plus all runtime-facing mirrors"
)]
/// Focuses agent.
pub(crate) fn focus_agent(
    agent_id: AgentId,
    visibility_mode: VisibilityMode,
    session: &mut AppSessionState,
    agent_catalog: &AgentCatalog,
    runtime_index: &AgentRuntimeIndex,
    owned_tmux_sessions: &OwnedTmuxSessionStore,
    selection: &mut crate::hud::AgentListSelection,
    active_terminal_content: &mut ActiveTerminalContentState,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    input_capture: &mut HudInputCaptureState,
    app_state_persistence: &mut AppStatePersistenceState,
    view_state: &mut TerminalViewState,
    visibility_state: &mut TerminalVisibilityState,
    time: &Time,
    redraws: &mut bevy::prelude::MessageWriter<RequestRedraw>,
) {
    focus_agent_without_persist(
        agent_id,
        visibility_mode,
        session,
        agent_catalog,
        runtime_index,
        owned_tmux_sessions,
        selection,
        active_terminal_content,
        terminal_manager,
        focus_state,
        input_capture,
        view_state,
        visibility_state,
        redraws,
    );
    let _ = (app_state_persistence, time);
}

#[cfg(test)]
mod tests {
    use super::apply_focus_intent;
    use crate::{
        agents::{AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex},
        app::{AppSessionState, VisibilityMode},
        hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
        terminals::{
            ActiveTerminalContentState, OwnedTmuxSessionInfo, OwnedTmuxSessionStore,
            TerminalFocusState, TerminalManager, TerminalViewState,
        },
        tests::test_bridge,
    };
    use bevy::prelude::Vec2;

    struct FocusFixture {
        agent_catalog: AgentCatalog,
        runtime_index: AgentRuntimeIndex,
        terminal_manager: TerminalManager,
        focus_state: TerminalFocusState,
        selection: crate::hud::AgentListSelection,
        active_terminal_content: ActiveTerminalContentState,
        input_capture: HudInputCaptureState,
        view_state: TerminalViewState,
        visibility_state: TerminalVisibilityState,
        owned_tmux_sessions: OwnedTmuxSessionStore,
        agent_a: AgentId,
        agent_b: AgentId,
        terminal_a: crate::terminals::TerminalId,
        terminal_b: crate::terminals::TerminalId,
    }

    fn focus_fixture() -> FocusFixture {
        let (bridge_a, _) = test_bridge();
        let (bridge_b, _) = test_bridge();
        let mut terminal_manager = TerminalManager::default();
        let terminal_a =
            terminal_manager.create_terminal_with_session(bridge_a, "neozeus-session-a".into());
        let terminal_b =
            terminal_manager.create_terminal_with_session(bridge_b, "neozeus-session-b".into());

        let mut agent_catalog = AgentCatalog::default();
        let agent_a = agent_catalog.create_agent(
            Some("alpha".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let agent_b = agent_catalog.create_agent(
            Some("beta".into()),
            AgentKind::Terminal,
            AgentKind::Terminal.capabilities(),
        );
        let owner_uid = agent_catalog.uid(agent_b).unwrap().to_owned();
        let mut runtime_index = AgentRuntimeIndex::default();
        runtime_index.link_terminal(agent_a, terminal_a, "neozeus-session-a".into(), None);
        runtime_index.link_terminal(agent_b, terminal_b, "neozeus-session-b".into(), None);

        let mut view_state = TerminalViewState::default();
        view_state.focus_terminal(Some(terminal_a));
        view_state.apply_offset_delta(Some(terminal_a), Vec2::new(4.0, 2.0));
        view_state.focus_terminal(Some(terminal_b));
        view_state.apply_offset_delta(Some(terminal_b), Vec2::new(-3.0, 1.0));
        view_state.focus_terminal(Some(terminal_a));

        let mut owned_tmux_sessions = OwnedTmuxSessionStore::default();
        let _ = owned_tmux_sessions.replace_sessions(vec![OwnedTmuxSessionInfo {
            session_uid: "tmux-1".into(),
            owner_agent_uid: owner_uid,
            tmux_name: "neozeus-tmux-1".into(),
            display_name: "tmux child".into(),
            cwd: "/tmp/demo".into(),
            attached: false,
            created_unix: 1,
        }]);

        FocusFixture {
            agent_catalog,
            runtime_index,
            terminal_manager,
            focus_state: TerminalFocusState::default(),
            selection: crate::hud::AgentListSelection::None,
            active_terminal_content: ActiveTerminalContentState::default(),
            input_capture: HudInputCaptureState::default(),
            view_state,
            visibility_state: TerminalVisibilityState::default(),
            owned_tmux_sessions,
            agent_a,
            agent_b,
            terminal_a,
            terminal_b,
        }
    }

    #[test]
    fn focusing_agent_updates_all_dependent_projections_coherently() {
        let mut fixture = focus_fixture();
        let mut session = AppSessionState::default();
        session
            .focus_intent
            .focus_agent(fixture.agent_b, VisibilityMode::FocusedOnly);

        apply_focus_intent(
            &mut session,
            &fixture.agent_catalog,
            &fixture.runtime_index,
            &fixture.owned_tmux_sessions,
            &mut fixture.selection,
            &mut fixture.active_terminal_content,
            &mut fixture.terminal_manager,
            &mut fixture.focus_state,
            &mut fixture.input_capture,
            &mut fixture.view_state,
            &mut fixture.visibility_state,
        );

        assert_eq!(
            fixture.selection,
            crate::hud::AgentListSelection::Agent(fixture.agent_b)
        );
        assert_eq!(fixture.focus_state.active_id(), Some(fixture.terminal_b));
        assert_eq!(
            fixture
                .active_terminal_content
                .selected_owned_tmux_session_uid(),
            None
        );
        assert_eq!(
            fixture.visibility_state.policy,
            TerminalVisibilityPolicy::Isolate(fixture.terminal_b)
        );
        assert_eq!(fixture.view_state.offset, Vec2::new(-3.0, 1.0));
        assert_eq!(fixture.input_capture.direct_input_terminal, None);
    }

    #[test]
    fn clearing_focus_clears_all_dependent_projections_coherently() {
        let mut fixture = focus_fixture();
        let mut session = AppSessionState::default();
        session.focus_intent.focus_owned_tmux("tmux-1".into());
        apply_focus_intent(
            &mut session,
            &fixture.agent_catalog,
            &fixture.runtime_index,
            &fixture.owned_tmux_sessions,
            &mut fixture.selection,
            &mut fixture.active_terminal_content,
            &mut fixture.terminal_manager,
            &mut fixture.focus_state,
            &mut fixture.input_capture,
            &mut fixture.view_state,
            &mut fixture.visibility_state,
        );
        fixture.input_capture.direct_input_terminal = fixture.focus_state.active_id();
        session.focus_intent.clear(VisibilityMode::ShowAll);

        apply_focus_intent(
            &mut session,
            &fixture.agent_catalog,
            &fixture.runtime_index,
            &fixture.owned_tmux_sessions,
            &mut fixture.selection,
            &mut fixture.active_terminal_content,
            &mut fixture.terminal_manager,
            &mut fixture.focus_state,
            &mut fixture.input_capture,
            &mut fixture.view_state,
            &mut fixture.visibility_state,
        );

        assert_eq!(fixture.selection, crate::hud::AgentListSelection::None);
        assert_eq!(fixture.focus_state.active_id(), None);
        assert_eq!(
            fixture
                .active_terminal_content
                .selected_owned_tmux_session_uid(),
            None
        );
        assert_eq!(
            fixture.visibility_state.policy,
            TerminalVisibilityPolicy::ShowAll
        );
        assert_eq!(fixture.view_state.offset, Vec2::ZERO);
        assert_eq!(fixture.input_capture.direct_input_terminal, None);
    }

    #[test]
    fn owned_tmux_selection_projects_consistently_into_focus_and_selection_state() {
        let mut fixture = focus_fixture();
        let mut session = AppSessionState::default();
        session.focus_intent.focus_owned_tmux("tmux-1".into());

        apply_focus_intent(
            &mut session,
            &fixture.agent_catalog,
            &fixture.runtime_index,
            &fixture.owned_tmux_sessions,
            &mut fixture.selection,
            &mut fixture.active_terminal_content,
            &mut fixture.terminal_manager,
            &mut fixture.focus_state,
            &mut fixture.input_capture,
            &mut fixture.view_state,
            &mut fixture.visibility_state,
        );

        assert_eq!(
            fixture.selection,
            crate::hud::AgentListSelection::OwnedTmux("tmux-1".into())
        );
        assert_eq!(
            fixture
                .active_terminal_content
                .selected_owned_tmux_session_uid(),
            Some("tmux-1")
        );
        assert_eq!(fixture.focus_state.active_id(), Some(fixture.terminal_b));
        assert_eq!(
            fixture.visibility_state.policy,
            TerminalVisibilityPolicy::ShowAll
        );
    }

    #[test]
    fn direct_terminal_input_reconciles_with_projected_active_terminal() {
        let mut fixture = focus_fixture();
        let mut session = AppSessionState::default();
        session
            .focus_intent
            .focus_agent(fixture.agent_a, VisibilityMode::FocusedOnly);
        fixture.input_capture.direct_input_terminal = Some(fixture.terminal_b);

        apply_focus_intent(
            &mut session,
            &fixture.agent_catalog,
            &fixture.runtime_index,
            &fixture.owned_tmux_sessions,
            &mut fixture.selection,
            &mut fixture.active_terminal_content,
            &mut fixture.terminal_manager,
            &mut fixture.focus_state,
            &mut fixture.input_capture,
            &mut fixture.view_state,
            &mut fixture.visibility_state,
        );

        assert_eq!(fixture.focus_state.active_id(), Some(fixture.terminal_a));
        assert_eq!(fixture.input_capture.direct_input_terminal, None);
    }
}
