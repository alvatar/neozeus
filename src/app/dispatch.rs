use crate::{
    agents::{AgentCatalog, AgentKind, AgentRuntimeIndex},
    app::{mark_app_state_dirty, AppStatePersistenceState},
    conversations::{
        mark_conversations_dirty, AgentTaskStore, ConversationPersistenceState, ConversationStore,
        MessageTransportAdapter,
    },
    hud::{HudInputCaptureState, HudLayoutState, TerminalVisibilityState},
    terminals::{
        append_debug_log, mark_terminal_notes_dirty, ActiveTerminalContentState,
        OwnedTmuxSessionStore, TerminalFocusState, TerminalManager, TerminalNotesState,
        TerminalPresentationStore, TerminalRuntimeSpawner, TerminalViewState,
        PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
    },
};

use super::{
    commands::{
        AgentCommand, AppCommand, ComposerCommand, OwnedTmuxCommand, TaskCommand as AppTaskCommand,
        WidgetCommand,
    },
    session::{AppSessionState, VisibilityMode},
    use_cases,
};
use bevy::{ecs::system::SystemParam, prelude::*, window::RequestRedraw};

#[allow(
    clippy::too_many_arguments,
    reason = "agent cleanup spans domain stores, persistence dirtiness, and selection state together"
)]
fn purge_removed_agent_state(
    time: &Time,
    agent_id: crate::agents::AgentId,
    agent_catalog: &mut AgentCatalog,
    app_session: &mut AppSessionState,
    selection: &mut crate::hud::AgentListSelection,
    task_store: &mut AgentTaskStore,
    conversations: &mut ConversationStore,
    conversation_persistence: &mut ConversationPersistenceState,
    notes_state: &mut TerminalNotesState,
    app_state_persistence: &mut AppStatePersistenceState,
) {
    let agent_uid = agent_catalog.uid(agent_id).map(str::to_owned);
    let _ = agent_catalog.remove(agent_id);
    app_session.composer.unbind_agent(agent_id);
    if app_session.focus_intent.selected_agent() == Some(agent_id) {
        app_session.focus_intent.clear(VisibilityMode::ShowAll);
        *selection = crate::hud::AgentListSelection::None;
    }

    if task_store.remove_agent(agent_id) {
        if let Some(agent_uid) = agent_uid.as_deref() {
            if notes_state.remove_note_text_by_agent_uid(agent_uid) {
                mark_terminal_notes_dirty(notes_state, Some(time));
            }
        }
    }
    if conversations.remove_agent(agent_id) {
        mark_conversations_dirty(conversation_persistence, Some(time));
    }
    mark_app_state_dirty(app_state_persistence, Some(time));
}

