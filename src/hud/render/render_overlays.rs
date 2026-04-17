use super::*;

/// Draws the centered create-agent dialog modal.
pub(super) fn draw_create_agent_dialog(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    if !app_session.create_agent_dialog.visible {
        return;
    }

    let dialog = &app_session.create_agent_dialog;
    let rect = create_agent_dialog_rect(window);
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);

    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 14.0),
        "Create agent",
        20.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );

    let name_rect = create_agent_name_field_rect(window);
    let kind_options = create_agent_kind_option_rects(window);
    let folder_rect = create_agent_starting_folder_rect(window);
    let create_rect = create_agent_create_button_rect(window);

    painter.label(
        Vec2::new(rect.x + 24.0, name_rect.y + 7.0),
        "Name",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_single_line_dialog_field(
        painter,
        window,
        &dialog.name_field,
        name_rect,
        dialog.focus == CreateAgentDialogField::Name,
    );

    painter.label(
        Vec2::new(rect.x + 24.0, kind_options[0].1.y + 3.0),
        "Type",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    for (kind, option_rect, label) in kind_options {
        let selected = dialog.kind == kind;
        let focused = dialog.focus == CreateAgentDialogField::Kind;
        let square_rect = HudRect {
            x: option_rect.x,
            y: option_rect.y + 2.0,
            w: 16.0,
            h: 16.0,
        };
        painter.fill_rect(
            square_rect,
            if selected {
                HudColors::TEXT
            } else {
                HudColors::BUTTON
            },
            0.0,
        );
        painter.stroke_rect(
            square_rect,
            if focused && selected {
                HudColors::TEXT
            } else {
                HudColors::BUTTON_BORDER
            },
            0.0,
        );
        painter.label(
            Vec2::new(option_rect.x + 26.0, option_rect.y),
            label,
            16.0,
            if selected {
                HudColors::TEXT
            } else {
                HudColors::TEXT_MUTED
            },
            VelloTextAnchor::TopLeft,
        );
    }

    painter.label(
        Vec2::new(rect.x + 24.0, folder_rect.y + 7.0),
        "cwd",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_single_line_dialog_field(
        painter,
        window,
        &dialog.cwd_field.field,
        folder_rect,
        dialog.focus == CreateAgentDialogField::StartingFolder,
    );

    draw_dialog_button_row(
        &mut *painter,
        [(
            create_rect,
            "Create",
            dialog.focus == CreateAgentDialogField::CreateButton,
        )],
    );

    if let Some(error) = dialog.error.as_deref() {
        painter.label(
            Vec2::new(rect.x + 24.0, create_rect.y - 26.0),
            error,
            14.0,
            peniko::Color::from_rgba8(220, 80, 80, 255),
            VelloTextAnchor::TopLeft,
        );
    }
}

/// Draws the centered clone-agent dialog modal.
pub(super) fn draw_clone_agent_dialog(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    if !app_session.clone_agent_dialog.visible {
        return;
    }

    let dialog = &app_session.clone_agent_dialog;
    let rect = clone_agent_dialog_rect(window);
    let name_rect = clone_agent_name_field_rect(window);
    let workdir_rect = clone_agent_workdir_rect(window);
    let clone_rect = clone_agent_submit_button_rect(window);

    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 14.0),
        "Clone agent",
        20.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(rect.x + 24.0, name_rect.y + 7.0),
        "Name",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_single_line_dialog_field(
        painter,
        window,
        &dialog.name_field,
        name_rect,
        dialog.focus == CloneAgentDialogField::Name,
    );

    if dialog.supports_workdir() {
        painter.label(
            Vec2::new(rect.x + 24.0, workdir_rect.y + 3.0),
            "Mode",
            15.0,
            HudColors::TEXT_MUTED,
            VelloTextAnchor::TopLeft,
        );
        painter.fill_rect(
            workdir_rect,
            if dialog.workdir {
                HudColors::TEXT
            } else {
                HudColors::BUTTON
            },
            0.0,
        );
        painter.stroke_rect(
            workdir_rect,
            if dialog.focus == CloneAgentDialogField::Workdir {
                HudColors::TEXT
            } else {
                HudColors::BUTTON_BORDER
            },
            0.0,
        );
        painter.label(
            Vec2::new(workdir_rect.x + workdir_rect.w + 12.0, workdir_rect.y - 1.0),
            "Create workdir",
            16.0,
            if dialog.workdir {
                HudColors::TEXT
            } else {
                HudColors::TEXT_MUTED
            },
            VelloTextAnchor::TopLeft,
        );
    }

    draw_dialog_button_row(
        painter,
        [(
            clone_rect,
            "Clone",
            dialog.focus == CloneAgentDialogField::CloneButton,
        )],
    );

    if let Some(error) = dialog.error.as_deref() {
        painter.label(
            Vec2::new(rect.x + 24.0, clone_rect.y - 26.0),
            error,
            14.0,
            peniko::Color::from_rgba8(220, 80, 80, 255),
            VelloTextAnchor::TopLeft,
        );
    }
}

/// Draws the centered rename-agent dialog modal.
pub(super) fn draw_rename_agent_dialog(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    if !app_session.rename_agent_dialog.visible {
        return;
    }

    let dialog = &app_session.rename_agent_dialog;
    let rect = rename_agent_dialog_rect(window);
    let name_rect = rename_agent_name_field_rect(window);
    let rename_rect = rename_agent_submit_button_rect(window);

    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 14.0),
        "Rename agent",
        20.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(rect.x + 24.0, name_rect.y + 7.0),
        "Name",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_single_line_dialog_field(
        painter,
        window,
        &dialog.name_field,
        name_rect,
        dialog.focus == RenameAgentDialogField::Name,
    );
    draw_dialog_button_row(
        painter,
        [(
            rename_rect,
            "Rename",
            dialog.focus == RenameAgentDialogField::RenameButton,
        )],
    );

    if let Some(error) = dialog.error.as_deref() {
        painter.label(
            Vec2::new(rect.x + 24.0, rename_rect.y - 26.0),
            error,
            14.0,
            peniko::Color::from_rgba8(220, 80, 80, 255),
            VelloTextAnchor::TopLeft,
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared editor dialog shell intentionally owns the common title/body/button/footer/error surface"
)]
fn draw_text_editor_dialog<I, L>(
    painter: &mut HudPainter,
    window: &Window,
    rect: HudRect,
    title: &str,
    editor: &TextEditorState,
    editor_focused: bool,
    buttons: I,
    footer_text: Option<&str>,
    error_text: Option<&str>,
) where
    I: IntoIterator<Item = (HudRect, L, bool)>,
    L: Into<String>,
{
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);

    let title_rect = HudRect {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: 44.0,
    };
    painter.fill_rect(title_rect, HudColors::MESSAGE_BOX, 12.0);
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 12.0),
        title,
        18.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );

    let buttons = buttons.into_iter().collect::<Vec<_>>();
    let button_row_y = buttons[0].0.y;
    let info_row_y = button_row_y - 26.0;
    let body_rect = HudRect {
        x: rect.x + 22.0,
        y: rect.y + 64.0,
        w: rect.w - 44.0,
        h: (info_row_y - 12.0 - (rect.y + 64.0)).max(96.0),
    };
    draw_text_editor_body(painter, window, editor, body_rect, editor_focused);
    draw_dialog_button_row(painter, buttons);

    if let Some(footer_text) = footer_text {
        painter.label(
            Vec2::new(rect.x + 24.0, info_row_y),
            footer_text,
            15.0,
            HudColors::TEXT_MUTED,
            VelloTextAnchor::TopLeft,
        );
    }
    if let Some(error_text) = error_text {
        painter.label(
            Vec2::new(rect.x + 24.0, button_row_y - 26.0),
            error_text,
            14.0,
            peniko::Color::from_rgba8(220, 80, 80, 255),
            VelloTextAnchor::TopLeft,
        );
    }
}

