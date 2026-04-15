use crate::{
    app::{
        AegisDialogField, AppSessionState, CloneAgentDialogField, CreateAgentDialogField,
        RenameAgentDialogField, ResetDialogFocus, TextFieldState,
    },
    composer::{
        aegis_dialog_rect, aegis_enable_button_rect, clone_agent_dialog_rect,
        clone_agent_name_field_rect, clone_agent_submit_button_rect, clone_agent_workdir_rect,
        create_agent_create_button_rect, create_agent_dialog_rect, create_agent_kind_option_rects,
        create_agent_name_field_rect, create_agent_starting_folder_rect,
        message_box_action_buttons, message_box_rect, rename_agent_dialog_rect,
        rename_agent_name_field_rect, rename_agent_submit_button_rect, reset_dialog_buttons,
        reset_dialog_rect, task_dialog_action_buttons, task_dialog_rect,
        wrapped_text_rows_measured, MessageDialogFocus, TaskDialogFocus, TextEditorState,
    },
    startup::DaemonConnectionState,
};

use super::{
    modules,
    state::{
        AgentListUiState, ConversationListUiState, HudLayoutState, HudRect, HUD_TITLEBAR_HEIGHT,
    },
    view_models::{AgentListView, ComposerView, ConversationListView, InfoBarView, ThreadView},
    widgets::HudWidgetKey,
};
use bevy::{prelude::*, window::PrimaryWindow};
use bevy_vello::{
    parley::PositionedLayoutItem,
    prelude::{
        kurbo::{Affine, Line, Rect, RoundedRect, Stroke},
        peniko::{self, Fill},
        vello, VelloFont, VelloScene2d, VelloTextAlign, VelloTextAnchor, VelloTextStyle,
    },
};
mod render_overlays;
mod render_primitives;
mod render_scene_entry;
mod render_text_editors;

use render_scene_entry::{
    render_hud_modal_scene_impl, render_hud_overlay_scene_impl, render_hud_scene_impl,
};

use render_overlays::{
    draw_aegis_dialog, draw_clone_agent_dialog, draw_create_agent_dialog, draw_message_box,
    draw_recovery_status_panel, draw_rename_agent_dialog, draw_reset_dialog,
    draw_startup_connect_overlay, draw_task_dialog,
};

#[cfg(test)]
use render_text_editors::{
    active_line_bounds, cursor_visual_span, single_line_field_viewport, wrapped_editor_rows,
    wrapped_row_is_active, CursorVisualSpan,
};
use render_text_editors::{
    draw_dialog_button_row, draw_single_line_dialog_field, draw_text_editor_body,
    editor_selection_status,
};

pub(crate) use render_primitives::{
    apply_alpha, interpolate_color, HudColors, HudPainter, HudRenderInputs,
};
use render_primitives::{hud_rect_to_scene, log_hud_draw_colors_if_requested};

#[derive(Component)]
pub(crate) struct HudVectorSceneMarker;

#[derive(Component)]
pub(crate) struct HudOverlayVectorSceneMarker;

#[derive(Component)]
pub(crate) struct HudModalVectorSceneMarker;

#[cfg(test)]
pub(crate) const HUD_OVERLAY_CAMERA_ORDER: isize = 70;

