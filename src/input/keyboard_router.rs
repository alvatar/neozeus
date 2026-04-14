use super::{
    direct_terminal_input::handle_direct_input_route,
    keybindings::{binding_action_for_event, KeybindingAction, PRIMARY_KEYBINDINGS},
    keyboard_route::{resolve_keyboard_route, KeyboardRoute},
};
use crate::{
    aegis::{AegisPolicyStore, DEFAULT_AEGIS_PROMPT},
    agents::{AgentCatalog, AgentRuntimeIndex},
    app::{
        AegisCommand, AgentCommand as AppAgentCommand, AppCommand, AppSessionState,
        ComposerCommand, ComposerRequest, CreateAgentKind, OwnedTmuxCommand,
        TaskCommand as AppTaskCommand, WidgetCommand,
    },
    hud::{
        adjacent_agent_list_target, AgentListNavigationTarget, AgentListSelection,
        AgentListUiState, AgentListView, HudInputCaptureState,
    },
    terminals::{TerminalCommand, TerminalFocusState, TerminalId, TerminalManager},
};
use bevy::{
    app::AppExit,
    ecs::system::SystemParam,
    input::keyboard::KeyboardInput,
    prelude::*,
    window::{PrimaryWindow, RequestRedraw},
};
use bevy_egui::EguiClipboard;

#[derive(SystemParam)]
struct KeyboardInputContext<'w, 's> {
    messages: MessageReader<'w, 's, KeyboardInput>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    primary_window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    terminal_manager: Res<'w, TerminalManager>,
    focus_state: Res<'w, TerminalFocusState>,
    runtime_index: Res<'w, AgentRuntimeIndex>,
    agent_catalog: Res<'w, AgentCatalog>,
    aegis_policy: Res<'w, AegisPolicyStore>,
    app_session: ResMut<'w, AppSessionState>,
    input_capture: ResMut<'w, HudInputCaptureState>,
    agent_list_state: ResMut<'w, AgentListUiState>,
    agent_list_view: Res<'w, AgentListView>,
    selection: Option<Res<'w, AgentListSelection>>,
    clipboard: Option<ResMut<'w, EguiClipboard>>,
    clipboard_ingress: Local<'s, super::modal_dialogs::MessageDialogClipboardIngressState>,
    app_commands: MessageWriter<'w, AppCommand>,
    app_exits: MessageWriter<'w, AppExit>,
    redraws: MessageWriter<'w, RequestRedraw>,
}

#[derive(Clone, Copy)]
struct ActiveTerminalTarget {
    terminal_id: TerminalId,
    agent_id: Option<crate::agents::AgentId>,
}

fn shift_pressed(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
}

fn matched_primary_action(
    event: &KeyboardInput,
    keys: &ButtonInput<KeyCode>,
) -> Option<KeybindingAction> {
    let (ctrl, alt, super_key) = super::has_plain_modifiers(keys);
    binding_action_for_event(
        PRIMARY_KEYBINDINGS,
        event,
        ctrl,
        alt,
        shift_pressed(keys),
        super_key,
    )
}

fn resolve_active_interactive_terminal(
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    runtime_index: &AgentRuntimeIndex,
) -> Option<ActiveTerminalTarget> {
    let terminal_id = focus_state.active_id()?;
    let terminal = terminal_manager.get(terminal_id)?;
    terminal
        .snapshot
        .runtime
        .is_interactive()
        .then(|| ActiveTerminalTarget {
            terminal_id,
            agent_id: runtime_index.agent_for_terminal(terminal_id),
        })
}

fn selected_list_target(selection: Option<&AgentListSelection>) -> AgentListSelection {
    selection.cloned().unwrap_or_default()
}

fn resolve_pause_target(
    app_session: &AppSessionState,
    active_terminal: Option<ActiveTerminalTarget>,
) -> Option<crate::agents::AgentId> {
    app_session
        .focus_intent
        .selected_agent()
        .or(active_terminal.and_then(|target| target.agent_id))
}

