use super::*;

/// Returns the drawable content rectangle inside a module shell.
///
/// Most modules exclude the titlebar from content rendering; the agent list is full-bleed and keeps
/// the entire shell rect.
fn module_content_rect(module_id: HudWidgetKey, shell_rect: HudRect) -> HudRect {
    if matches!(module_id, HudWidgetKey::AgentList | HudWidgetKey::InfoBar) {
        return shell_rect;
    }
    HudRect {
        x: shell_rect.x,
        y: shell_rect.y + HUD_TITLEBAR_HEIGHT.min(shell_rect.h),
        w: shell_rect.w,
        h: (shell_rect.h - HUD_TITLEBAR_HEIGHT.min(shell_rect.h)).max(0.0),
    }
}

/// Draws the shared shell chrome for a HUD module.
///
/// The agent list intentionally opts out because it has its own custom full-height framing.
fn draw_module_shell(painter: &mut HudPainter, module_id: HudWidgetKey, shell_rect: HudRect) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    if module_id == HudWidgetKey::AgentList {
        return;
    }
    if module_id == HudWidgetKey::InfoBar {
        painter.fill_rect(shell_rect, modules::INFO_BAR_BACKGROUND, 0.0);
        painter.stroke_rect_width(shell_rect, modules::INFO_BAR_BORDER, 1.0);
        return;
    }
    painter.fill_rect(shell_rect, HudColors::FRAME, 8.0);
    painter.stroke_rect(shell_rect, HudColors::BORDER, 8.0);
    painter.fill_rect(
        HudRect {
            x: shell_rect.x,
            y: shell_rect.y,
            w: shell_rect.w,
            h: HUD_TITLEBAR_HEIGHT.min(shell_rect.h),
        },
        HudColors::TITLE,
        8.0,
    );
    painter.label(
        Vec2::new(shell_rect.x + 12.0, shell_rect.y + 8.0),
        &format!("{} {}", module_id.number(), module_id.title()),
        16.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD scene rebuild reads HUD, terminal, font, and Vello scene resources together"
)]
/// Rebuilds the main HUD vector scene from retained module state and live terminal inputs.
///
/// The scene is reconstructed from scratch every frame: each visible module shell is drawn, its
/// content is clipped to the content rect, and module-specific rendering is delegated through the HUD
/// module dispatcher.
pub(super) fn render_hud_scene_impl(
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
    mut scene: Mut<VelloScene2d>,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    let mut built = vello::Scene::new();
    if startup_connect.is_some_and(|state| state.modal_visible()) {
        *scene = VelloScene2d::from(built);
        return;
    }
    let inputs = HudRenderInputs {
        agent_list_view: &agent_list_view,
        conversation_list_view: &conversation_list_view,
        thread_view: &thread_view,
        info_bar_view: &info_bar_view,
        agent_list_text_selection: &agent_list_text_selection,
    };

    for module_id in layout_state.iter_z_order() {
        let Some(module) = layout_state.get(module_id) else {
            continue;
        };
        if !module.shell.enabled && module.shell.current_alpha <= 0.01 {
            continue;
        }

        let shell_rect = module.shell.current_rect;
        let alpha = module.shell.current_alpha.max(0.0);
        let mut painter = HudPainter::new(&mut built, &fonts, &primary_window, alpha);
        draw_module_shell(&mut painter, module_id, shell_rect);

        let content_rect = module_content_rect(module_id, module.shell.current_rect);
        built.push_clip_layer(
            Fill::NonZero,
            Affine::IDENTITY,
            &hud_rect_to_scene(&primary_window, content_rect),
        );
        let mut painter = HudPainter::new(&mut built, &fonts, &primary_window, alpha);
        modules::render_module_content(
            module_id,
            content_rect,
            &mut painter,
            &inputs,
            &agent_list_state,
            &conversation_list_state,
        );
        built.pop_layer();
    }

    log_hud_draw_colors_if_requested(&built);
    *scene = VelloScene2d::from(built);
}

#[allow(
    clippy::too_many_arguments,
    reason = "modal HUD scene rebuild reads modal, layout, selection, and font resources together"
)]
/// Rebuilds the separate modal HUD scene that contains the message box and task dialog overlays.
///
/// Modal rendering is isolated from the main HUD scene so compositor/layer logic can treat it as a
/// separate surface.
pub(super) fn render_hud_modal_scene_impl(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    agent_list_state: Res<AgentListUiState>,
    agent_list_view: Res<AgentListView>,
    selection: Option<Res<crate::hud::view_models::AgentListSelection>>,
    mut bloom_occlusion: ResMut<crate::hud::HudBloomOcclusionState>,
    app_session: Res<AppSessionState>,
    composer_view: Res<ComposerView>,
    startup_connect: Option<Res<DaemonConnectionState>>,
    fonts: Res<Assets<VelloFont>>,
    mut scene: Mut<VelloScene2d>,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    let mut built = vello::Scene::new();
    bloom_occlusion.rect = None;
    let mut painter = HudPainter::new(&mut built, &fonts, &primary_window, 1.0);
    if let Some(startup_connect) = startup_connect.as_deref() {
        draw_startup_connect_overlay(&mut painter, &primary_window, startup_connect);
    }
    draw_recovery_status_panel(&mut painter, &primary_window, &app_session);
    draw_create_agent_dialog(&mut painter, &primary_window, &app_session);
    draw_clone_agent_dialog(&mut painter, &primary_window, &app_session);
    draw_rename_agent_dialog(&mut painter, &primary_window, &app_session);
    draw_reset_dialog(&mut painter, &primary_window, &app_session);
    draw_aegis_dialog(&mut painter, &primary_window, &app_session);
    draw_message_box(
        &mut painter,
        &primary_window,
        &app_session.composer.message_editor,
        composer_view.title.as_deref().unwrap_or("Message"),
        app_session.composer.message_dialog_focus,
    );
    draw_task_dialog(
        &mut painter,
        &primary_window,
        &app_session.composer.task_editor,
        composer_view.title.as_deref().unwrap_or("Tasks"),
        app_session.composer.task_dialog_focus,
    );
    if let Some(agent_list_module) = layout_state.get(HudWidgetKey::AgentList) {
        let agent_list_alpha = agent_list_module.shell.current_alpha.max(0.0);
        if agent_list_module.shell.enabled || agent_list_alpha > 0.01 {
            let mut painter =
                HudPainter::new(&mut built, &fonts, &primary_window, agent_list_alpha);
            bloom_occlusion.rect = modules::render_hover_overlay(
                &primary_window,
                &agent_list_state,
                selection.as_deref(),
                module_content_rect(
                    HudWidgetKey::AgentList,
                    agent_list_module.shell.current_rect,
                ),
                &mut painter,
                &agent_list_view,
            );
        }
    }
    *scene = VelloScene2d::from(built);
}
