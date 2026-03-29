use super::*;
use crate::{
    app::{AgentCommand as AppAgentCommand, AppCommand},
    hud::{
        AgentListUiState, AgentListView, ConversationListUiState, ConversationListView, HudRect,
        HudState, HudWidgetKey, InfoBarView,
    },
    terminals::TerminalManager,
    tests::test_bridge,
};
use bevy::prelude::*;

/// Verifies that clicking a conversation-list row selects the linked terminal through the standard
/// focus+isolate command pair.
#[test]
fn clicking_conversation_list_row_emits_focus_and_isolate_commands() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::ConversationList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[2]),
    );
    let conversation_list_view = ConversationListView {
        rows: vec![crate::hud::ConversationListRowView {
            agent_id: crate::agents::AgentId(1),
            terminal_id: Some(terminal_id),
            conversation_id: crate::conversations::ConversationId(1),
            label: "alpha".into(),
            message_count: 2,
            selected: true,
        }],
    };
    let mut emitted_commands = Vec::new();

    crate::hud::handle_pointer_click(
        HudWidgetKey::ConversationList,
        HudRect {
            x: 332.0,
            y: 140.0,
            w: 320.0,
            h: 280.0,
        },
        Vec2::new(360.0, 154.0),
        &AgentListUiState::default(),
        &ConversationListUiState::default(),
        &AgentListView::default(),
        &conversation_list_view,
        &InfoBarView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(
        emitted_commands,
        vec![
            AppCommand::Agent(AppAgentCommand::Focus(crate::agents::AgentId(1))),
            AppCommand::Agent(AppAgentCommand::Inspect(crate::agents::AgentId(1))),
        ]
    );
}
