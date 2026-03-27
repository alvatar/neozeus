mod animation;
mod bloom;
mod capture;
mod compositor;
mod input;
mod message_box;
mod messages;
mod modules;
mod persistence;
mod render;
mod setup;
mod state;
mod view_models;
mod widgets;

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
pub(crate) use messages::HudIntent;
pub(crate) use persistence::{save_hud_layout_if_dirty, HudPersistenceState};
pub(crate) use render::{
    render_hud_modal_scene, render_hud_scene, HudModalCameraMarker, HudModalVectorSceneMarker,
    HudVectorSceneMarker, HUD_MODAL_CAMERA_ORDER, HUD_MODAL_RENDER_LAYER,
};
pub(crate) use setup::{append_hud_log, hud_needs_redraw, setup_hud, sync_structural_hud_layout};
pub(crate) use state::{
    default_hud_module_instance, docked_agent_list_rect, HudDragState, HudInputCaptureState,
    HudLayoutState, HudModuleModel, HudRect, TerminalVisibilityPolicy, TerminalVisibilityState,
    HUD_AGENT_LIST_WIDTH, HUD_BUTTON_GAP, HUD_BUTTON_HEIGHT, HUD_BUTTON_MIN_WIDTH,
    HUD_MODULE_PADDING, HUD_ROW_HEIGHT, HUD_TITLEBAR_HEIGHT,
};
pub(crate) use view_models::{
    sync_hud_view_models, AgentListView, ComposerView, ConversationListView, ThreadView,
};
pub(crate) use widgets::{HudWidgetDefinition, HudWidgetKey, HUD_WIDGET_DEFINITIONS};

#[cfg(test)]
pub(crate) use {
    compositor::{
        HudCompositeLayerId, HudCompositeLayerMarker, HUD_COMPOSITE_FOREGROUND_Z,
        HUD_COMPOSITE_RENDER_LAYER,
    },
    modules::{
        agent_row_rect, agent_rows, debug_toolbar_buttons,
        handle_pointer_click as dispatch_hud_pointer_click, handle_scroll as dispatch_hud_scroll,
        AgentListRowSection,
    },
    state::{HudModalState, HudState},
    view_models::{AgentListRowView, ConversationListRowView},
    widgets::HUD_WIDGET_DEFINITIONS as HUD_MODULE_DEFINITIONS,
};
