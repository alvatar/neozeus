mod interaction;
mod render;
mod rows;

pub(crate) use interaction::{
    clear_hover, handle_hover, handle_pointer_click, handle_scroll, reorder_target_index,
    row_at_point, selected_text_for_rows, text_row_at_point,
};
pub(crate) use render::{
    agent_list_bloom_specs, render_content, render_hover_overlay, AgentListBloomAuthoringSpec,
    AgentListBloomSourceKind, AgentListBloomSourceSegment,
};
pub(crate) use rows::{
    agent_list_content_height, AGENT_LIST_BORDER_ORANGE_B, AGENT_LIST_BORDER_ORANGE_G,
    AGENT_LIST_BORDER_ORANGE_R, AGENT_LIST_HEADER_HEIGHT, AGENT_LIST_LEFT_RAIL_WIDTH,
    AGENT_LIST_PAUSED_GRAY_B, AGENT_LIST_PAUSED_GRAY_G, AGENT_LIST_PAUSED_GRAY_R,
    AGENT_LIST_WORKING_GREEN_B, AGENT_LIST_WORKING_GREEN_G, AGENT_LIST_WORKING_GREEN_R,
};
pub(crate) use rows::{
    agent_row_label_position, agent_row_label_text, agent_row_rect, agent_row_text_hit_rect,
    row_main_rect, AgentListRowSection, AGENT_LIST_BLOOM_RED_B, AGENT_LIST_BLOOM_RED_G,
    AGENT_LIST_BLOOM_RED_R, AGENT_ROW_LABEL_SCALE_X, AGENT_ROW_LABEL_SCALE_Y,
    AGENT_ROW_LABEL_TEXT_SIZE, TMUX_ROW_LABEL_SCALE_X, TMUX_ROW_LABEL_SCALE_Y,
    TMUX_ROW_LABEL_TEXT_SIZE,
};
pub(in crate::hud) use rows::{agent_rows, projected_agent_rows, AgentListDragPreview};

#[cfg(test)]
mod tests;