/// Draws the message-box modal, including title, editor body, buttons, and status line.
pub(super) fn draw_reset_dialog(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    if !app_session.reset_dialog.visible {
        return;
    }

    let dialog = &app_session.reset_dialog;
    let rect = reset_dialog_rect(window);
    let buttons = reset_dialog_buttons(window);
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 14.0),
        "Reset runtime",
        20.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 56.0),
        "Kill all live agents, terminals, and owned tmux sessions, then rebuild from the saved snapshot.",
        16.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_dialog_button_row(
        painter,
        buttons.into_iter().map(|(target, rect, label)| {
            (
                rect,
                label,
                match target {
                    crate::composer::ResetDialogTarget::CancelButton => {
                        dialog.focus == ResetDialogFocus::CancelButton
                    }
                    crate::composer::ResetDialogTarget::ResetButton => {
                        dialog.focus == ResetDialogFocus::ResetButton
                    }
                },
            )
        }),
    );
}

fn recovery_status_border_color(tone: crate::app::RecoveryStatusTone) -> peniko::Color {
    match tone {
        crate::app::RecoveryStatusTone::Info => HudColors::BORDER,
        crate::app::RecoveryStatusTone::Success => peniko::Color::from_rgba8(34, 120, 54, 255),
        crate::app::RecoveryStatusTone::Error => peniko::Color::from_rgba8(156, 52, 36, 255),
    }
}

