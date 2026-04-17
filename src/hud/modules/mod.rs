mod agent_list;
mod conversation_list;
mod dispatch;
mod info_bar;
mod thread_pane;

pub(in crate::hud) use agent_list::agent_rows;
#[cfg(test)]
pub(crate) use agent_list::{
    agent_list_bloom_specs, agent_row_rect, AgentListBloomAuthoringSpec, AgentListBloomSourceKind,
    AgentListBloomSourceSegment, AgentListRowSection,
};
pub(crate) use agent_list::{
    render_hover_overlay, reorder_target_index, row_at_point, selected_text_for_rows,
    text_row_at_point, AGENT_LIST_BLOOM_RED_B, AGENT_LIST_BLOOM_RED_G, AGENT_LIST_BLOOM_RED_R,
    AGENT_LIST_BORDER_ORANGE_B, AGENT_LIST_BORDER_ORANGE_G, AGENT_LIST_BORDER_ORANGE_R,
};
pub(crate) use dispatch::{
    clear_hover, handle_hover, handle_pointer_click, handle_scroll, render_module_content,
};
pub(in crate::hud) use info_bar::{INFO_BAR_BACKGROUND, INFO_BAR_BORDER};
