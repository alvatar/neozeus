use crate::{
    agents::{AgentId, AgentRuntimeIndex},
    conversations::{
        AgentTaskStore, ConversationStore, MessageAuthor, MessageDeliveryState,
        MessageTransportAdapter,
    },
    hud::HudLayoutState,
    terminals::{
        mark_terminal_notes_dirty, TerminalFocusState, TerminalManager, TerminalNotesState,
        TerminalPresentationStore, TerminalViewState,
    },
};
use bevy::{prelude::*, window::RequestRedraw};

pub(crate) fn send_terminal_command(
    terminal_id: crate::terminals::TerminalId,
    command: &str,
    terminal_manager: &TerminalManager,
) {
    if let Some(terminal) = terminal_manager.get(terminal_id) {
        terminal
            .bridge
            .send(crate::terminals::TerminalCommand::SendCommand(
                command.to_owned(),
            ));
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "message send spans domain store, transport adapter, and runtime mapping"
)]
pub(crate) fn send_message(
    conversation_id: crate::conversations::ConversationId,
    sender: AgentId,
    body: String,
    conversations: &mut ConversationStore,
    _transport: &MessageTransportAdapter,
    runtime_index: &AgentRuntimeIndex,
    terminal_manager: &TerminalManager,
) {
    let message_id = conversations.push_message(
        conversation_id,
        MessageAuthor::User,
        body.clone(),
        MessageDeliveryState::Pending,
    );
    let Some(terminal_id) = runtime_index.primary_terminal(sender) else {
        conversations.set_delivery(
            message_id,
            MessageDeliveryState::Failed("no terminal linked".into()),
        );
        return;
    };
    send_terminal_command(terminal_id, &body, terminal_manager);
    conversations.set_delivery(message_id, MessageDeliveryState::Delivered);
}

pub(crate) fn set_task_text(
    agent_id: AgentId,
    text: &str,
    tasks: &mut AgentTaskStore,
    notes_state: &mut TerminalNotesState,
    runtime_index: &AgentRuntimeIndex,
    time: &Time,
) -> bool {
    let task_changed = tasks.set_text(agent_id, text);
    let notes_changed = runtime_index
        .session_name(agent_id)
        .is_some_and(|session_name| notes_state.set_note_text(session_name, text));
    if notes_changed {
        mark_terminal_notes_dirty(notes_state, Some(time));
    }
    task_changed || notes_changed
}

pub(crate) fn append_task(
    agent_id: AgentId,
    text: &str,
    tasks: &mut AgentTaskStore,
    notes_state: &mut TerminalNotesState,
    runtime_index: &AgentRuntimeIndex,
    time: &Time,
) -> bool {
    if !tasks.append_task(agent_id, text) {
        return false;
    }
    if let Some(updated) = tasks.text(agent_id).map(str::to_owned) {
        let _ = set_task_text(agent_id, &updated, tasks, notes_state, runtime_index, time);
    }
    true
}

pub(crate) fn prepend_task(
    agent_id: AgentId,
    text: &str,
    tasks: &mut AgentTaskStore,
    notes_state: &mut TerminalNotesState,
    runtime_index: &AgentRuntimeIndex,
    time: &Time,
) -> bool {
    if !tasks.prepend_task(agent_id, text) {
        return false;
    }
    if let Some(updated) = tasks.text(agent_id).map(str::to_owned) {
        let _ = set_task_text(agent_id, &updated, tasks, notes_state, runtime_index, time);
    }
    true
}

pub(crate) fn clear_done_tasks(
    agent_id: AgentId,
    tasks: &mut AgentTaskStore,
    notes_state: &mut TerminalNotesState,
    runtime_index: &AgentRuntimeIndex,
    time: &Time,
) -> bool {
    if !tasks.clear_done(agent_id) {
        return false;
    }
    if let Some(text) = tasks.text(agent_id).map(str::to_owned) {
        let _ = set_task_text(agent_id, &text, tasks, notes_state, runtime_index, time);
    } else if let Some(session_name) = runtime_index.session_name(agent_id) {
        let _ = notes_state.set_note_text(session_name, "");
        mark_terminal_notes_dirty(notes_state, Some(time));
    }
    true
}

pub(crate) fn consume_next_task(
    agent_id: AgentId,
    tasks: &mut AgentTaskStore,
    notes_state: &mut TerminalNotesState,
    runtime_index: &AgentRuntimeIndex,
    terminal_manager: &TerminalManager,
    time: &Time,
) -> bool {
    let Some(message) = tasks.consume_next(agent_id) else {
        return false;
    };
    if let Some(terminal_id) = runtime_index.primary_terminal(agent_id) {
        send_terminal_command(terminal_id, &message, terminal_manager);
    }
    if let Some(updated) = tasks.text(agent_id).map(str::to_owned) {
        let _ = set_task_text(agent_id, &updated, tasks, notes_state, runtime_index, time);
    } else if let Some(session_name) = runtime_index.session_name(agent_id) {
        let _ = notes_state.set_note_text(session_name, "");
        mark_terminal_notes_dirty(notes_state, Some(time));
    }
    true
}

pub(crate) fn toggle_active_display_mode(
    focus_state: &TerminalFocusState,
    presentation_store: &mut TerminalPresentationStore,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    presentation_store.toggle_active_display_mode(focus_state.active_id());
    redraws.write(RequestRedraw);
}

pub(crate) fn reset_active_view(
    focus_state: &TerminalFocusState,
    view_state: &mut TerminalViewState,
    redraws: &mut MessageWriter<RequestRedraw>,
) {
    view_state.distance = 10.0;
    view_state.reset_active_offset(focus_state.active_id());
    redraws.write(RequestRedraw);
}

pub(crate) fn toggle_widget(
    widget_id: crate::hud::HudWidgetKey,
    layout_state: &mut HudLayoutState,
) {
    let enabled = layout_state
        .get(widget_id)
        .is_some_and(|module| !module.shell.enabled);
    layout_state.set_module_enabled(widget_id, enabled);
}