fn supports_clone(agent_catalog: &AgentCatalog, agent_id: crate::agents::AgentId) -> bool {
    match agent_catalog.kind(agent_id) {
        Some(crate::agents::AgentKind::Pi) => {
            agent_catalog.clone_source_session_path(agent_id).is_some()
        }
        Some(crate::agents::AgentKind::Claude | crate::agents::AgentKind::Codex) => {
            agent_catalog.recovery_spec(agent_id).is_some()
        }
        Some(crate::agents::AgentKind::Terminal | crate::agents::AgentKind::Verifier) | None => {
            false
        }
    }
}

fn emit_agent_list_navigation(
    selection: &AgentListSelection,
    agent_list_view: &AgentListView,
    step: isize,
    app_commands: &mut MessageWriter<AppCommand>,
) {
    let Some(target) = adjacent_agent_list_target(selection, agent_list_view, step) else {
        return;
    };
    match target {
        AgentListNavigationTarget::Agent(agent_id) => {
            app_commands.write(AppCommand::Agent(AppAgentCommand::Focus(agent_id)));
            app_commands.write(AppCommand::Agent(AppAgentCommand::Inspect(agent_id)));
        }
        AgentListNavigationTarget::OwnedTmux(session_uid) => {
            app_commands.write(AppCommand::OwnedTmux(OwnedTmuxCommand::Select {
                session_uid,
            }));
        }
    }
}