pub(super) fn draw_recovery_status_panel(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    let Some(title) = app_session.recovery_status.title.as_deref() else {
        return;
    };

    let text_width = 396.0;
    let (title_rows, _) = wrapped_text_rows_measured(title, title.len(), text_width, |segment| {
        painter.text_size(segment, 18.0).x
    });
    let detail_rows = app_session
        .recovery_status
        .details
        .iter()
        .flat_map(|detail| {
            wrapped_text_rows_measured(detail, detail.len(), text_width, |segment| {
                painter.text_size(segment, 14.0).x
            })
            .0
        })
        .collect::<Vec<_>>();
    let visible_detail_rows = detail_rows.len().min(8);
    let height = 24.0 + title_rows.len() as f32 * 22.0 + visible_detail_rows as f32 * 18.0 + 18.0;
    let rect = HudRect {
        x: window.width() - 452.0,
        y: 72.0,
        w: 420.0,
        h: height.max(84.0),
    };
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(
        rect,
        recovery_status_border_color(app_session.recovery_status.tone),
        12.0,
    );

    let mut y = rect.y + 16.0;
    for row in title_rows {
        painter.label(
            Vec2::new(rect.x + 14.0, y),
            row.display_text,
            18.0,
            HudColors::TEXT,
            VelloTextAnchor::TopLeft,
        );
        y += 22.0;
    }
    for row in detail_rows.into_iter().take(8) {
        painter.label(
            Vec2::new(rect.x + 14.0, y),
            row.display_text,
            14.0,
            HudColors::TEXT_MUTED,
            VelloTextAnchor::TopLeft,
        );
        y += 18.0;
    }
}

pub(super) fn draw_aegis_dialog(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    if !app_session.aegis_dialog.visible {
        return;
    }

    let dialog = &app_session.aegis_dialog;
    let rect = aegis_dialog_rect(window);
    let enable_rect = aegis_enable_button_rect(window);
    let (line_number, column_number) = dialog.prompt_editor.cursor_line_and_column();
    let footer = format!(
        "Ln {} · Col {} · {} · Enter newline · Esc cancel",
        line_number + 1,
        column_number + 1,
        editor_selection_status(&dialog.prompt_editor)
    );
    draw_text_editor_dialog(
        painter,
        window,
        rect,
        "Aegis",
        &dialog.prompt_editor,
        dialog.focus == AegisDialogField::Prompt,
        [(
            enable_rect,
            "Enable",
            dialog.focus == AegisDialogField::EnableButton,
        )],
        Some(&footer),
        dialog.error.as_deref(),
    );
}

pub(super) fn draw_message_box(
    painter: &mut HudPainter,
    window: &Window,
    message_box: &TextEditorState,
    title: &str,
    focus: MessageDialogFocus,
    config: &crate::app_config::NeoZeusConfig,
) {
    if !message_box.visible {
        return;
    }

    let rect = message_box_rect(window);
    let body_rect = crate::composer::message_box_body_rect(window);
    let shortcut_rects = crate::composer::message_box_shortcut_button_rects(window);
    let action_buttons = message_box_action_buttons(window);
    let (line_number, column_number) = message_box.cursor_line_and_column();
    let footer = format!(
        "Ln {} · Col {} · {} · Enter newline · Ctrl-S send · Esc cancel · C-Space mark · C-w cut · M-w copy · C-y yank · M-y ring",
        line_number + 1,
        column_number + 1,
        editor_selection_status(message_box)
    );

    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);

    let title_rect = HudRect {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: 44.0,
    };
    painter.fill_rect(title_rect, HudColors::MESSAGE_BOX, 12.0);
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 12.0),
        title,
        18.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );

    draw_text_editor_body(
        painter,
        window,
        message_box,
        body_rect,
        focus == MessageDialogFocus::Editor,
    );
    draw_dialog_button_row(
        painter,
        shortcut_rects
            .into_iter()
            .zip(config.message_box_shortcuts().iter())
            .map(|(rect, shortcut)| (rect, shortcut.title.clone(), false)),
    );
    draw_dialog_button_row(
        painter,
        action_buttons.into_iter().map(|(action, rect, label)| {
            (
                rect,
                label.to_owned(),
                match action {
                    crate::composer::MessageBoxAction::AppendTask => {
                        focus == MessageDialogFocus::AppendButton
                    }
                    crate::composer::MessageBoxAction::PrependTask => {
                        focus == MessageDialogFocus::PrependButton
                    }
                },
            )
        }),
    );

    let button_row_y = action_buttons[0].1.y;
    let info_row_y = button_row_y - 26.0;
    painter.label(
        Vec2::new(body_rect.x, info_row_y),
        &footer,
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
}

