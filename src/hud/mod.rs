mod animation;
mod bloom;
mod capture;
mod compositor;
mod input;
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
    sync_hud_offscreen_compositor, HudCompositeCameraMarker, HudOffscreenCompositor,
};
pub(crate) use input::{handle_hud_module_shortcuts, handle_hud_pointer_input};
pub(crate) use persistence::{save_hud_layout_if_dirty, HudPersistenceState};
pub(crate) use render::{
    render_hud_modal_scene, render_hud_scene, HudModalCameraMarker, HudModalVectorSceneMarker,
};
pub(crate) use setup::{hud_needs_redraw, setup_hud, sync_structural_hud_layout};
pub(crate) use state::{
    AgentListUiState, ConversationListUiState, HudInputCaptureState, HudLayoutState, HudRect,
    InfoBarUiState, TerminalVisibilityPolicy, TerminalVisibilityState, ThreadPaneUiState,
};
pub(crate) use view_models::{
    sync_hud_view_models, sync_info_bar_view_model, AgentListView, ComposerView,
    ConversationListView, InfoBarView, ThreadView,
};

#[cfg(test)]
pub(crate) use view_models::{AgentListRowKey, AgentListRowKind, OwnedTmuxOwnerBinding, UsageBarView};
pub(crate) use widgets::HudWidgetKey;

#[cfg(test)]
pub(crate) use tests::*;

#[cfg(test)]
mod tests;
