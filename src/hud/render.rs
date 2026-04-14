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

use render_scene_entry::{render_hud_modal_scene_impl, render_hud_scene_impl};

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
    apply_alpha, interpolate_color, HudColors, HudPainter, HudPainterSet, HudRenderInputs,
};
use render_primitives::{hud_rect_to_scene, log_hud_draw_colors_if_requested};

#[derive(Component)]
pub(crate) struct HudVectorSceneMarker;

#[derive(Component)]
pub(crate) struct HudModalVectorSceneMarker;

#[derive(Component)]
pub(crate) struct HudModalCameraMarker;

pub(crate) const HUD_MODAL_RENDER_LAYER: usize = 33;
pub(crate) const HUD_MODAL_CAMERA_ORDER: isize = 101;

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
    fonts: Res<Assets<VelloFont>>,
    startup_connect: Option<Res<DaemonConnectionState>>,
    surfaces: Res<crate::hud::HudSurfaceRegistry>,
    bloom_groups: Res<crate::hud::HudBloomGroupRegistry>,
    mut bloom_group_state: ResMut<crate::hud::HudBloomGroupRenderState>,
    mut scenes: Query<&mut VelloScene2d>,
) {
    let scene_entity = surfaces
        .scene_entity(crate::hud::render_surface::HudSurfaceId::MainHud)
        .expect("main HUD surface scene should be registered");
    let mut built_bloom_group_scenes = std::collections::BTreeMap::new();
    for group in crate::hud::HudBloomGroupId::ordered_for_surface(
        crate::hud::render_surface::HudSurfaceId::MainHud,
    ) {
        built_bloom_group_scenes.insert(*group, bevy_vello::prelude::vello::Scene::new());
    }
    {
        let mut scene = scenes
            .get_mut(scene_entity)
            .expect("main HUD surface scene entity should exist");
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
            fonts,
            startup_connect,
            &mut scene,
            &mut built_bloom_group_scenes,
        );
    }
    let active_groups = built_bloom_group_scenes
        .iter()
        .filter_map(|(group, scene)| (!scene.encoding().is_empty()).then_some(*group))
        .collect();
    bloom_group_state.set_active_groups(active_groups);
    for (group, built_scene) in built_bloom_group_scenes {
        if let Some(entity) = bloom_groups.scene_entity(group) {
            let mut scene = scenes
                .get_mut(entity)
                .expect("bloom group scene entity should exist");
            *scene = VelloScene2d::from(built_scene);
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD modal scene entrypoint forwards layout, selection, modal, and font resources together"
)]
pub(crate) fn render_hud_modal_scene(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    agent_list_state: Res<AgentListUiState>,
    agent_list_view: Res<AgentListView>,
    selection: Option<Res<crate::hud::view_models::AgentListSelection>>,
    bloom_occlusion: ResMut<crate::hud::HudBloomOcclusionState>,
    app_session: Res<AppSessionState>,
    composer_view: Res<ComposerView>,
    startup_connect: Option<Res<DaemonConnectionState>>,
    fonts: Res<Assets<VelloFont>>,
    surfaces: Res<crate::hud::HudSurfaceRegistry>,
    mut scenes: Query<&mut VelloScene2d>,
) {
    let scene_entity = surfaces
        .scene_entity(crate::hud::render_surface::HudSurfaceId::ModalHud)
        .expect("modal HUD surface scene should be registered");
    let scene = scenes
        .get_mut(scene_entity)
        .expect("modal HUD surface scene entity should exist");
    render_hud_modal_scene_impl(
        primary_window,
        layout_state,
        agent_list_state,
        agent_list_view,
        selection,
        bloom_occlusion,
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
        active_line_bounds, cursor_visual_span, single_line_field_viewport, wrapped_editor_rows,
        wrapped_row_is_active, CursorVisualSpan, HudPainterSet,
    };
    use crate::hud::{HudBloomGroupId, HudRenderRoute, HudSurfaceId};
    use bevy::prelude::{Assets, Window};
    use bevy_vello::prelude::{vello, VelloFont};

    #[test]
    fn bloom_group_contract_is_surface_owned_and_isolated() {
        assert_eq!(
            HudBloomGroupId::AgentListSelection.surface(),
            HudSurfaceId::MainHud
        );
        assert_eq!(
            HudBloomGroupId::AgentListAegis.surface(),
            HudSurfaceId::MainHud
        );
        assert_ne!(
            HudBloomGroupId::AgentListSelection,
            HudBloomGroupId::AgentListAegis
        );
    }

    #[test]
    fn render_route_contract_preserves_surface_and_group_identity() {
        assert_eq!(
            HudRenderRoute::Base {
                surface: HudSurfaceId::ModalHud,
            }
            .surface(),
            HudSurfaceId::ModalHud
        );
        assert_eq!(
            HudRenderRoute::Bloom {
                group: HudBloomGroupId::AgentListSelection,
            }
            .surface(),
            HudSurfaceId::MainHud
        );
    }

    #[test]
    fn painter_set_routes_base_and_bloom_draws_into_separate_scenes() {
        let mut base = vello::Scene::new();
        let mut groups = std::collections::BTreeMap::from([
            (HudBloomGroupId::AgentListSelection, vello::Scene::new()),
            (HudBloomGroupId::AgentListAegis, vello::Scene::new()),
        ]);
        let fonts = Assets::<VelloFont>::default();
        let window = Window::default();
        {
            let mut painters = HudPainterSet::new(&mut base, &mut groups, &fonts, &window, 1.0);
            painters.with_base_painter(|painter| {
                painter.fill_rect(
                    crate::hud::HudRect {
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                    },
                    super::HudColors::TEXT,
                    0.0,
                );
            });
        }
        assert!(!base.encoding().is_empty());
        assert!(groups[&HudBloomGroupId::AgentListSelection]
            .encoding()
            .is_empty());

        {
            let mut painters = HudPainterSet::new(&mut base, &mut groups, &fonts, &window, 1.0);
            painters.with_bloom_group_painter(HudBloomGroupId::AgentListSelection, |painter| {
                painter.fill_rect(
                    crate::hud::HudRect {
                        x: 0.0,
                        y: 0.0,
                        w: 8.0,
                        h: 8.0,
                    },
                    super::HudColors::TEXT_MUTED,
                    0.0,
                );
            });
        }
        assert!(!groups[&HudBloomGroupId::AgentListSelection]
            .encoding()
            .is_empty());
        assert!(groups[&HudBloomGroupId::AgentListAegis]
            .encoding()
            .is_empty());
    }

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
}