/// Reconciles the new agent domain from terminal/runtime state that may still be created through
/// legacy startup or verifier paths.
///
/// The sync is intentionally conservative: missing agent records are created, stale links are
/// removed, and runtime lifecycle is refreshed. It does not overwrite explicit catalog labels once
/// an agent exists, and it only clears row selection when the selected agent disappears.
#[derive(SystemParam)]
pub(crate) struct FocusProjectionContext<'w> {
    selection: ResMut<'w, crate::hud::AgentListSelection>,
    focus_state: Option<ResMut<'w, TerminalFocusState>>,
    input_capture: Option<ResMut<'w, HudInputCaptureState>>,
    visibility_state: Option<ResMut<'w, TerminalVisibilityState>>,
    view_state: Option<ResMut<'w, TerminalViewState>>,
    owned_tmux_sessions: Option<Res<'w, OwnedTmuxSessionStore>>,
    active_terminal_content: Option<ResMut<'w, ActiveTerminalContentState>>,
    terminal_manager: ResMut<'w, TerminalManager>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal reconciliation now owns cleanup parity across agent, task, conversation, and note stores"
)]
pub(crate) fn sync_agents_from_terminals(
    time: Res<Time>,
    mut agent_catalog: ResMut<AgentCatalog>,
    mut runtime_index: ResMut<AgentRuntimeIndex>,
    mut app_session: ResMut<AppSessionState>,
    mut task_store: ResMut<AgentTaskStore>,
    mut conversations: ResMut<ConversationStore>,
    mut conversation_persistence: ResMut<ConversationPersistenceState>,
    mut notes_state: ResMut<TerminalNotesState>,
    mut app_state_persistence: ResMut<AppStatePersistenceState>,
    mut focus: FocusProjectionContext,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let existing_terminals = focus
        .terminal_manager
        .terminal_ids()
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let stale_terminals = runtime_index
        .terminal_to_agent
        .keys()
        .copied()
        .filter(|terminal_id| !existing_terminals.contains(terminal_id))
        .collect::<Vec<_>>();
    let mut removed_any_terminal = false;
    for terminal_id in stale_terminals {
        if let Some(agent_id) = runtime_index.remove_terminal(terminal_id) {
            removed_any_terminal = true;
            purge_removed_agent_state(
                &time,
                agent_id,
                &mut agent_catalog,
                &mut app_session,
                &mut focus.selection,
                &mut task_store,
                &mut conversations,
                &mut conversation_persistence,
                &mut notes_state,
                &mut app_state_persistence,
            );
        }
    }

    for terminal_id in focus.terminal_manager.terminal_ids().iter().copied() {
        let Some(terminal) = focus.terminal_manager.get(terminal_id) else {
            continue;
        };
        let _agent_id = runtime_index
            .agent_for_terminal(terminal_id)
            .unwrap_or_else(|| {
                let kind = if terminal.session_name.starts_with(VERIFIER_SESSION_PREFIX) {
                    AgentKind::Verifier
                } else {
                    AgentKind::Terminal
                };
                let capabilities = kind.capabilities();
                let agent_id = agent_catalog.create_agent(None, kind, capabilities);
                runtime_index.link_terminal(
                    agent_id,
                    terminal_id,
                    terminal.session_name.clone(),
                    Some(&terminal.snapshot.runtime),
                );
                agent_id
            });
        runtime_index.update_runtime(terminal_id, &terminal.snapshot.runtime);
    }

    if removed_any_terminal {
        let default_owned_tmux_sessions = OwnedTmuxSessionStore::default();
        let mut default_active_terminal_content = ActiveTerminalContentState::default();
        let mut default_focus_state = TerminalFocusState::default();
        let mut default_input_capture = HudInputCaptureState::default();
        let mut default_view_state = TerminalViewState::default();
        let mut default_visibility_state = TerminalVisibilityState::default();
        use_cases::apply_focus_intent(
            &mut app_session,
            &agent_catalog,
            &runtime_index,
            focus
                .owned_tmux_sessions
                .as_deref()
                .unwrap_or(&default_owned_tmux_sessions),
            &mut focus.selection,
            focus
                .active_terminal_content
                .as_deref_mut()
                .unwrap_or(&mut default_active_terminal_content),
            &mut focus.terminal_manager,
            focus
                .focus_state
                .as_deref_mut()
                .unwrap_or(&mut default_focus_state),
            focus
                .input_capture
                .as_deref_mut()
                .unwrap_or(&mut default_input_capture),
            focus
                .view_state
                .as_deref_mut()
                .unwrap_or(&mut default_view_state),
            focus
                .visibility_state
                .as_deref_mut()
                .unwrap_or(&mut default_visibility_state),
        );
    }
}

/// Refreshes the open task editor text after task-store mutations.
fn refresh_open_task_editor(
    app_session: &mut AppSessionState,
    agent_id: crate::agents::AgentId,
    task_store: &AgentTaskStore,
) {
    if !matches!(
        app_session.composer.session.as_ref().map(|session| &session.mode),
        Some(crate::composer::ComposerMode::TaskEdit {
            agent_id: open_agent_id,
        }) if *open_agent_id == agent_id
    ) {
        return;
    }
    app_session
        .composer
        .task_editor
        .load_text(task_store.text(agent_id).unwrap_or_default());
}

#[derive(SystemParam)]
pub(super) struct AppCommandContext<'w> {
    time: Res<'w, Time>,
    agent_catalog: ResMut<'w, AgentCatalog>,
    runtime_index: ResMut<'w, AgentRuntimeIndex>,
    app_session: ResMut<'w, AppSessionState>,
    terminal_manager: ResMut<'w, TerminalManager>,
    focus_state: ResMut<'w, TerminalFocusState>,
    runtime_spawner: Res<'w, TerminalRuntimeSpawner>,
    owned_tmux_sessions: ResMut<'w, OwnedTmuxSessionStore>,
    active_terminal_content: ResMut<'w, ActiveTerminalContentState>,
    input_capture: ResMut<'w, HudInputCaptureState>,
    layout_state: ResMut<'w, HudLayoutState>,
    selection: ResMut<'w, crate::hud::AgentListSelection>,
    task_store: ResMut<'w, AgentTaskStore>,
    conversations: ResMut<'w, ConversationStore>,
    conversation_persistence: ResMut<'w, ConversationPersistenceState>,
    notes_state: ResMut<'w, TerminalNotesState>,
    transport: Res<'w, MessageTransportAdapter>,
    app_state_persistence: ResMut<'w, AppStatePersistenceState>,
    visibility_state: ResMut<'w, TerminalVisibilityState>,
    view_state: ResMut<'w, TerminalViewState>,
    presentation_store: Option<ResMut<'w, TerminalPresentationStore>>,
    redraws: MessageWriter<'w, RequestRedraw>,
}

