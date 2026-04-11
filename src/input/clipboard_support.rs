use super::*;

pub(crate) fn paste_into_create_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    match create_agent_dialog_target_at(window, cursor) {
        Some(CreateAgentDialogTarget::NameField) => {
            app_session.create_agent_dialog.focus = CreateAgentDialogField::Name;
            app_session.create_agent_dialog.cwd_field.clear_completion();
            app_session.create_agent_dialog.error = None;
            let text = crate::agents::uppercase_agent_label_text(text);
            app_session
                .create_agent_dialog
                .name_field
                .insert_text(&text)
        }
        Some(CreateAgentDialogTarget::StartingFolderField) => {
            app_session.create_agent_dialog.focus = CreateAgentDialogField::StartingFolder;
            app_session.create_agent_dialog.error = None;
            app_session
                .create_agent_dialog
                .cwd_field
                .mutate_text(|field| field.insert_text(text))
        }
        _ => false,
    }
}

pub(crate) fn paste_into_clone_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    match clone_agent_dialog_target_at(window, cursor) {
        Some(CloneAgentDialogTarget::NameField) => {
            app_session.clone_agent_dialog.focus = CloneAgentDialogField::Name;
            app_session.clone_agent_dialog.error = None;
            let text = crate::agents::uppercase_agent_label_text(text);
            app_session.clone_agent_dialog.name_field.insert_text(&text)
        }
        _ => false,
    }
}

pub(crate) fn paste_into_rename_agent_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    match rename_agent_dialog_target_at(window, cursor) {
        Some(RenameAgentDialogTarget::NameField) => {
            app_session.rename_agent_dialog.focus = RenameAgentDialogField::Name;
            app_session.rename_agent_dialog.error = None;
            let text = crate::agents::uppercase_agent_label_text(text);
            app_session
                .rename_agent_dialog
                .name_field
                .insert_text(&text)
        }
        _ => false,
    }
}

pub(crate) fn paste_into_aegis_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    match aegis_dialog_target_at(window, cursor) {
        Some(AegisDialogTarget::PromptField) => {
            app_session.aegis_dialog.focus = crate::app::AegisDialogField::Prompt;
            app_session.aegis_dialog.error = None;
            app_session.aegis_dialog.prompt_editor.insert_text(text)
        }
        _ => false,
    }
}

pub(crate) fn paste_into_message_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    if message_box_action_at(window, cursor).is_some() || !message_box_rect(window).contains(cursor)
    {
        return false;
    }
    app_session.composer.message_dialog_focus = MessageDialogFocus::Editor;
    app_session.composer.message_editor.insert_text(text)
}

pub(crate) fn paste_into_task_dialog(
    app_session: &mut AppSessionState,
    window: &Window,
    cursor: Vec2,
    text: &str,
) -> bool {
    if task_dialog_action_at(window, cursor).is_some() || !task_dialog_rect(window).contains(cursor)
    {
        return false;
    }
    app_session.composer.task_dialog_focus = TaskDialogFocus::Editor;
    app_session.composer.task_editor.insert_text(text)
}

#[cfg(target_os = "linux")]
pub(super) fn read_linux_primary_selection_text_with(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
    mut run_command: impl FnMut(&str, &[&str]) -> Option<Vec<u8>>,
) -> Option<String> {
    let env = LinuxDisplayEnvironment::new(session_type, wayland_display, display);
    let prefer_wayland = env.prefers_wayland();
    let prefer_x11 = env.prefers_x11();
    let mut candidates = Vec::new();
    if prefer_wayland {
        candidates.push(("wl-paste", ["--primary", "--no-newline"].as_slice()));
    }
    if prefer_x11 {
        candidates.push(("xclip", ["-selection", "primary", "-o"].as_slice()));
        candidates.push(("xsel", ["--primary", "--output"].as_slice()));
    }
    if !prefer_wayland && !prefer_x11 {
        candidates.push(("wl-paste", ["--primary", "--no-newline"].as_slice()));
        candidates.push(("xclip", ["-selection", "primary", "-o"].as_slice()));
        candidates.push(("xsel", ["--primary", "--output"].as_slice()));
    }

    candidates.into_iter().find_map(|(program, args)| {
        let output = run_command(program, args)?;
        if output.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&output).into_owned())
    })
}

#[cfg(target_os = "linux")]
fn read_linux_primary_selection_text() -> Option<String> {
    read_linux_primary_selection_text_with(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY")
            .as_deref()
            .and_then(|value| value.to_str()),
        std::env::var_os("DISPLAY")
            .as_deref()
            .and_then(|value| value.to_str()),
        |program, args| {
            let output = Command::new(program).args(args).output().ok()?;
            if !output.status.success() {
                return None;
            }
            Some(output.stdout)
        },
    )
}

