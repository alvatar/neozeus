use crate::{
    app::request_final_frame_capture,
    hud::{
        animate_hud_modules, apply_hud_module_requests, apply_terminal_focus_requests,
        apply_terminal_lifecycle_requests, apply_terminal_send_requests,
        apply_terminal_task_requests, apply_terminal_view_requests, apply_visibility_requests,
        dispatch_hud_intents, finalize_window_capture, handle_hud_module_shortcuts,
        handle_hud_pointer_input, render_hud_modal_scene, render_hud_scene,
        request_hud_composite_capture, request_hud_texture_capture, request_window_capture,
        save_hud_layout_if_dirty, setup_hud, setup_hud_widget_bloom, sync_hud_offscreen_compositor,
        sync_hud_widget_bloom, sync_structural_hud_layout,
    },
    input::{
        drag_terminal_view, focus_terminal_on_panel_click, handle_global_terminal_spawn_shortcut,
        handle_terminal_direct_input_keyboard, handle_terminal_lifecycle_shortcuts,
        handle_terminal_message_box_keyboard, hide_terminal_on_background_click,
        zoom_terminal_view,
    },
    startup::{request_redraw_while_visuals_active, setup_scene},
    terminals::{
        configure_terminal_fonts, save_terminal_notes_if_dirty, save_terminal_sessions_if_dirty,
        sync_active_terminal_dimensions, sync_terminal_hud_surface, sync_terminal_panel_frames,
        sync_terminal_presentations, sync_terminal_texture,
    },
    verification::run_verification_scenario,
};
use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum NeoZeusSet {
    PollTerminal,
    RasterTerminal,
    UiInput,
    HudInput,
    HudIntentDispatch,
    HudCommands,
    PresentTerminal,
    HudAnimation,
    HudRender,
    Redraw,
}

/// Configures app schedule.
pub(crate) fn configure_app_schedule(app: &mut App) {
    app.configure_sets(
        Update,
        NeoZeusSet::PollTerminal.before(NeoZeusSet::RasterTerminal),
    )
    .configure_sets(
        Update,
        NeoZeusSet::RasterTerminal.before(NeoZeusSet::PresentTerminal),
    )
    .configure_sets(
        Update,
        NeoZeusSet::UiInput
            .before(NeoZeusSet::RasterTerminal)
            .before(NeoZeusSet::PresentTerminal)
            .before(NeoZeusSet::HudIntentDispatch),
    )
    .configure_sets(
        Update,
        NeoZeusSet::HudInput.before(NeoZeusSet::HudIntentDispatch),
    )
    .configure_sets(
        Update,
        NeoZeusSet::HudIntentDispatch.before(NeoZeusSet::HudCommands),
    )
    .configure_sets(
        Update,
        NeoZeusSet::HudCommands
            .before(NeoZeusSet::RasterTerminal)
            .before(NeoZeusSet::PresentTerminal)
            .before(NeoZeusSet::HudAnimation),
    )
    .configure_sets(
        Update,
        NeoZeusSet::PresentTerminal.before(NeoZeusSet::HudAnimation),
    )
    .configure_sets(
        Update,
        NeoZeusSet::HudAnimation.before(NeoZeusSet::HudRender),
    )
    .configure_sets(Update, NeoZeusSet::HudRender.before(NeoZeusSet::Redraw))
    .add_systems(
        Startup,
        (setup_scene, setup_hud, setup_hud_widget_bloom).chain(),
    )
    .add_systems(PostStartup, sync_hud_offscreen_compositor)
    .add_systems(
        Update,
        sync_structural_hud_layout
            .before(NeoZeusSet::UiInput)
            .before(NeoZeusSet::HudInput)
            .before(NeoZeusSet::PresentTerminal),
    )
    .add_systems(
        Update,
        crate::terminals::poll_terminal_snapshots.in_set(NeoZeusSet::PollTerminal),
    )
    .add_systems(
        Update,
        (
            sync_active_terminal_dimensions,
            configure_terminal_fonts,
            sync_terminal_texture,
        )
            .chain()
            .in_set(NeoZeusSet::RasterTerminal),
    )
    .add_systems(
        Update,
        (
            handle_global_terminal_spawn_shortcut,
            handle_terminal_lifecycle_shortcuts,
            handle_terminal_direct_input_keyboard,
            handle_terminal_message_box_keyboard,
            focus_terminal_on_panel_click,
            hide_terminal_on_background_click,
            drag_terminal_view,
            zoom_terminal_view,
        )
            .in_set(NeoZeusSet::UiInput),
    )
    .add_systems(
        Update,
        (handle_hud_pointer_input, handle_hud_module_shortcuts).in_set(NeoZeusSet::HudInput),
    )
    .add_systems(
        Update,
        dispatch_hud_intents.in_set(NeoZeusSet::HudIntentDispatch),
    )
    .add_systems(
        Update,
        (
            apply_terminal_focus_requests,
            apply_visibility_requests,
            apply_hud_module_requests,
            apply_terminal_view_requests,
            apply_terminal_send_requests,
            apply_terminal_task_requests,
            apply_terminal_lifecycle_requests,
            run_verification_scenario,
        )
            .in_set(NeoZeusSet::HudCommands),
    )
    .add_systems(
        Update,
        (
            sync_terminal_presentations,
            sync_terminal_panel_frames,
            sync_terminal_hud_surface,
        )
            .in_set(NeoZeusSet::PresentTerminal),
    )
    .add_systems(
        Update,
        (
            animate_hud_modules,
            save_hud_layout_if_dirty,
            save_terminal_notes_if_dirty,
            save_terminal_sessions_if_dirty,
        )
            .in_set(NeoZeusSet::HudAnimation),
    )
    .add_systems(
        Update,
        (
            render_hud_scene,
            render_hud_modal_scene,
            sync_hud_offscreen_compositor,
            request_hud_texture_capture,
            request_hud_composite_capture,
            request_window_capture,
            sync_hud_widget_bloom,
            request_final_frame_capture,
        )
            .chain()
            .in_set(NeoZeusSet::HudRender),
    )
    .add_systems(
        Update,
        (request_redraw_while_visuals_active, finalize_window_capture).in_set(NeoZeusSet::Redraw),
    );
}