fn apply_agent_command(command: &AgentCommand, ctx: &mut AppCommandContext) {
    match command {
        AgentCommand::Create {
            label,
            kind,
            working_directory,
        } => {
            if let Err(error) = use_cases::spawn_agent_terminal(
                &mut ctx.agent_catalog,
                &mut ctx.runtime_index,
                &mut ctx.app_session,
                &mut ctx.selection,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &ctx.owned_tmux_sessions,
                &mut ctx.active_terminal_content,
                &ctx.runtime_spawner,
                &mut ctx.input_capture,
                &mut ctx.app_state_persistence,
                &mut ctx.visibility_state,
                &mut ctx.view_state,
                ctx.presentation_store.as_deref_mut(),
                &ctx.time,
                PERSISTENT_SESSION_PREFIX,
                *kind,
                label.clone(),
                Some(working_directory.as_str()),
                &mut ctx.redraws,
            ) {
                ctx.app_session.create_agent_dialog.error = Some(error.clone());
                append_debug_log(format!("create agent failed: {error}"));
                ctx.redraws.write(RequestRedraw);
            } else {
                ctx.app_session.create_agent_dialog.close();
            }
        }
        AgentCommand::Rename { agent_id, label } => {
            match ctx.agent_catalog.validate_rename_label(*agent_id, label) {
                Ok(label) => {
                    if let Some(session_name) = ctx.runtime_index.session_name(*agent_id) {
                        if let Err(error) = ctx
                            .runtime_spawner
                            .update_session_metadata_label(session_name, Some(label.as_str()))
                        {
                            ctx.app_session.rename_agent_dialog.error = Some(error.clone());
                            append_debug_log(format!(
                                "rename agent failed for {session_name}: {error}"
                            ));
                            ctx.redraws.write(RequestRedraw);
                            return;
                        }
                    }
                    match ctx.agent_catalog.rename_agent(*agent_id, label) {
                        Ok(()) => {
                            ctx.app_session.rename_agent_dialog.close();
                            mark_app_state_dirty(&mut ctx.app_state_persistence, Some(&ctx.time));
                            ctx.redraws.write(RequestRedraw);
                        }
                        Err(error) => {
                            ctx.app_session.rename_agent_dialog.error = Some(error.clone());
                            append_debug_log(format!("rename agent failed: {error}"));
                            ctx.redraws.write(RequestRedraw);
                        }
                    }
                }
                Err(error) => {
                    ctx.app_session.rename_agent_dialog.error = Some(error.clone());
                    append_debug_log(format!("rename agent failed: {error}"));
                    ctx.redraws.write(RequestRedraw);
                }
            }
        }
        AgentCommand::Clone {
            source_agent_id,
            label,
            workdir,
        } => {
            if let Err(error) = use_cases::clone_pi_agent(
                &mut ctx.agent_catalog,
                &mut ctx.runtime_index,
                &mut ctx.app_session,
                &mut ctx.selection,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &ctx.owned_tmux_sessions,
                &mut ctx.active_terminal_content,
                &ctx.runtime_spawner,
                &mut ctx.input_capture,
                &mut ctx.app_state_persistence,
                &mut ctx.visibility_state,
                &mut ctx.view_state,
                &ctx.time,
                *source_agent_id,
                label,
                *workdir,
                &mut ctx.redraws,
            ) {
                ctx.app_session.clone_agent_dialog.error = Some(error.clone());
                append_debug_log(format!("clone Pi agent failed: {error}"));
                ctx.redraws.write(RequestRedraw);
            } else {
                ctx.app_session.clone_agent_dialog.close();
            }
        }
        AgentCommand::Focus(agent_id) => {
            use_cases::focus_agent(
                *agent_id,
                VisibilityMode::ShowAll,
                &mut ctx.app_session,
                &ctx.agent_catalog,
                &ctx.runtime_index,
                &ctx.owned_tmux_sessions,
                &mut ctx.selection,
                &mut ctx.active_terminal_content,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &mut ctx.input_capture,
                &mut ctx.app_state_persistence,
                &mut ctx.view_state,
                &mut ctx.visibility_state,
                &ctx.time,
                &mut ctx.redraws,
            );
        }
        AgentCommand::Inspect(agent_id) => {
            use_cases::focus_agent(
                *agent_id,
                VisibilityMode::FocusedOnly,
                &mut ctx.app_session,
                &ctx.agent_catalog,
                &ctx.runtime_index,
                &ctx.owned_tmux_sessions,
                &mut ctx.selection,
                &mut ctx.active_terminal_content,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &mut ctx.input_capture,
                &mut ctx.app_state_persistence,
                &mut ctx.view_state,
                &mut ctx.visibility_state,
                &ctx.time,
                &mut ctx.redraws,
            );
        }
        AgentCommand::Reorder {
            agent_id,
            target_index,
        } => {
            if ctx.agent_catalog.move_to_index(*agent_id, *target_index) {
                mark_app_state_dirty(&mut ctx.app_state_persistence, Some(&ctx.time));
                ctx.redraws.write(RequestRedraw);
            }
        }
        AgentCommand::ClearFocus => {
            ctx.app_session.focus_intent.clear(VisibilityMode::ShowAll);
            use_cases::apply_focus_intent(
                &mut ctx.app_session,
                &ctx.agent_catalog,
                &ctx.runtime_index,
                &ctx.owned_tmux_sessions,
                &mut ctx.selection,
                &mut ctx.active_terminal_content,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &mut ctx.input_capture,
                &mut ctx.view_state,
                &mut ctx.visibility_state,
            );
            mark_app_state_dirty(&mut ctx.app_state_persistence, Some(&ctx.time));
            ctx.redraws.write(RequestRedraw);
        }
        AgentCommand::KillSelected => {
            let agent_id = match ctx.app_session.focus_intent.selected_agent() {
                Some(agent_id) => agent_id,
                None => match *ctx.selection {
                    crate::hud::AgentListSelection::Agent(agent_id) => {
                        let visibility_mode = ctx.app_session.visibility_mode();
                        ctx.app_session
                            .focus_intent
                            .focus_agent(agent_id, visibility_mode);
                        agent_id
                    }
                    crate::hud::AgentListSelection::None
                    | crate::hud::AgentListSelection::OwnedTmux(_) => return,
                },
            };
            ctx.active_terminal_content.clear();
            if let Err(error) = use_cases::kill_selected_agent(
                agent_id,
                &ctx.time,
                &mut ctx.agent_catalog,
                &mut ctx.runtime_index,
                &mut ctx.app_session,
                &mut ctx.selection,
                &mut ctx.task_store,
                &mut ctx.conversations,
                &mut ctx.conversation_persistence,
                &mut ctx.notes_state,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &ctx.runtime_spawner,
                &ctx.owned_tmux_sessions,
                &mut ctx.active_terminal_content,
                &mut ctx.input_capture,
                &mut ctx.app_state_persistence,
                &mut ctx.visibility_state,
                &mut ctx.view_state,
                &mut ctx.redraws,
            ) {
                append_debug_log(format!("kill selected agent failed: {error}"));
            }
        }
    }
}

