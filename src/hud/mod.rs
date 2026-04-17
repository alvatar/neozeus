mod animation;
mod bloom;
mod capture;
mod compositor;
mod input;
mod layer_surface;
mod modules;
mod persistence;
mod render;
mod render_group;
mod render_layer;
mod render_visibility;
mod setup;
mod state;
mod view_models;
mod widgets;

pub(crate) use animation::animate_hud_modules;
pub(crate) use bloom::{
    setup_hud_widget_bloom, sync_hud_widget_bloom, AgentListBloomAdditiveCameraMarker,
    AgentListBloomBlurMaterial, HudBloomLayerConfig, HudBloomSettings, HudWidgetBloom,
};
pub(crate) use capture::{
    finalize_window_capture, request_hud_composite_capture, request_hud_texture_capture,
    request_window_capture, HudCompositeCaptureConfig, HudTextureCaptureConfig,
    WindowCaptureConfig,
};
pub(crate) use compositor::{
    sync_hud_offscreen_compositor, HudCompositeCameraMarker, HudOffscreenCompositor,
};
#[cfg(test)]
pub(crate) use compositor::HUD_COMPOSITE_RENDER_LAYER;
#[cfg(test)]
pub(crate) use compositor::{
    HudCompositeLayerId, HudCompositeLayerMarker, HUD_COMPOSITE_FOREGROUND_Z,
};
#[cfg(test)]
pub(crate) use input::handle_hud_module_shortcuts;
pub(crate) use input::{
    adjacent_agent_list_target, handle_hud_pointer_input, AgentListNavigationTarget,
};
pub(crate) use layer_surface::HudLayerSurfacePlugin;
pub(crate) use persistence::{save_hud_layout_if_dirty, HudPersistenceState};
pub(crate) use render::{render_hud_modal_scene, render_hud_overlay_scene, render_hud_scene};
#[cfg(test)]
pub(crate) use render::{
    HudModalVectorSceneMarker, HudOverlayVectorSceneMarker, HudVectorSceneMarker,
    HUD_OVERLAY_CAMERA_ORDER,
};
pub(crate) use render_group::{HudBloomGroupAuthoring, HudBloomGroupId};
#[cfg(test)]
pub(crate) use render_layer::HudLayerSceneMarker;
pub(crate) use render_layer::{HudLayerId, HudLayerRegistry};
pub(crate) use render_visibility::{
    sync_hud_render_visibility_policy, HudRenderVisibilityPolicy,
};
pub(crate) use setup::{hud_needs_redraw, setup_hud, sync_structural_hud_layout};
pub(crate) use state::{
    AgentListUiState, ConversationListUiState, HudInputCaptureState, HudLayoutState, HudRect,
    InfoBarUiState, TerminalVisibilityPolicy, TerminalVisibilityState, ThreadPaneUiState,
};
pub(crate) use view_models::{
    sync_hud_view_models, sync_info_bar_view_model, AgentListRowKey, AgentListSelection,
    AgentListView, ComposerView, ConversationListView, InfoBarView, ThreadView,
};

#[cfg(test)]
pub(crate) use view_models::{
    selected_agent_id, AgentListActivity, AgentListRowKind, OwnedTmuxOwnerBinding, UsageBarView,
};
pub(crate) use widgets::HudWidgetKey;

#[cfg(test)]
pub(crate) use modules::{handle_pointer_click, handle_scroll};
#[cfg(test)]
pub(in crate::hud) use state::default_hud_module_instance;
#[cfg(test)]
pub(crate) use state::{
    docked_agent_list_rect, docked_agent_list_rect_with_top_inset, docked_info_bar_rect,
    AgentListDragState, HudDragState, HudModalState, HudState,
};
#[cfg(test)]
pub(crate) use view_models::{AgentListRowView, ConversationListRowView};
#[cfg(test)]
pub(crate) use widgets::HUD_WIDGET_DEFINITIONS as HUD_MODULE_DEFINITIONS;