#[allow(
    clippy::too_many_arguments,
    reason = "HUD scene rebuild reads HUD, terminal, font, and Vello scene resources together"
)]
pub(crate) fn render_hud_scene(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    agent_list_state: Res<AgentListUiState>,
    conversation_list_state: Res<ConversationListUiState>,
    agent_list_view: Res<AgentListView>,
    conversation_list_view: Res<ConversationListView>,
    thread_view: Res<ThreadView>,
    info_bar_view: Res<InfoBarView>,
    agent_list_text_selection: Res<crate::text_selection::AgentListTextSelectionState>,
    agent_catalog: Option<Res<crate::agents::AgentCatalog>>,
    aegis_policy: Res<crate::aegis::AegisPolicyStore>,
    fonts: Res<Assets<VelloFont>>,
    startup_connect: Option<Res<DaemonConnectionState>>,
    bloom_groups: ResMut<super::HudBloomGroupAuthoring>,
    scene: Single<&mut VelloScene2d, With<HudVectorSceneMarker>>,
) {
    render_hud_scene_impl(
        primary_window,
        layout_state,
        agent_list_state,
        conversation_list_state,
        agent_list_view,
        conversation_list_view,
        thread_view,
        info_bar_view,
        agent_list_text_selection,
        agent_catalog,
        aegis_policy,
        fonts,
        startup_connect,
        bloom_groups,
        scene,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD overlay scene entrypoint forwards layout, selection, and font resources together"
)]
pub(crate) fn render_hud_overlay_scene(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    agent_list_state: Res<AgentListUiState>,
    agent_list_view: Res<AgentListView>,
    selection: Option<Res<crate::hud::view_models::AgentListSelection>>,
    fonts: Res<Assets<VelloFont>>,
    bloom_groups: ResMut<super::HudBloomGroupAuthoring>,
    scene: Single<&mut VelloScene2d, With<HudOverlayVectorSceneMarker>>,
) {
    render_hud_overlay_scene_impl(
        primary_window,
        layout_state,
        agent_list_state,
        agent_list_view,
        selection,
        fonts,
        bloom_groups,
        scene,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD modal scene entrypoint forwards layout, selection, modal, and font resources together"
)]
pub(crate) fn render_hud_modal_scene(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    app_session: Res<AppSessionState>,
    composer_view: Res<ComposerView>,
    startup_connect: Option<Res<DaemonConnectionState>>,
    fonts: Res<Assets<VelloFont>>,
    scene: Single<&mut VelloScene2d, With<HudModalVectorSceneMarker>>,
) {
    render_hud_modal_scene_impl(
        primary_window,
        app_session,
        composer_view,
        startup_connect,
        fonts,
        scene,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        active_line_bounds, cursor_visual_span, render_hud_modal_scene, render_hud_overlay_scene,
        render_hud_scene, single_line_field_viewport, wrapped_editor_rows, wrapped_row_is_active,
        CursorVisualSpan, HudModalVectorSceneMarker, HudOverlayVectorSceneMarker,
        HudVectorSceneMarker,
    };
    use crate::{
        agents::{AgentId, AgentKind},
        hud::{
            default_hud_module_instance, AgentListActivity, AgentListRowKind, AgentListRowView,
            AgentListSelection, AgentListUiState, AgentListView, HudRect, HudState, HudWidgetKey,
            HUD_MODULE_DEFINITIONS,
        },
        shared::daemon_wire::DaemonSessionMetrics,
        tests::insert_test_hud_state,
    };
    use bevy::{ecs::system::RunSystemOnce, prelude::*, window::PrimaryWindow};
    use bevy_vello::prelude::{VelloFont, VelloScene2d};

    #[test]
    fn single_line_field_viewport_keeps_cursor_visible_at_end_of_long_text() {
        let (_start, visible_cursor_col, display) =
            single_line_field_viewport("abcdefghijklmno", 15, 6);
        assert_eq!(display, "klmno");
        assert_eq!(visible_cursor_col, 5);
    }

    #[test]
    fn single_line_field_viewport_handles_utf8_cursor_boundaries() {
        let text = "aébΩz";
        let cursor = text.find('Ω').expect("omega should exist");
        let (_start, visible_cursor_col, display) = single_line_field_viewport(text, cursor, 4);
        assert_eq!(display, "aébΩ");
        assert_eq!(visible_cursor_col, 3);
    }

    #[test]
    fn wrapped_editor_rows_wraps_long_line_and_tracks_cursor_segment() {
        let (rows, cursor_row) = wrapped_editor_rows("abcdefghij", 4, 0, 9);
        let displays = rows.iter().map(|row| row.display_text).collect::<Vec<_>>();
        assert_eq!(displays, vec!["abcd", "efgh", "ij"]);
        assert_eq!(cursor_row, 2);
        assert_eq!(rows[2].cursor_col, Some(1));
    }

    #[test]
    fn wrapped_editor_rows_keeps_cursor_at_end_of_exact_boundary_segment() {
        let (rows, cursor_row) = wrapped_editor_rows("abcdefgh", 4, 0, 8);
        assert_eq!(rows.len(), 2);
        assert_eq!(cursor_row, 1);
        assert_eq!(rows[1].display_text, "efgh");
        assert_eq!(rows[1].cursor_col, Some(4));
    }

    #[test]
    fn wrapped_editor_rows_wraps_whole_words_without_hiding_characters() {
        let (rows, _cursor_row) = wrapped_editor_rows("hello world", 7, 0, 0);
        let displays = rows.iter().map(|row| row.display_text).collect::<Vec<_>>();
        assert_eq!(displays, vec!["hello ", "world"]);
    }

    #[test]
    fn cursor_visual_span_inverts_the_character_under_cursor() {
        assert_eq!(
            cursor_visual_span("abcd", 1),
            CursorVisualSpan::InvertedGlyph {
                leading_text: "a",
                glyph: "b",
                trailing_text: "cd",
            }
        );
    }

    #[test]
    fn wrapped_row_activity_marks_all_visual_rows_of_active_logical_line() {
        let text = "hello world\nnext";
        let (rows, _cursor_row) = wrapped_editor_rows(text, 7, 0, 8);
        let active_line = active_line_bounds(text, 8);
        let active_rows = rows
            .iter()
            .map(|row| wrapped_row_is_active(row, active_line))
            .collect::<Vec<_>>();
        assert_eq!(active_rows, vec![true, true, false]);
    }

    #[test]
    fn render_hud_scene_authors_main_layer_bloom_groups_for_focused_agent() {
        let mut world = World::default();
        let mut hud_state = HudState::default();
        let mut agent_list = default_hud_module_instance(&HUD_MODULE_DEFINITIONS[1]);
        agent_list.shell.set_canonical_rect(
            HudRect {
                x: 0.0,
                y: 0.0,
                w: 320.0,
                h: 220.0,
            },
            true,
        );
        hud_state.insert(HudWidgetKey::AgentList, agent_list);
        insert_test_hud_state(&mut world, hud_state);
        world.insert_resource(Assets::<VelloFont>::default());
        world.insert_resource(crate::hud::HudBloomGroupAuthoring::default());
        world.insert_resource(AgentListView {
            rows: vec![AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(AgentId(1)),
                label: "ALPHA".into(),
                focused: true,
                kind: AgentListRowKind::Agent {
                    agent_id: AgentId(1),
                    terminal_id: Some(crate::terminals::TerminalId(11)),
                    has_tasks: false,
                    interactive: true,
                    activity: AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: AgentKind::Terminal,
                    session_metrics: DaemonSessionMetrics::default(),
                },
            }],
        });
        world.insert_resource(crate::app::AppSessionState::default());
        world.insert_resource(crate::aegis::AegisPolicyStore::default());
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..default()
            },
            PrimaryWindow,
        ));
        world.spawn((VelloScene2d::default(), HudVectorSceneMarker));

        world.run_system_once(render_hud_scene).unwrap();

        assert!(
            world
                .resource::<crate::hud::HudBloomGroupAuthoring>()
                .rects_for(
                    crate::hud::HudLayerId::Main,
                    crate::hud::HudBloomGroupId::AgentListSelection,
                )
                .next()
                .is_some(),
            "main HUD render should author bloom-source rects for focused agent rows"
        );
    }

    #[test]
    fn selected_agent_context_renders_in_overlay_scene_not_modal_scene() {
        let mut world = World::default();
        let mut hud_state = HudState::default();
        let mut agent_list = default_hud_module_instance(&HUD_MODULE_DEFINITIONS[1]);
        agent_list.shell.set_canonical_rect(
            HudRect {
                x: 0.0,
                y: 0.0,
                w: 320.0,
                h: 220.0,
            },
            true,
        );
        hud_state.insert(HudWidgetKey::AgentList, agent_list);
        insert_test_hud_state(&mut world, hud_state);
        world.insert_resource(Assets::<VelloFont>::default());
        world.insert_resource(crate::hud::HudBloomGroupAuthoring::default());
        world.insert_resource(AgentListUiState {
            show_selected_context: true,
            ..Default::default()
        });
        world.insert_resource(AgentListSelection::Agent(AgentId(1)));
        world.insert_resource(AgentListView {
            rows: vec![AgentListRowView {
                key: crate::hud::AgentListRowKey::Agent(AgentId(1)),
                label: "ALPHA".into(),
                focused: true,
                kind: AgentListRowKind::Agent {
                    agent_id: AgentId(1),
                    terminal_id: Some(crate::terminals::TerminalId(11)),
                    has_tasks: false,
                    interactive: true,
                    activity: AgentListActivity::Idle,
                    paused: false,
                    context_pct_milli: None,
                    agent_kind: AgentKind::Terminal,
                    session_metrics: DaemonSessionMetrics::default(),
                },
            }],
        });
        world.insert_resource(crate::hud::ComposerView::default());
        world.insert_resource(crate::app::AppSessionState::default());
        world.spawn((
            Window {
                resolution: (1400, 900).into(),
                ..default()
            },
            PrimaryWindow,
        ));
        let overlay_scene = world
            .spawn((VelloScene2d::default(), HudOverlayVectorSceneMarker))
            .id();
        let modal_scene = world
            .spawn((VelloScene2d::default(), HudModalVectorSceneMarker))
            .id();

        world.run_system_once(render_hud_overlay_scene).unwrap();
        world.run_system_once(render_hud_modal_scene).unwrap();

        assert!(
            !world
                .get::<VelloScene2d>(overlay_scene)
                .expect("overlay scene exists")
                .encoding()
                .is_empty(),
            "selected context should be authored into Overlay"
        );
        assert!(
            world
                .get::<VelloScene2d>(modal_scene)
                .expect("modal scene exists")
                .encoding()
                .is_empty(),
            "selected context should not be authored into Modal"
        );
    }
}