fn handle_primary_route(ctx: &mut KeyboardInputContext<'_, '_>) {
    let selection = selected_list_target(ctx.selection.as_deref());
    let active_terminal = resolve_active_interactive_terminal(
        &ctx.terminal_manager,
        &ctx.focus_state,
        &ctx.runtime_index,
    );
    let pause_target = resolve_pause_target(&ctx.app_session, active_terminal);

    for event in ctx.messages.read() {
        let Some(action) = matched_primary_action(event, &ctx.keys) else {
            continue;
        };
        match action {
            KeybindingAction::SpawnTerminal => {
                ctx.app_session
                    .create_agent_dialog
                    .open(CreateAgentKind::Pi);
                ctx.redraws.write(RequestRedraw);
            }
            KeybindingAction::OpenCloneDialog => {
                let AgentListSelection::Agent(agent_id) = selection else {
                    continue;
                };
                if !supports_clone(&ctx.agent_catalog, agent_id) {
                    continue;
                }
                let Some(kind) = ctx.agent_catalog.kind(agent_id) else {
                    continue;
                };
                let current_label = ctx.agent_catalog.label(agent_id).unwrap_or("AGENT");
                ctx.app_session
                    .clone_agent_dialog
                    .open(agent_id, kind, current_label);
                ctx.redraws.write(RequestRedraw);
            }
            KeybindingAction::KillSelected => match selection {
                AgentListSelection::OwnedTmux(_) => {
                    ctx.app_commands
                        .write(AppCommand::OwnedTmux(OwnedTmuxCommand::KillSelected));
                }
                AgentListSelection::Agent(_) => {
                    ctx.app_commands
                        .write(AppCommand::Agent(AppAgentCommand::KillSelected));
                }
                AgentListSelection::None => {}
            },
            KeybindingAction::OpenResetDialog => {
                ctx.app_session.reset_dialog.open();
                ctx.app_session.recovery_status.show_reset_requested();
                ctx.redraws.write(RequestRedraw);
            }
            KeybindingAction::ExitApplication => {
                ctx.app_exits.write(AppExit::Success);
            }
            KeybindingAction::OpenMessageEditor => {
                if let Some(agent_id) = active_terminal.and_then(|target| target.agent_id) {
                    ctx.app_commands
                        .write(AppCommand::Composer(ComposerCommand::Open(
                            ComposerRequest {
                                mode: crate::composer::ComposerMode::Message { agent_id },
                            },
                        )));
                }
            }
            KeybindingAction::OpenTaskEditor => {
                if let Some(agent_id) = active_terminal.and_then(|target| target.agent_id) {
                    ctx.app_commands
                        .write(AppCommand::Composer(ComposerCommand::Open(
                            ComposerRequest {
                                mode: crate::composer::ComposerMode::TaskEdit { agent_id },
                            },
                        )));
                }
            }
            KeybindingAction::OpenRenameDialog => {
                if let Some(agent_id) = active_terminal.and_then(|target| target.agent_id) {
                    let current_label = ctx.agent_catalog.label(agent_id).unwrap_or_default();
                    ctx.app_session
                        .rename_agent_dialog
                        .open(agent_id, current_label);
                    ctx.redraws.write(RequestRedraw);
                }
            }
            KeybindingAction::ToggleAegis => {
                if let Some(agent_id) = active_terminal.and_then(|target| target.agent_id) {
                    if let Some(agent_uid) = ctx.agent_catalog.uid(agent_id) {
                        if ctx.aegis_policy.is_enabled(agent_uid) {
                            ctx.app_commands
                                .write(AppCommand::Aegis(AegisCommand::Disable { agent_id }));
                        } else {
                            let prompt_text = ctx
                                .aegis_policy
                                .prompt_text(agent_uid)
                                .unwrap_or(DEFAULT_AEGIS_PROMPT);
                            ctx.app_session.aegis_dialog.open(agent_id, prompt_text);
                            ctx.redraws.write(RequestRedraw);
                        }
                    }
                }
            }
            KeybindingAction::ConsumeNextTask => {
                if let Some(agent_id) = active_terminal.and_then(|target| target.agent_id) {
                    ctx.app_commands
                        .write(AppCommand::Task(AppTaskCommand::ConsumeNext { agent_id }));
                }
            }
            KeybindingAction::TogglePaused => {
                if let Some(agent_id) = pause_target {
                    ctx.app_commands
                        .write(AppCommand::Agent(AppAgentCommand::TogglePaused(agent_id)));
                }
            }
            KeybindingAction::ToggleAgentContext => {
                ctx.agent_list_state.show_selected_context =
                    !ctx.agent_list_state.show_selected_context;
                ctx.redraws.write(RequestRedraw);
            }
            KeybindingAction::ClearDoneTasks => {
                if let Some(agent_id) = active_terminal.and_then(|target| target.agent_id) {
                    ctx.app_commands
                        .write(AppCommand::Task(AppTaskCommand::ClearDone { agent_id }));
                }
            }
            KeybindingAction::ScrollPageUp => {
                if let Some(target) = active_terminal {
                    if let Some(terminal) = ctx.terminal_manager.get(target.terminal_id) {
                        terminal.bridge.send(TerminalCommand::ScrollDisplay(
                            crate::input::terminal_page_scroll_rows(
                                &ctx.terminal_manager,
                                target.terminal_id,
                            ),
                        ));
                    }
                }
            }
            KeybindingAction::ScrollPageDown => {
                if let Some(target) = active_terminal {
                    if let Some(terminal) = ctx.terminal_manager.get(target.terminal_id) {
                        terminal.bridge.send(TerminalCommand::ScrollDisplay(
                            -crate::input::terminal_page_scroll_rows(
                                &ctx.terminal_manager,
                                target.terminal_id,
                            ),
                        ));
                    }
                }
            }
            KeybindingAction::HudNextRow => emit_agent_list_navigation(
                &selection,
                &ctx.agent_list_view,
                1,
                &mut ctx.app_commands,
            ),
            KeybindingAction::HudPrevRow => emit_agent_list_navigation(
                &selection,
                &ctx.agent_list_view,
                -1,
                &mut ctx.app_commands,
            ),
            KeybindingAction::ToggleWidget(widget_id) => {
                ctx.app_commands
                    .write(AppCommand::Widget(WidgetCommand::Toggle(widget_id)));
            }
            KeybindingAction::ResetWidget(widget_id) => {
                ctx.app_commands
                    .write(AppCommand::Widget(WidgetCommand::Reset(widget_id)));
            }
            KeybindingAction::ToggleDirectInput
            | KeybindingAction::DirectInputScrollToBottom
            | KeybindingAction::DialogEscape
            | KeybindingAction::DialogTabForward
            | KeybindingAction::DialogTabBackward
            | KeybindingAction::MessageDialogSubmit
            | KeybindingAction::TaskDialogClearDone => {}
        }
    }
}

