use super::{
    surface_from_ansi_text_auto_size, OwnedTmuxSessionStore, TerminalId, TerminalRuntimeSpawner,
    TerminalSurface,
};
use bevy::prelude::*;

const ACTIVE_TMUX_SYNC_INTERVAL_SECS: f32 = 0.5;
const OWNED_TMUX_CAPTURE_LINES: usize = 200;

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct ActiveTerminalContentSyncState {
    last_sync_secs: Option<f32>,
}

/// Holds the currently selected terminal-content override for the active terminal panel.
///
/// The default mode is the focused agent terminal. When a tmux child row is selected, the app
/// stores the selected session id plus the resolved owner terminal here so the terminal renderer can
/// consume an explicit presentation contract instead of reaching back into HUD state.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct ActiveTerminalContentState {
    selected_owned_tmux_session_uid: Option<String>,
    owner_terminal_id: Option<TerminalId>,
    owned_tmux_capture_text: Option<String>,
    owned_tmux_surface: Option<TerminalSurface>,
    last_error: Option<String>,
    presentation_revision: u64,
}

impl ActiveTerminalContentState {
    /// Selects one owned tmux session as the active terminal content override.
    pub(crate) fn select_owned_tmux(
        &mut self,
        session_uid: String,
        owner_terminal_id: Option<TerminalId>,
    ) {
        self.selected_owned_tmux_session_uid = Some(session_uid);
        self.owner_terminal_id = owner_terminal_id;
        self.owned_tmux_capture_text = None;
        self.owned_tmux_surface = None;
        self.last_error = None;
        self.bump_presentation_revision();
    }

    /// Clears any active tmux override and returns control to the focused agent terminal.
    pub(crate) fn clear(&mut self) {
        self.selected_owned_tmux_session_uid = None;
        self.owner_terminal_id = None;
        self.owned_tmux_capture_text = None;
        self.owned_tmux_surface = None;
        self.last_error = None;
        self.bump_presentation_revision();
    }

    /// Returns the selected owned tmux session uid, if the terminal panel is currently overridden.
    pub(crate) fn selected_owned_tmux_session_uid(&self) -> Option<&str> {
        self.selected_owned_tmux_session_uid.as_deref()
    }

    /// Returns the last tmux capture/update error for the active override, if any.
    pub(crate) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Returns the currently selected tmux surface only when it belongs on the provided terminal.
    pub(crate) fn owned_tmux_surface_for(
        &self,
        terminal_id: TerminalId,
    ) -> Option<&TerminalSurface> {
        (self.owner_terminal_id == Some(terminal_id))
            .then_some(self.owned_tmux_surface.as_ref())
            .flatten()
    }

    pub(crate) fn set_last_error(&mut self, error: String) {
        self.last_error = Some(error);
    }

    pub(crate) fn presentation_override_revision_for(
        &self,
        terminal_id: TerminalId,
    ) -> Option<u64> {
        (self.owner_terminal_id == Some(terminal_id)
            && self.selected_owned_tmux_session_uid.is_some())
        .then_some(self.presentation_revision)
    }

    #[cfg(test)]
    pub(crate) fn presentation_revision(&self) -> u64 {
        self.presentation_revision
    }

    fn bump_presentation_revision(&mut self) {
        self.presentation_revision = self.presentation_revision.wrapping_add(1);
    }

    fn set_missing_session_error(&mut self) -> bool {
        let error = Some("Owned tmux session is no longer available".to_owned());
        if self.owned_tmux_capture_text.is_none()
            && self.owned_tmux_surface.is_none()
            && self.last_error == error
        {
            return false;
        }
        self.owned_tmux_capture_text = None;
        self.owned_tmux_surface = None;
        self.last_error = error;
        self.bump_presentation_revision();
        true
    }

    fn update_capture(&mut self, text: String) -> bool {
        if self.owned_tmux_capture_text.as_deref() == Some(text.as_str())
            && self.last_error.is_none()
            && self.owned_tmux_surface.is_some()
        {
            return false;
        }
        let mut surface = surface_from_ansi_text_auto_size(&text);
        surface.cursor = None;
        self.owned_tmux_capture_text = Some(text);
        self.owned_tmux_surface = Some(surface);
        self.last_error = None;
        self.bump_presentation_revision();
        true
    }

    fn update_capture_error(&mut self, error: String) -> bool {
        if self.owned_tmux_capture_text.is_none()
            && self.owned_tmux_surface.is_none()
            && self.last_error.as_deref() == Some(error.as_str())
        {
            return false;
        }
        self.owned_tmux_capture_text = None;
        self.owned_tmux_surface = None;
        self.last_error = Some(error);
        self.bump_presentation_revision();
        true
    }
}

/// Refreshes the active terminal-content override from the currently selected owned tmux session.
pub(crate) fn sync_active_terminal_content(
    time: Res<Time>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    session_store: Res<OwnedTmuxSessionStore>,
    mut sync_state: ResMut<ActiveTerminalContentSyncState>,
    mut active_content: ResMut<ActiveTerminalContentState>,
    mut redraws: MessageWriter<bevy::window::RequestRedraw>,
) {
    let Some(session_uid) = active_content.selected_owned_tmux_session_uid.clone() else {
        return;
    };
    let now_secs = time.elapsed_secs();
    if sync_state
        .last_sync_secs
        .is_some_and(|last_sync_secs| now_secs - last_sync_secs < ACTIVE_TMUX_SYNC_INTERVAL_SECS)
    {
        return;
    }
    sync_state.last_sync_secs = Some(now_secs);

    if !session_store
        .sessions
        .iter()
        .any(|session| session.session_uid == session_uid)
    {
        if active_content.set_missing_session_error() {
            redraws.write(bevy::window::RequestRedraw);
        }
        return;
    }

    let changed =
        match runtime_spawner.capture_owned_tmux_session(&session_uid, OWNED_TMUX_CAPTURE_LINES) {
            Ok(text) => active_content.update_capture(text),
            Err(error) => active_content.update_capture_error(error),
        };
    if changed {
        redraws.write(bevy::window::RequestRedraw);
    }
}
