use crate::{
    app::save_app_state_if_dirty,
    conversations::{save_conversations_if_dirty, sync_task_notes_projection},
    hud::{
        animate_hud_modules, finalize_window_capture, handle_hud_module_shortcuts,
        handle_hud_pointer_input, render_hud_modal_scene, render_hud_scene,
        request_hud_composite_capture, request_hud_texture_capture, request_window_capture,
        save_hud_layout_if_dirty, setup_hud, setup_hud_widget_bloom, sync_hud_offscreen_compositor,
        sync_hud_view_models, sync_hud_widget_bloom, sync_info_bar_view_model,
        sync_structural_hud_layout,
    },
    input::{
        drag_terminal_view, focus_terminal_on_panel_click, handle_global_terminal_spawn_shortcut,
        handle_terminal_direct_input_keyboard, handle_terminal_lifecycle_shortcuts,
        handle_terminal_message_box_keyboard, hide_terminal_on_background_click,
        zoom_terminal_view,
    },
    startup::{advance_startup_connecting, request_redraw_while_visuals_active, setup_scene},
    terminals::{
        configure_terminal_fonts, save_terminal_notes_if_dirty, sync_active_terminal_dimensions,
        sync_terminal_hud_surface, sync_terminal_panel_frames, sync_terminal_presentations,
        sync_terminal_projection_entities, sync_terminal_texture,
    },
    usage::{refresh_usage_caches_if_needed, sync_usage_snapshot_from_cache},
    verification::run_verification_scenario,
};

use super::{
    dispatch::{apply_app_commands, sync_agents_from_terminals},
    output::request_final_frame_capture,
};
use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum NeoZeusSet {
    PollTerminal,
    RasterTerminal,
    UiInput,
    HudInput,
    AppCommandDispatch,
    UseCaseExecution,
    UsageRefresh,
    PresentTerminal,
    HudAnimation,
    HudRender,
    Redraw,
}

/// Declares the application's update pipeline and wires every system into its stage.
///
/// The ordering here is architectural, not cosmetic: terminal polling must happen before raster,
/// raster before presentation, app commands before use-case execution, and redraw decisions only
/// after both terminal and HUD rendering work has had a chance to update state. The startup chain is
/// also assembled here so scene setup, HUD setup, and bloom setup happen in a deterministic order.
pub(crate) fn configure_app_schedule(app: &mut App) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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
            .before(NeoZeusSet::AppCommandDispatch),
    )
    .configure_sets(
        Update,
        NeoZeusSet::HudInput.before(NeoZeusSet::AppCommandDispatch),
    )
    .configure_sets(
        Update,
        NeoZeusSet::AppCommandDispatch.before(NeoZeusSet::UseCaseExecution),
    )
    .configure_sets(
        Update,
        NeoZeusSet::UseCaseExecution
            .before(NeoZeusSet::RasterTerminal)
            .before(NeoZeusSet::PresentTerminal)
            .before(NeoZeusSet::UsageRefresh)
            .before(NeoZeusSet::HudAnimation),
    )
    .configure_sets(
        Update,
        NeoZeusSet::UsageRefresh
            .before(NeoZeusSet::PresentTerminal)
            .before(NeoZeusSet::HudRender),
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
        advance_startup_connecting
            .before(NeoZeusSet::PollTerminal)
            .before(NeoZeusSet::UiInput)
            .before(NeoZeusSet::PresentTerminal),
    )
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
    .add_systems(Update, sync_hud_view_models.before(NeoZeusSet::HudRender))
    .add_systems(
        Update,
        (
            refresh_usage_caches_if_needed,
            sync_usage_snapshot_from_cache,
            sync_info_bar_view_model,
        )
            .chain()
            .in_set(NeoZeusSet::UsageRefresh),
    )
    .add_systems(
        Update,
        sync_agents_from_terminals.in_set(NeoZeusSet::AppCommandDispatch),
    )
    .add_systems(
        Update,
        (apply_app_commands, run_verification_scenario)
            .chain()
            .in_set(NeoZeusSet::UseCaseExecution),
    )
    .add_systems(
        Update,
        (
            sync_terminal_projection_entities,
            sync_terminal_presentations,
            sync_terminal_panel_frames,
            sync_terminal_hud_surface,
        )
            .chain()
            .in_set(NeoZeusSet::PresentTerminal),
    )
    .add_systems(
        Update,
        (
            animate_hud_modules,
            sync_task_notes_projection,
            save_hud_layout_if_dirty,
            save_terminal_notes_if_dirty,
            save_app_state_if_dirty,
            save_conversations_if_dirty,
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