fn handle_keyboard_input_with_context(ctx: &mut KeyboardInputContext<'_, '_>) {
    let route = resolve_keyboard_route(&ctx.app_session, &ctx.input_capture, &ctx.primary_window);
    let modifiers = super::message_box_key_modifiers(&ctx.keys);

    match route {
        KeyboardRoute::Ignore => {}
        KeyboardRoute::DirectInput => handle_direct_input_route(
            &mut ctx.messages,
            &ctx.keys,
            &ctx.primary_window,
            &ctx.terminal_manager,
            &ctx.focus_state,
            &mut ctx.app_session,
            &mut ctx.input_capture,
            &mut ctx.redraws,
        ),
        KeyboardRoute::ResetDialog => {
            let _ = super::handle_reset_dialog_events(
                &mut ctx.app_session,
                &mut ctx.messages,
                modifiers,
                &mut ctx.app_commands,
                &mut ctx.redraws,
            );
        }
        KeyboardRoute::AegisDialog => {
            let _ = super::handle_aegis_dialog_events(
                &mut ctx.app_session,
                &ctx.primary_window,
                &mut ctx.messages,
                modifiers,
                &mut ctx.app_commands,
                &mut ctx.redraws,
            );
        }
        KeyboardRoute::RenameAgentDialog => {
            let _ = super::handle_rename_dialog_events(
                &mut ctx.app_session,
                &mut ctx.messages,
                modifiers,
                &mut ctx.app_commands,
                &mut ctx.redraws,
            );
        }
        KeyboardRoute::CloneAgentDialog => {
            let _ = super::handle_clone_dialog_events(
                &mut ctx.app_session,
                &mut ctx.messages,
                modifiers,
                &mut ctx.app_commands,
                &mut ctx.redraws,
            );
        }
        KeyboardRoute::CreateAgentDialog => {
            let _ = super::handle_create_dialog_events(
                &mut ctx.app_session,
                &mut ctx.messages,
                modifiers,
                &mut ctx.app_commands,
                &mut ctx.redraws,
            );
        }
        KeyboardRoute::MessageDialog => {
            super::sync_message_editor_clipboard_ingress(
                &ctx.app_session,
                ctx.clipboard.as_deref_mut(),
                &mut ctx.clipboard_ingress,
            );
            let _ = super::handle_message_editor_events(
                &mut ctx.app_session,
                &ctx.primary_window,
                &mut ctx.messages,
                modifiers,
                ctx.clipboard.as_deref_mut(),
                &mut ctx.clipboard_ingress,
                &mut ctx.app_commands,
                &mut ctx.redraws,
            );
        }
        KeyboardRoute::TaskDialog => {
            let _ = super::handle_task_editor_events(
                &mut ctx.app_session,
                &ctx.primary_window,
                &mut ctx.messages,
                modifiers,
                &mut ctx.app_commands,
                &mut ctx.redraws,
            );
        }
        KeyboardRoute::Primary => handle_primary_route(ctx),
    }
}

/// Routes all keyboard input through one explicit precedence resolver.
///
/// This exclusive wrapper keeps `crate::input` as the stable entrypoint while letting the router use
/// a focused internal `SystemState` context instead of another giant scheduled function signature.
pub(crate) fn handle_keyboard_input(world: &mut World) {
    let mut state: bevy::ecs::system::SystemState<KeyboardInputContext> =
        bevy::ecs::system::SystemState::new(world);
    {
        let mut ctx = state.get_mut(world);
        handle_keyboard_input_with_context(&mut ctx);
    }
    state.apply(world);
}
