use crate::{
    agents::{AgentCatalog, AgentKind, AgentRuntimeIndex},
    app::{mark_app_state_dirty, AppStatePersistenceState},
    conversations::{
        AgentTaskStore, ConversationPersistenceState, ConversationStore, MessageTransportAdapter,
    },
    hud::{HudInputCaptureState, HudLayoutState, TerminalVisibilityState},
    startup::StartupLoadingState,
    terminals::{
        append_debug_log, TerminalFocusState, TerminalManager, TerminalRuntimeSpawner,
        TerminalViewState, PERSISTENT_SESSION_PREFIX, VERIFIER_SESSION_PREFIX,
    },
};

use super::{
    commands::{
        AgentCommand, AppCommand, ComposerCommand, TaskCommand as AppTaskCommand, WidgetCommand,
    },
    session::{AppSessionState, VisibilityMode},
    use_cases,
};
use bevy::{ecs::system::SystemParam, prelude::*, window::RequestRedraw};

/// Reconciles the new agent domain from terminal/runtime state that may still be created through
/// legacy startup or verifier paths.
///
/// The sync is intentionally conservative: missing agent records are created, stale links are
/// removed, runtime lifecycle is refreshed, and active-agent selection is mirrored from the current
/// terminal focus. It does not overwrite explicit catalog labels once an agent exists.
pub(crate) fn sync_agents_from_terminals(
    mut agent_catalog: ResMut<AgentCatalog>,
    mut runtime_index: ResMut<AgentRuntimeIndex>,
    mut app_session: ResMut<AppSessionState>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
) {
    // Rebuild the derived or projected state from the authoritative resources in one pass so partial updates cannot drift.
    let existing_terminals = terminal_manager
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
    for terminal_id in stale_terminals {
        if let Some(agent_id) = runtime_index.remove_terminal(terminal_id) {
            let _ = agent_catalog.remove(agent_id);
            app_session.composer.unbind_agent(agent_id);
            if app_session.active_agent == Some(agent_id) {
                app_session.active_agent = None;
            }
        }
    }

    for terminal_id in terminal_manager.terminal_ids().iter().copied() {
        let Some(terminal) = terminal_manager.get(terminal_id) else {
            continue;
        };
        let _agent_id = runtime_index
            .agent_for_terminal(terminal_id)
            .unwrap_or_else(|| {
                let kind = if terminal.session_name.starts_with(VERIFIER_SESSION_PREFIX) {
                    AgentKind::Verifier
                } else {
                    AgentKind::Pi
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

    app_session.active_agent = focus_state
        .active_id()
        .and_then(|terminal_id| runtime_index.agent_for_terminal(terminal_id));
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
    input_capture: ResMut<'w, HudInputCaptureState>,
    layout_state: ResMut<'w, HudLayoutState>,
    task_store: ResMut<'w, AgentTaskStore>,
    conversations: ResMut<'w, ConversationStore>,
    conversation_persistence: ResMut<'w, ConversationPersistenceState>,
    transport: Res<'w, MessageTransportAdapter>,
    app_state_persistence: ResMut<'w, AppStatePersistenceState>,
    visibility_state: ResMut<'w, TerminalVisibilityState>,
    view_state: ResMut<'w, TerminalViewState>,
    startup_loading: Option<ResMut<'w, StartupLoadingState>>,
    redraws: MessageWriter<'w, RequestRedraw>,
}

/// Applies queued app-level commands through the explicit use-case layer.
///
/// This is the sole UI/input mutation entrypoint after command translation. Each command is decoded
/// into one narrow use-case call so product policy lives in named handlers instead of in HUD fanout
/// tables or widget code.
pub(super) fn apply_app_commands(
    mut app_commands: MessageReader<AppCommand>,
    mut ctx: AppCommandContext,
) {
    // Keep the control flow staged so each branch owns one behavior path and later branches only run when earlier capture rules do not apply.
    for command in app_commands.read() {
        match command {
            AppCommand::Agent(command) => match command {
                AgentCommand::Create {
                    label,
                    kind,
                    working_directory,
                } => {
                    if let Err(error) = use_cases::spawn_agent_terminal(
                        &mut ctx.agent_catalog,
                        &mut ctx.runtime_index,
                        &mut ctx.app_session,
                        &mut ctx.terminal_manager,
                        &mut ctx.focus_state,
                        &ctx.runtime_spawner,
                        &mut ctx.input_capture,
                        &mut ctx.app_state_persistence,
                        &mut ctx.visibility_state,
                        &mut ctx.view_state,
                        ctx.startup_loading.as_deref_mut(),
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
                AgentCommand::Focus(agent_id) => {
                    ctx.app_session.visibility_mode = VisibilityMode::ShowAll;
                    use_cases::focus_agent(
                        *agent_id,
                        &mut ctx.app_session,
                        &ctx.runtime_index,
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
                    ctx.app_session.visibility_mode = VisibilityMode::FocusedOnly;
                    use_cases::focus_agent(
                        *agent_id,
                        &mut ctx.app_session,
                        &ctx.runtime_index,
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
                    ctx.app_session.active_agent = None;
                    let _ = ctx.focus_state.clear_active_terminal();
                    #[cfg(test)]
                    ctx.terminal_manager
                        .replace_test_focus_state(&ctx.focus_state);
                    ctx.visibility_state.policy = crate::hud::TerminalVisibilityPolicy::ShowAll;
                    ctx.view_state.focus_terminal(None);
                    ctx.input_capture
                        .reconcile_direct_terminal_input(ctx.focus_state.active_id());
                    mark_app_state_dirty(&mut ctx.app_state_persistence, Some(&ctx.time));
                    ctx.redraws.write(RequestRedraw);
                }
                AgentCommand::KillActive => {
                    if let Err(error) = use_cases::kill_active_agent(
                        &ctx.time,
                        &mut ctx.agent_catalog,
                        &mut ctx.runtime_index,
                        &mut ctx.app_session,
                        &mut ctx.task_store,
                        &mut ctx.terminal_manager,
                        &mut ctx.focus_state,
                        &ctx.runtime_spawner,
                        &mut ctx.input_capture,
                        &mut ctx.app_state_persistence,
                        &mut ctx.visibility_state,
                        &mut ctx.view_state,
                        &mut ctx.redraws,
                    ) {
                        append_debug_log(format!("kill active agent failed: {error}"));
                    }
                }
            },
            AppCommand::Task(command) => match command {
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
            },
            AppCommand::Composer(command) => match command {
                ComposerCommand::Open(request) => {
                    use_cases::open_composer(
                        request,
                        &mut ctx.app_session,
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
                        &ctx.terminal_manager,
                        &ctx.transport,
                        &ctx.time,
                        &mut ctx.redraws,
                    );
                }
                ComposerCommand::Cancel => {
                    use_cases::cancel_composer(&mut ctx.app_session, &mut ctx.redraws);
                }
            },
            AppCommand::Widget(command) => match command {
                WidgetCommand::Toggle(widget_id) => {
                    use_cases::toggle_widget(*widget_id, &mut ctx.layout_state);
                    ctx.redraws.write(RequestRedraw);
                }
                WidgetCommand::Reset(widget_id) => {
                    use_cases::reset_widget(*widget_id, &mut ctx.layout_state);
                    ctx.redraws.write(RequestRedraw);
                }
            },
        }
    }
}