fn apply_owned_tmux_command(command: &OwnedTmuxCommand, ctx: &mut AppCommandContext) {
    match command {
        OwnedTmuxCommand::Select { session_uid } => {
            use_cases::select_owned_tmux(
                session_uid,
                &mut ctx.selection,
                &ctx.agent_catalog,
                &ctx.runtime_index,
                &mut ctx.app_session,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &mut ctx.input_capture,
                &mut ctx.view_state,
                &mut ctx.visibility_state,
                &ctx.runtime_spawner,
                &mut ctx.owned_tmux_sessions,
                &mut ctx.active_terminal_content,
                &mut ctx.redraws,
            );
        }
        OwnedTmuxCommand::ClearSelection => {
            ctx.app_session.focus_intent.clear(VisibilityMode::ShowAll);
            use_cases::apply_focus_intent(
                &mut ctx.app_session,
                &ctx.agent_catalog,
                &ctx.runtime_index,
                &ctx.owned_tmux_sessions,
                &mut ctx.selection,
                &mut ctx.active_terminal_content,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &mut ctx.input_capture,
                &mut ctx.view_state,
                &mut ctx.visibility_state,
            );
            ctx.redraws.write(RequestRedraw);
        }
        OwnedTmuxCommand::KillSelected => {
            use_cases::kill_selected_owned_tmux(
                &mut ctx.app_session,
                &ctx.agent_catalog,
                &ctx.runtime_index,
                &mut ctx.terminal_manager,
                &mut ctx.focus_state,
                &mut ctx.input_capture,
                &mut ctx.view_state,
                &mut ctx.visibility_state,
                &ctx.runtime_spawner,
                &mut ctx.selection,
                &mut ctx.owned_tmux_sessions,
                &mut ctx.active_terminal_content,
                &mut ctx.redraws,
            );
            if let Some(error) = ctx.active_terminal_content.last_error() {
                append_debug_log(format!("kill owned tmux failed: {error}"));
            }
        }
    }
}

