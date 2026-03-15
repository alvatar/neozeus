use crate::{
    hud::{
        modules, AgentDirectory, HudCommand, HudEnvelope, HudEvent, HudModuleId, HudRecipients,
        HudState, TerminalVisibilityPolicy, TerminalVisibilityState,
    },
    terminals::{
        append_debug_log, spawn_terminal_presentation, TerminalManager, TerminalPresentationStore,
        TerminalRuntimeSpawner, TerminalViewState,
    },
};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct HudDispatcher {
    pub(crate) commands: Vec<HudCommand>,
    pub(crate) events: Vec<HudEnvelope<HudEvent>>,
}

fn recipients_match(module_id: HudModuleId, recipients: &HudRecipients) -> bool {
    match recipients {
        HudRecipients::One(id) => *id == module_id,
        HudRecipients::Some(ids) => ids.contains(&module_id),
        HudRecipients::All => true,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD command application touches app/domain resources, terminal runtime, and HUD state together"
)]
pub(crate) fn apply_hud_commands(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    mut hud_state: ResMut<HudState>,
    mut dispatcher: ResMut<HudDispatcher>,
    mut agent_directory: ResMut<AgentDirectory>,
    mut visibility_state: ResMut<TerminalVisibilityState>,
    mut view_state: ResMut<TerminalViewState>,
) {
    let queued = std::mem::take(&mut dispatcher.commands);
    for command in queued {
        match command {
            HudCommand::SpawnTerminal => {
                let bridge = runtime_spawner.spawn();
                let (terminal_id, slot) = terminal_manager.create_terminal_with_slot(bridge);
                spawn_terminal_presentation(
                    &mut commands,
                    &mut images,
                    &mut presentation_store,
                    terminal_id,
                    slot,
                );
                append_debug_log(format!("spawned terminal {}", terminal_id.0));
                dispatcher.events.push(HudEnvelope {
                    recipients: HudRecipients::Some(vec![
                        HudModuleId::DebugToolbar,
                        HudModuleId::AgentList,
                    ]),
                    payload: HudEvent::TerminalSpawned(terminal_id),
                });
            }
            HudCommand::FocusTerminal(id) => {
                terminal_manager.focus_terminal(id);
                dispatcher.events.push(HudEnvelope {
                    recipients: HudRecipients::Some(vec![
                        HudModuleId::DebugToolbar,
                        HudModuleId::AgentList,
                    ]),
                    payload: HudEvent::TerminalFocused(id),
                });
            }
            HudCommand::HideAllButTerminal(id) => {
                visibility_state.policy = TerminalVisibilityPolicy::Isolate(id);
                dispatcher.events.push(HudEnvelope {
                    recipients: HudRecipients::All,
                    payload: HudEvent::TerminalPresentationPolicyChanged(visibility_state.policy),
                });
            }
            HudCommand::ShowAllTerminals => {
                visibility_state.policy = TerminalVisibilityPolicy::ShowAll;
                dispatcher.events.push(HudEnvelope {
                    recipients: HudRecipients::All,
                    payload: HudEvent::TerminalPresentationPolicyChanged(visibility_state.policy),
                });
            }
            HudCommand::RenameAgent { terminal_id, label } => {
                agent_directory.labels.insert(terminal_id, label.clone());
                dispatcher.events.push(HudEnvelope {
                    recipients: HudRecipients::Some(vec![
                        HudModuleId::DebugToolbar,
                        HudModuleId::AgentList,
                    ]),
                    payload: HudEvent::AgentRenamed { terminal_id, label },
                });
            }
            HudCommand::ToggleModule(id) => {
                let enabled = hud_state
                    .get(id)
                    .is_some_and(|module| !module.shell.enabled);
                hud_state.toggle_module(id);
                dispatcher.events.push(HudEnvelope {
                    recipients: HudRecipients::One(id),
                    payload: HudEvent::ModuleEnabledChanged { id, enabled },
                });
            }
            HudCommand::ToggleActiveTerminalDisplayMode => {
                let active_id = terminal_manager.active_id();
                presentation_store.toggle_active_display_mode(active_id);
                if let Some(id) = active_id {
                    dispatcher.events.push(HudEnvelope {
                        recipients: HudRecipients::Some(vec![
                            HudModuleId::DebugToolbar,
                            HudModuleId::AgentList,
                        ]),
                        payload: HudEvent::ActiveTerminalDisplayModeToggled(id),
                    });
                }
            }
            HudCommand::ResetTerminalView => {
                view_state.distance = 10.0;
                view_state.offset = Vec2::ZERO;
            }
            HudCommand::SendActiveTerminalCommand(command) => {
                if let Some(bridge) = terminal_manager.active_bridge() {
                    bridge.send(crate::terminals::TerminalCommand::SendCommand(command));
                }
            }
        }
    }
}

pub(crate) fn dispatch_hud_events(
    mut hud_state: ResMut<HudState>,
    mut dispatcher: ResMut<HudDispatcher>,
) {
    let queued = std::mem::take(&mut dispatcher.events);
    for envelope in queued {
        for module_id in hud_state.iter_z_order().collect::<Vec<_>>() {
            if !recipients_match(module_id, &envelope.recipients) {
                continue;
            }
            let Some(module) = hud_state.get_mut(module_id) else {
                continue;
            };
            modules::handle_event(&mut module.model, &envelope.payload);
        }
    }
}