#[cfg(target_os = "linux")]
pub(super) fn write_linux_primary_selection_text_with(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
    text: &str,
    mut run_command: impl FnMut(&str, &[&str], &str) -> bool,
) -> bool {
    let env = LinuxDisplayEnvironment::new(session_type, wayland_display, display);
    let prefer_wayland = env.prefers_wayland();
    let prefer_x11 = env.prefers_x11();
    let mut candidates = Vec::new();
    if prefer_wayland {
        candidates.push(("wl-copy", ["--primary", "--type", "text/plain"].as_slice()));
    }
    if prefer_x11 {
        candidates.push(("xclip", ["-selection", "primary", "-in"].as_slice()));
        candidates.push(("xsel", ["--primary", "--input"].as_slice()));
    }
    if !prefer_wayland && !prefer_x11 {
        candidates.push(("wl-copy", ["--primary", "--type", "text/plain"].as_slice()));
        candidates.push(("xclip", ["-selection", "primary", "-in"].as_slice()));
        candidates.push(("xsel", ["--primary", "--input"].as_slice()));
    }

    candidates
        .into_iter()
        .any(|(program, args)| run_command(program, args, text))
}

#[cfg(target_os = "linux")]
pub(super) fn stop_primary_selection_owner(owner: &mut PrimarySelectionOwnerState) {
    if let Some(mut child) = owner.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(target_os = "linux")]
pub(super) fn write_linux_primary_selection_text(
    owner: &mut PrimarySelectionOwnerState,
    text: &str,
) -> bool {
    let success = write_linux_primary_selection_text_with(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY")
            .as_deref()
            .and_then(|value| value.to_str()),
        std::env::var_os("DISPLAY")
            .as_deref()
            .and_then(|value| value.to_str()),
        text,
        |program, args, text| {
            stop_primary_selection_owner(owner);
            let Ok(mut child) = Command::new(program)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                return false;
            };
            let Some(mut stdin) = child.stdin.take() else {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            };
            if stdin.write_all(text.as_bytes()).is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            drop(stdin);
            owner.child = Some(child);
            true
        },
    );
    if !success {
        stop_primary_selection_owner(owner);
    }
    success
}

fn middle_click_paste_text(clipboard: Option<&mut EguiClipboard>) -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Some(text) = read_linux_primary_selection_text().filter(|text| !text.is_empty()) {
        return Some(text);
    }

    clipboard.and_then(|clipboard| clipboard.get_text().filter(|text| !text.is_empty()))
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal middle-click paste needs layout, focus, hit-testing, runtime, and text together"
)]
pub(crate) fn paste_into_direct_input_terminal(
    window: &Window,
    cursor: Vec2,
    layout_state: &HudLayoutState,
    terminal_manager: &TerminalManager,
    focus_state: &TerminalFocusState,
    input_capture: &HudInputCaptureState,
    panels: &Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
    text: &str,
) -> bool {
    if layout_state.topmost_enabled_at(cursor).is_some() {
        return false;
    }

    let Some(target_terminal) = input_capture.direct_input_terminal else {
        return false;
    };
    if Some(target_terminal) != focus_state.active_id() {
        return false;
    }
    let Some(panel) = topmost_terminal_panel_at_cursor(window, panels, cursor) else {
        return false;
    };
    if panel.id != target_terminal {
        return false;
    }
    let Some(terminal) = terminal_manager.get(target_terminal) else {
        return false;
    };
    if !terminal_is_interactive(&terminal.snapshot.runtime) {
        return false;
    }
    terminal
        .bridge
        .send(TerminalCommand::InputText(text.to_owned()));
    true
}

#[allow(
    clippy::too_many_arguments,
    reason = "middle-click paste needs clipboard, writable target hit-testing, terminal state, and redraws together"
)]
pub(crate) fn handle_middle_click_paste(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    terminal_manager: Res<TerminalManager>,
    focus_state: Res<TerminalFocusState>,
    mut app_session: ResMut<AppSessionState>,
    input_capture: Res<HudInputCaptureState>,
    mut clipboard: Option<ResMut<EguiClipboard>>,
    mut redraws: MessageWriter<RequestRedraw>,
    panels: Query<(&TerminalPanel, &TerminalPresentation, &Visibility)>,
) {
    if !primary_window.focused || !mouse_buttons.just_pressed(MouseButton::Middle) {
        return;
    }
    let Some(cursor) = primary_window.cursor_position() else {
        return;
    };
    let Some(text) = middle_click_paste_text(clipboard.as_deref_mut()) else {
        return;
    };

    let pasted_into_dialog = if app_session.create_agent_dialog.visible {
        paste_into_create_agent_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.clone_agent_dialog.visible {
        paste_into_clone_agent_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.rename_agent_dialog.visible {
        paste_into_rename_agent_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.aegis_dialog.visible {
        paste_into_aegis_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.composer.message_editor.visible {
        paste_into_message_dialog(&mut app_session, &primary_window, cursor, &text)
    } else if app_session.composer.task_editor.visible {
        paste_into_task_dialog(&mut app_session, &primary_window, cursor, &text)
    } else {
        false
    };
    if pasted_into_dialog {
        redraws.write(RequestRedraw);
        return;
    }

    let _ = paste_into_direct_input_terminal(
        &primary_window,
        cursor,
        &layout_state,
        &terminal_manager,
        &focus_state,
        &input_capture,
        &panels,
        &text,
    );
}