fn apply_task_command(command: &AppTaskCommand, ctx: &mut AppCommandContext) {
    match command {
        AppTaskCommand::Append { agent_id, text } => {
            if use_cases::append_task(*agent_id, text, &mut ctx.task_store) {
                refresh_open_task_editor(&mut ctx.app_session, *agent_id, &ctx.task_store);
                ctx.redraws.write(RequestRedraw);
            }
        }
        AppTaskCommand::Prepend { agent_id, text } => {
            if use_cases::prepend_task(*agent_id, text, &mut ctx.task_store) {
                refresh_open_task_editor(&mut ctx.app_session, *agent_id, &ctx.task_store);
                ctx.redraws.write(RequestRedraw);
            }
        }
        AppTaskCommand::ClearDone { agent_id } => {
            if use_cases::clear_done_tasks(*agent_id, &mut ctx.task_store) {
                refresh_open_task_editor(&mut ctx.app_session, *agent_id, &ctx.task_store);
                ctx.redraws.write(RequestRedraw);
            }
        }
        AppTaskCommand::ConsumeNext { agent_id } => {
            if use_cases::consume_next_task(
                *agent_id,
                &mut ctx.task_store,
                ctx.runtime_index.primary_terminal(*agent_id),
                &ctx.terminal_manager,
            ) {
                refresh_open_task_editor(&mut ctx.app_session, *agent_id, &ctx.task_store);
                ctx.redraws.write(RequestRedraw);
            }
        }
    }
}

fn apply_composer_command(command: &ComposerCommand, ctx: &mut AppCommandContext) {
    match command {
        ComposerCommand::Open(request) => {
            use_cases::open_composer(
                request,
                &mut ctx.app_session,
                &mut ctx.input_capture,
                &ctx.runtime_index,
                &ctx.task_store,
                &mut ctx.redraws,
            );
        }
        ComposerCommand::Submit => {
            use_cases::submit_composer(
                &mut ctx.app_session,
                &mut ctx.conversations,
                &mut ctx.conversation_persistence,
                &mut ctx.task_store,
                &ctx.runtime_index,
                &ctx.runtime_spawner,
                &ctx.transport,
                &ctx.time,
                &mut ctx.redraws,
            );
        }
        ComposerCommand::Cancel => {
            use_cases::cancel_composer(&mut ctx.app_session, &mut ctx.redraws);
        }
    }
}

fn apply_widget_command(command: &WidgetCommand, ctx: &mut AppCommandContext) {
    match command {
        WidgetCommand::Toggle(widget_id) => {
            use_cases::toggle_widget(*widget_id, &mut ctx.layout_state);
            ctx.redraws.write(RequestRedraw);
        }
        WidgetCommand::Reset(widget_id) => {
            use_cases::reset_widget(*widget_id, &mut ctx.layout_state);
            ctx.redraws.write(RequestRedraw);
        }
    }
}

/// Applies queued app-level commands through the explicit use-case layer.
///
/// The top-level dispatcher now routes commands into narrower domain executors so the per-domain
/// mutation policy stays local instead of accumulating inside one giant match body.
pub(super) fn apply_app_commands(
    mut app_commands: MessageReader<AppCommand>,
    mut ctx: AppCommandContext,
) {
    for command in app_commands.read() {
        match command {
            AppCommand::Agent(command) => apply_agent_command(command, &mut ctx),
            AppCommand::OwnedTmux(command) => apply_owned_tmux_command(command, &mut ctx),
            AppCommand::Task(command) => apply_task_command(command, &mut ctx),
            AppCommand::Composer(command) => apply_composer_command(command, &mut ctx),
            AppCommand::Widget(command) => apply_widget_command(command, &mut ctx),
        }
    }
}
