use crate::{
    aegis::advance_aegis_runtime,
    agents::sync_agent_status,
    app::save_app_state_if_dirty,
    conversations::{save_conversations_if_dirty, sync_task_notes_projection},
    hud::{
        animate_hud_modules, finalize_window_capture, handle_hud_pointer_input,
        render_hud_modal_scene, render_hud_scene, request_hud_bloom_group_capture,
        request_hud_composite_capture, request_hud_texture_capture, request_window_capture,
        save_hud_layout_if_dirty, setup_hud, setup_hud_widget_bloom, sync_hud_offscreen_compositor,
        sync_hud_view_models, sync_hud_widget_bloom, sync_info_bar_view_model,
        sync_structural_hud_layout,
    },
    input::{
        drag_terminal_view, focus_terminal_on_panel_click, handle_keyboard_input,
        handle_middle_click_paste, handle_terminal_text_selection,
        hide_terminal_on_background_click, scroll_terminal_with_mouse_wheel,
        sync_primary_selection_from_ui_text_selection, zoom_terminal_view,
    },
    startup::{advance_startup_connecting, request_redraw_while_visuals_active, setup_scene},
    terminals::{
        configure_terminal_fonts, save_terminal_notes_if_dirty, sync_active_terminal_dimensions,
        sync_live_session_metrics, sync_terminal_hud_surface, sync_terminal_panel_frames,
        sync_terminal_presentations, sync_terminal_projection_entities, sync_terminal_texture,
    },
    text_selection::sync_terminal_text_selection_to_surface,
    usage::{refresh_usage_caches_if_needed, sync_usage_snapshot_from_cache},
    verification::{run_verification_scenario, sync_verification_capture_barrier},
    visual_contract::sync_visual_contract_state,
};

use super::{
    dispatch::{apply_app_commands, sync_agents_from_terminals},
    output::{finalize_final_frame_capture, request_final_frame_capture},
    session::AppSessionState,
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
    DeriveVisuals,
    PresentTerminal,
    HudAnimation,
    HudRender,
    Redraw,
}

fn advance_recovery_status_timeout(time: Res<Time>, mut app_session: ResMut<AppSessionState>) {
    app_session.recovery_status.tick(time.delta_secs());
}

fn configure_update_set_ordering(app: &mut App) {
    app.configure_sets(
        Update,
        NeoZeusSet::UiInput
            .before(NeoZeusSet::PollTerminal)
            .before(NeoZeusSet::RasterTerminal)
            .before(NeoZeusSet::PresentTerminal)
            .before(NeoZeusSet::AppCommandDispatch),
    )
    .configure_sets(
        Update,
        NeoZeusSet::PollTerminal.before(NeoZeusSet::RasterTerminal),
    )
    .configure_sets(
        Update,
        NeoZeusSet::RasterTerminal.before(NeoZeusSet::PresentTerminal),
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
            .before(NeoZeusSet::DeriveVisuals)
            .before(NeoZeusSet::RasterTerminal)
            .before(NeoZeusSet::PresentTerminal)
            .before(NeoZeusSet::UsageRefresh)
            .before(NeoZeusSet::HudAnimation),
    )
    .configure_sets(
        Update,
        NeoZeusSet::DeriveVisuals
            .before(NeoZeusSet::UsageRefresh)
            .before(NeoZeusSet::PresentTerminal)
            .before(NeoZeusSet::HudRender),
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
    .configure_sets(Update, NeoZeusSet::HudRender.before(NeoZeusSet::Redraw));
}

/// Declares the application's update pipeline and wires every system into its stage.
///
/// The ordering here is architectural, not cosmetic: user input must land before terminal polling so
/// the same update can observe freshly echoed terminal state, terminal polling must happen before
/// raster, raster before presentation, app commands before use-case execution, and redraw decisions
/// only after both terminal and HUD rendering work has had a chance to update state. The startup
/// chain is also assembled here so scene setup, HUD setup, and bloom setup happen in a deterministic
/// order.
pub(crate) fn configure_app_schedule(app: &mut App) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    configure_update_set_ordering(app);
    app.add_systems(
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
        (
            crate::terminals::poll_terminal_snapshots,
            sync_terminal_text_selection_to_surface,
        )
            .chain()
            .in_set(NeoZeusSet::PollTerminal),
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
            handle_keyboard_input,
            handle_middle_click_paste,
            handle_terminal_text_selection,
            focus_terminal_on_panel_click,
            hide_terminal_on_background_click,
            drag_terminal_view,
            scroll_terminal_with_mouse_wheel,
            zoom_terminal_view,
        )
            .in_set(NeoZeusSet::UiInput),
    )
    .add_systems(
        Update,
        handle_hud_pointer_input.in_set(NeoZeusSet::HudInput),
    )
    .add_systems(
        Update,
        sync_primary_selection_from_ui_text_selection.after(NeoZeusSet::HudInput),
    )
    .add_systems(
        Update,
        (
            sync_live_session_metrics,
            sync_agent_status,
            sync_visual_contract_state,
            crate::terminals::sync_owned_tmux_sessions,
            crate::terminals::sync_active_terminal_content,
            sync_hud_view_models,
        )
            .chain()
            .in_set(NeoZeusSet::DeriveVisuals),
    )
    .add_systems(
        Update,
        advance_aegis_runtime
            .after(NeoZeusSet::DeriveVisuals)
            .before(NeoZeusSet::UsageRefresh),
    )
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
            sync_verification_capture_barrier,
        )
            .chain()
            .in_set(NeoZeusSet::PresentTerminal),
    )
    .add_systems(
        Update,
        (
            advance_recovery_status_timeout,
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
            request_hud_bloom_group_capture,
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
        (
            request_redraw_while_visuals_active,
            finalize_window_capture,
            finalize_final_frame_capture,
        )
            .in_set(NeoZeusSet::Redraw),
    );
}

#[cfg(test)]
mod tests {
    use super::{configure_update_set_ordering, NeoZeusSet};
    use bevy::prelude::*;

    #[derive(Resource, Default)]
    struct ExecutionLog(Vec<&'static str>);

    fn record_ui_input(mut log: ResMut<ExecutionLog>) {
        log.0.push("ui-input");
    }

    fn record_poll_terminal(mut log: ResMut<ExecutionLog>) {
        log.0.push("poll-terminal");
    }

    fn record_raster(mut log: ResMut<ExecutionLog>) {
        log.0.push("raster-terminal");
    }

    #[test]
    fn ui_input_runs_before_terminal_poll_and_raster() {
        let mut app = App::new();
        app.insert_resource(ExecutionLog::default());
        configure_update_set_ordering(&mut app);
        app.add_systems(Update, record_ui_input.in_set(NeoZeusSet::UiInput));
        app.add_systems(
            Update,
            record_poll_terminal.in_set(NeoZeusSet::PollTerminal),
        );
        app.add_systems(Update, record_raster.in_set(NeoZeusSet::RasterTerminal));

        app.update();

        assert_eq!(
            app.world().resource::<ExecutionLog>().0,
            vec!["ui-input", "poll-terminal", "raster-terminal"],
            "ui input must run before terminal polling so echoed input can be consumed in the same update"
        );
    }
}