/// Draws the task-dialog modal, which reuses the shared text editor body with different title and
/// button copy.
pub(super) fn draw_task_dialog(
    painter: &mut HudPainter,
    window: &Window,
    task_dialog: &TextEditorState,
    title: &str,
    focus: TaskDialogFocus,
) {
    if !task_dialog.visible {
        return;
    }

    let rect = task_dialog_rect(window);
    let buttons = task_dialog_action_buttons(window);
    let (line_number, column_number) = task_dialog.cursor_line_and_column();
    let footer = format!(
        "Ln {} · Col {} · {} · Format: - [] task or - [ ] task · Ctrl-T clear done · Esc close+persist",
        line_number + 1,
        column_number + 1,
        editor_selection_status(task_dialog)
    );
    draw_text_editor_dialog(
        painter,
        window,
        rect,
        title,
        task_dialog,
        focus == TaskDialogFocus::Editor,
        buttons.into_iter().map(|(action, rect, label)| {
            (
                rect,
                label.to_owned(),
                match action {
                    crate::composer::TaskDialogAction::ClearDone => {
                        focus == TaskDialogFocus::ClearDoneButton
                    }
                },
            )
        }),
        Some(&footer),
        None,
    );
}

/// Rendering is skipped entirely when the dialog is hidden.
fn startup_connect_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * 0.46).clamp(420.0, 760.0),
        (window.height() * 0.22).clamp(180.0, 280.0),
    );
    HudRect {
        x: window.width() * 0.5 - size.x * 0.5,
        y: window.height() * 0.5 - size.y * 0.5,
        w: size.x,
        h: size.y,
    }
}

/// Draws startup connect overlay.
pub(super) fn draw_startup_connect_overlay(
    painter: &mut HudPainter,
    window: &Window,
    startup_connect: &DaemonConnectionState,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    if !startup_connect.modal_visible() {
        return;
    }

    let rect = startup_connect_rect(window);
    let glow_rect = HudRect {
        x: rect.x - 10.0,
        y: rect.y - 10.0,
        w: rect.w + 20.0,
        h: rect.h + 20.0,
    };
    let glow = peniko::Color::from_rgba8(
        modules::AGENT_LIST_BLOOM_RED_R,
        modules::AGENT_LIST_BLOOM_RED_G,
        modules::AGENT_LIST_BLOOM_RED_B,
        255,
    );
    let border = peniko::Color::from_rgba8(
        modules::AGENT_LIST_BORDER_ORANGE_R,
        modules::AGENT_LIST_BORDER_ORANGE_G,
        modules::AGENT_LIST_BORDER_ORANGE_B,
        255,
    );

    // Match the active-agent look: one hard border with a soft emissive halo behind it.
    painter.fill_rect(glow_rect, apply_alpha(glow, 0.18), 0.0);
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 0.0);
    painter.stroke_rect_width(rect, border, 2.5);

    let title = startup_connect.title();
    if !title.is_empty() {
        painter.label(
            Vec2::new(rect.x + rect.w * 0.5, rect.y + 34.0),
            title,
            26.0,
            border,
            VelloTextAnchor::Top,
        );
    }
    painter.label(
        Vec2::new(rect.x + rect.w * 0.5, rect.y + 86.0),
        startup_connect.status(),
        18.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::Top,
    );
    painter.label(
        Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h - 34.0),
        "Window is live. Session restore will begin as soon as the runtime is ready.",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::Bottom,
    );
}
