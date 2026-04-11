use crate::{
    agents::{AgentCatalog, AgentId, AgentRuntimeIndex},
    app::session::{AppSessionState, FocusIntentTarget, VisibilityMode},
    hud::{HudInputCaptureState, TerminalVisibilityPolicy, TerminalVisibilityState},
    terminals::{
        ActiveTerminalContentState, OwnedTmuxSessionStore, TerminalFocusState, TerminalManager,
        TerminalViewState,
    },
};
use bevy::{prelude::MessageWriter, window::RequestRedraw};

pub(crate) struct FocusProjectionContext<'a> {
    pub(crate) agent_catalog: &'a AgentCatalog,
    pub(crate) runtime_index: &'a AgentRuntimeIndex,
    pub(crate) owned_tmux_sessions: &'a OwnedTmuxSessionStore,
    pub(crate) selection: &'a mut crate::hud::AgentListSelection,
    pub(crate) active_terminal_content: &'a mut ActiveTerminalContentState,
    pub(crate) terminal_manager: &'a mut TerminalManager,
    pub(crate) focus_state: &'a mut TerminalFocusState,
    pub(crate) input_capture: &'a mut HudInputCaptureState,
    pub(crate) view_state: &'a mut TerminalViewState,
    pub(crate) visibility_state: &'a mut TerminalVisibilityState,
}

pub(crate) struct FocusMutationContext<'a, 'w> {
    pub(crate) session: &'a mut AppSessionState,
    pub(crate) projection: FocusProjectionContext<'a>,
    pub(crate) redraws: &'a mut MessageWriter<'w, RequestRedraw>,
}

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

fn reconcile_focus_intent(session: &mut AppSessionState, ctx: &FocusProjectionContext<'_>) {
    match &session.focus_intent.target {
        FocusIntentTarget::None => {}
        FocusIntentTarget::Agent(agent_id) => {
            if ctx.agent_catalog.uid(*agent_id).is_none() {
                session.focus_intent.clear(VisibilityMode::ShowAll);
            }
        }
        FocusIntentTarget::Terminal(terminal_id) => {
            if !ctx.terminal_manager.contains_terminal(*terminal_id) {
                session.focus_intent.clear(VisibilityMode::ShowAll);
            }
        }
        FocusIntentTarget::OwnedTmux(session_uid) => {
            if ctx.owned_tmux_sessions.session(session_uid).is_none() {
                session.focus_intent.clear(VisibilityMode::ShowAll);
            }
        }
    }
}

fn project_terminal_focus(
    terminal_id: Option<crate::terminals::TerminalId>,
    terminal_manager: &mut TerminalManager,
    focus_state: &mut TerminalFocusState,
    view_state: &mut TerminalViewState,
) {
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
}

pub(crate) fn project_focus_intent(
    session: &mut AppSessionState,
    ctx: &mut FocusProjectionContext<'_>,
) {
    reconcile_focus_intent(session, ctx);

    let focused_terminal_id = match &session.focus_intent.target {
        FocusIntentTarget::None => {
            *ctx.selection = crate::hud::AgentListSelection::None;
            ctx.active_terminal_content.clear();
            None
        }
        FocusIntentTarget::Agent(agent_id) => {
            *ctx.selection = crate::hud::AgentListSelection::Agent(*agent_id);
            ctx.active_terminal_content.clear();
            ctx.runtime_index.primary_terminal(*agent_id)
        }
        FocusIntentTarget::Terminal(terminal_id) => {
            *ctx.selection = crate::hud::AgentListSelection::None;
            ctx.active_terminal_content.clear();
            Some(*terminal_id)
        }
        FocusIntentTarget::OwnedTmux(session_uid) => {
            *ctx.selection = crate::hud::AgentListSelection::OwnedTmux(session_uid.clone());
            let owner_terminal_id = ctx
                .owned_tmux_sessions
                .session(session_uid)
                .and_then(|owned_tmux| ctx.agent_catalog.find_by_uid(&owned_tmux.owner_agent_uid))
                .and_then(|agent_id| ctx.runtime_index.primary_terminal(agent_id));
            ctx.active_terminal_content
                .select_owned_tmux(session_uid.clone(), owner_terminal_id);
            owner_terminal_id
        }
    };

    project_terminal_focus(
        focused_terminal_id,
        ctx.terminal_manager,
        ctx.focus_state,
        ctx.view_state,
    );
    ctx.visibility_state.policy =
        terminal_visibility_policy(session.visibility_mode(), focused_terminal_id);
    ctx.input_capture
        .reconcile_direct_terminal_input(ctx.focus_state.active_id());
}

fn redraw(ctx: &mut FocusMutationContext<'_, '_>) {
    ctx.redraws.write(RequestRedraw);
}

pub(crate) fn clear_focus_without_persist(
    visibility_mode: VisibilityMode,
    ctx: &mut FocusMutationContext<'_, '_>,
) {
    ctx.session.focus_intent.clear(visibility_mode);
    project_focus_intent(ctx.session, &mut ctx.projection);
    redraw(ctx);
}

