mod animation;
mod bloom;
mod capture;
mod commands;
mod compositor;
mod dispatcher;
mod input;
mod message_box;
mod messages;
mod modules;
mod persistence;
mod render;
mod setup;
mod state;

pub(crate) use animation::animate_hud_modules;
pub(crate) use bloom::{
    setup_hud_widget_bloom, sync_hud_widget_bloom, AgentListBloomAdditiveCameraMarker,
    AgentListBloomBlurMaterial, HudBloomSettings, HudWidgetBloom,
};
pub(crate) use capture::{
    finalize_window_capture, request_hud_composite_capture, request_hud_texture_capture,
    request_window_capture, HudCompositeCaptureConfig, HudTextureCaptureConfig,
    WindowCaptureConfig,
};
pub(crate) use commands::{
    apply_hud_module_requests, apply_terminal_focus_requests, apply_terminal_lifecycle_requests,
    apply_terminal_send_requests, apply_terminal_task_requests, apply_terminal_view_requests,
    apply_visibility_requests, dispatch_hud_intents,
};
pub(crate) use compositor::{
    setup_hud_offscreen_compositor, sync_hud_offscreen_compositor, HudCompositeCameraMarker,
    HudOffscreenCompositor,
};
pub(crate) use input::{handle_hud_module_shortcuts, handle_hud_pointer_input};
pub(crate) use message_box::{
    message_box_action_at, message_box_action_buttons, message_box_rect, task_dialog_action_at,
    task_dialog_action_buttons, task_dialog_rect, HudMessageBoxAction, HudMessageBoxState,
    HudTaskDialogAction, HudTaskDialogState,
};
pub(crate) use messages::{
    HudIntent, HudModuleRequest, TerminalFocusRequest, TerminalLifecycleRequest,
    TerminalSendRequest, TerminalTaskRequest, TerminalViewRequest, TerminalVisibilityRequest,
};
pub(crate) use persistence::{save_hud_layout_if_dirty, HudPersistenceState};
pub(crate) use render::{
    render_hud_modal_scene, render_hud_scene, HudModalCameraMarker, HudModalVectorSceneMarker,
    HudVectorSceneMarker, HUD_MODAL_CAMERA_ORDER, HUD_MODAL_RENDER_LAYER,
};
pub(crate) use setup::{append_hud_log, hud_needs_redraw, setup_hud, sync_structural_hud_layout};
pub(crate) use state::{
    default_hud_module_instance, docked_agent_list_rect, AgentDirectory, HudDragState,
    HudInputCaptureState, HudLayoutState, HudModalState, HudModuleId, HudModuleModel, HudRect,
    TerminalVisibilityPolicy, TerminalVisibilityState, HUD_BUTTON_GAP, HUD_BUTTON_HEIGHT,
    HUD_BUTTON_MIN_WIDTH, HUD_MODULE_DEFINITIONS, HUD_MODULE_PADDING, HUD_ROW_HEIGHT,
    HUD_TITLEBAR_HEIGHT,
};

#[cfg(test)]
pub(crate) use bloom::{
    agent_list_bloom_layer, agent_list_bloom_z, resolve_agent_list_bloom_debug_previews,
    resolve_agent_list_bloom_intensity, AgentListBloomBlurUniform, AgentListBloomCameraMarker,
    AgentListBloomCompositeMarker, AgentListBloomSourceKind, AgentListBloomSourceSegment,
    AgentListBloomSourceSprite,
};
#[cfg(test)]
pub(crate) use compositor::{
    HudCompositeLayerId, HudCompositeLayerMarker, HUD_COMPOSITE_FOREGROUND_Z,
    HUD_COMPOSITE_RENDER_LAYER,
};
#[cfg(test)]
pub(crate) use dispatcher::kill_active_terminal;
#[cfg(test)]
pub(crate) use modules::{
    agent_row_rect, agent_rows, debug_toolbar_buttons,
    handle_pointer_click as dispatch_hud_pointer_click, handle_scroll as dispatch_hud_scroll,
    resolve_agent_label, AgentListRowSection, AGENT_LIST_BLOOM_RED_B, AGENT_LIST_BLOOM_RED_G,
    AGENT_LIST_BLOOM_RED_R, AGENT_LIST_BORDER_ORANGE_B, AGENT_LIST_BORDER_ORANGE_G,
    AGENT_LIST_BORDER_ORANGE_R,
};
#[cfg(test)]
pub(crate) use persistence::{
    apply_persisted_layout, parse_persisted_hud_state, resolve_hud_layout_path_with,
    serialize_persisted_hud_state, PersistedHudModuleState, PersistedHudState,
};
#[cfg(test)]
pub(crate) use state::HudState;
