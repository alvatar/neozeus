use crate::hud::{
    HudIntent, HudModuleRequest, TerminalFocusRequest, TerminalLifecycleRequest,
    TerminalSendRequest, TerminalTaskRequest, TerminalViewRequest, TerminalVisibilityRequest,
};
use bevy::prelude::*;

#[allow(
    clippy::too_many_arguments,
    reason = "intent fanout is intentionally explicit across narrow request channels"
)]
pub(crate) fn dispatch_hud_intents(
    mut intents: MessageReader<HudIntent>,
    mut focus_requests: MessageWriter<TerminalFocusRequest>,
    mut visibility_requests: MessageWriter<TerminalVisibilityRequest>,
    mut module_requests: MessageWriter<HudModuleRequest>,
    mut view_requests: MessageWriter<TerminalViewRequest>,
    mut send_requests: MessageWriter<TerminalSendRequest>,
    mut lifecycle_requests: MessageWriter<TerminalLifecycleRequest>,
    mut task_requests: MessageWriter<TerminalTaskRequest>,
) {
    for intent in intents.read() {
        match intent {
            HudIntent::SpawnTerminal => {
                lifecycle_requests.write(TerminalLifecycleRequest::Spawn);
            }
            HudIntent::FocusTerminal(terminal_id) => {
                focus_requests.write(TerminalFocusRequest {
                    terminal_id: *terminal_id,
                });
            }
            HudIntent::HideAllButTerminal(terminal_id) => {
                visibility_requests.write(TerminalVisibilityRequest::Isolate(*terminal_id));
            }
            HudIntent::ShowAllTerminals => {
                visibility_requests.write(TerminalVisibilityRequest::ShowAll);
            }
            HudIntent::ToggleModule(id) => {
                module_requests.write(HudModuleRequest::Toggle(*id));
            }
            HudIntent::ResetModule(id) => {
                module_requests.write(HudModuleRequest::Reset(*id));
            }
            HudIntent::ToggleActiveTerminalDisplayMode => {
                view_requests.write(TerminalViewRequest::ToggleActiveDisplayMode);
            }
            HudIntent::ResetTerminalView => {
                view_requests.write(TerminalViewRequest::ResetActiveView);
            }
            HudIntent::SendActiveTerminalCommand(command) => {
                send_requests.write(TerminalSendRequest::Active(command.clone()));
            }
            HudIntent::SendTerminalCommand(terminal_id, command) => {
                send_requests.write(TerminalSendRequest::Target {
                    terminal_id: *terminal_id,
                    command: command.clone(),
                });
            }
            HudIntent::SetTerminalTaskText(terminal_id, text) => {
                task_requests.write(TerminalTaskRequest::SetText {
                    terminal_id: *terminal_id,
                    text: text.clone(),
                });
            }
            HudIntent::ClearDoneTerminalTasks(terminal_id) => {
                task_requests.write(TerminalTaskRequest::ClearDone {
                    terminal_id: *terminal_id,
                });
            }
            HudIntent::AppendTerminalTask(terminal_id, text) => {
                task_requests.write(TerminalTaskRequest::Append {
                    terminal_id: *terminal_id,
                    text: text.clone(),
                });
            }
            HudIntent::PrependTerminalTask(terminal_id, text) => {
                task_requests.write(TerminalTaskRequest::Prepend {
                    terminal_id: *terminal_id,
                    text: text.clone(),
                });
            }
            HudIntent::ConsumeNextTerminalTask(terminal_id) => {
                task_requests.write(TerminalTaskRequest::ConsumeNext {
                    terminal_id: *terminal_id,
                });
            }
            HudIntent::KillActiveTerminal => {
                lifecycle_requests.write(TerminalLifecycleRequest::KillActive);
            }
        }
    }
}