pub(crate) fn focus_agent_without_persist(
    agent_id: AgentId,
    visibility_mode: VisibilityMode,
    ctx: &mut FocusMutationContext<'_, '_>,
) {
    ctx.session
        .focus_intent
        .focus_agent(agent_id, visibility_mode);
    project_focus_intent(ctx.session, &mut ctx.projection);
    redraw(ctx);
}

pub(crate) fn focus_terminal_without_persist(
    terminal_id: crate::terminals::TerminalId,
    visibility_mode: VisibilityMode,
    ctx: &mut FocusMutationContext<'_, '_>,
) {
    ctx.session
        .focus_intent
        .focus_terminal(terminal_id, visibility_mode);
    project_focus_intent(ctx.session, &mut ctx.projection);
    redraw(ctx);
}

pub(crate) fn focus_owned_tmux_without_persist(
    session_uid: &str,
    ctx: &mut FocusMutationContext<'_, '_>,
) {
    ctx.session
        .focus_intent
        .focus_owned_tmux(session_uid.to_owned());
    project_focus_intent(ctx.session, &mut ctx.projection);
    redraw(ctx);
}

/// Focuses agent.
pub(crate) fn focus_agent(
    agent_id: AgentId,
    visibility_mode: VisibilityMode,
    ctx: &mut FocusMutationContext<'_, '_>,
) {
    focus_agent_without_persist(agent_id, visibility_mode, ctx);
}

#[cfg(test)]
mod tests {
    use super::{project_focus_intent, FocusProjectionContext};
    use crate::{
        agents::{AgentCatalog, AgentId, AgentKind, AgentRuntimeIndex},
        app::{session::FocusIntentTarget, AppSessionState, VisibilityMode},
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

    impl FocusFixture {
        fn projection_context(&mut self) -> FocusProjectionContext<'_> {
            FocusProjectionContext {
                agent_catalog: &self.agent_catalog,
                runtime_index: &self.runtime_index,
                owned_tmux_sessions: &self.owned_tmux_sessions,
                selection: &mut self.selection,
                active_terminal_content: &mut self.active_terminal_content,
                terminal_manager: &mut self.terminal_manager,
                focus_state: &mut self.focus_state,
                input_capture: &mut self.input_capture,
                view_state: &mut self.view_state,
                visibility_state: &mut self.visibility_state,
            }
        }
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

        project_focus_intent(&mut session, &mut fixture.projection_context());

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
    fn focusing_terminal_without_agent_keeps_selection_none_but_projects_terminal_focus() {
        let mut fixture = focus_fixture();
        let mut session = AppSessionState::default();
        session
            .focus_intent
            .focus_terminal(fixture.terminal_a, VisibilityMode::FocusedOnly);

        project_focus_intent(&mut session, &mut fixture.projection_context());

        assert_eq!(fixture.selection, crate::hud::AgentListSelection::None);
        assert_eq!(fixture.focus_state.active_id(), Some(fixture.terminal_a));
        assert_eq!(fixture.view_state.offset, Vec2::new(4.0, 2.0));
        assert_eq!(
            fixture.visibility_state.policy,
            TerminalVisibilityPolicy::Isolate(fixture.terminal_a)
        );
    }

    #[test]
    fn clearing_focus_clears_all_dependent_projections_coherently() {
        let mut fixture = focus_fixture();
        let mut session = AppSessionState::default();
        session.focus_intent.focus_owned_tmux("tmux-1".into());
        project_focus_intent(&mut session, &mut fixture.projection_context());
        fixture.input_capture.direct_input_terminal = fixture.focus_state.active_id();
        session.focus_intent.clear(VisibilityMode::ShowAll);

        project_focus_intent(&mut session, &mut fixture.projection_context());

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

        project_focus_intent(&mut session, &mut fixture.projection_context());

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

        project_focus_intent(&mut session, &mut fixture.projection_context());

        assert_eq!(fixture.focus_state.active_id(), Some(fixture.terminal_a));
        assert_eq!(fixture.input_capture.direct_input_terminal, None);
    }

    #[test]
    fn missing_terminal_focus_intent_reconciles_back_to_clear_state() {
        let mut fixture = focus_fixture();
        let mut session = AppSessionState::default();
        session.focus_intent.focus_terminal(
            crate::terminals::TerminalId(999),
            VisibilityMode::FocusedOnly,
        );

        project_focus_intent(&mut session, &mut fixture.projection_context());

        assert_eq!(session.focus_intent.target, FocusIntentTarget::None);
        assert_eq!(fixture.selection, crate::hud::AgentListSelection::None);
        assert_eq!(fixture.focus_state.active_id(), None);
        assert_eq!(
            fixture.visibility_state.policy,
            TerminalVisibilityPolicy::ShowAll
        );
    }
}
